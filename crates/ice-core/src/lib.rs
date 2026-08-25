//! Process lifecycle and status state machine for the sing-box core.

mod binary;
mod clash_api;
mod error;
mod health;
mod process;
mod reload;

pub use binary::{
    binary_in_target_root, current_target_dir, resolve_singbox_binary, BUNDLED_SINGBOX_VERSION,
};
pub use clash_api::{
    connection_stats, get_mode, proxy_delay, proxy_groups, select_group, select_outbound, set_mode,
    traffic_sample, ConnectionStats, GroupState, MockClashApi, RecordedRequest, TrafficSample,
    DELAY_TEST_URL, SELECTOR_TAG,
};
pub use error::CoreError;
pub use health::{
    tcp_bind_available, tcp_port_is_in_use, wait_tcp_ready, wait_tcp_ready_until,
    FailingHealthProbe, HealthCancel, HealthEndpoints, HealthProbe, ImmediateHealthProbe,
    SequenceHealthProbe, TcpHealthProbe, HEALTHCHECK_POLL_INTERVAL, HEALTHCHECK_TIMEOUT,
};
pub use process::{
    stop_process, CommandSpawner, ManagedProcess, MockProcess, MockSpawner, ProcessSpawner,
    STOP_GRACE_TIMEOUT,
};
pub use reload::{
    ConfigReloader, MockReloadMode, MockReloader, SignalReloader, WINDOWS_PORT_RELEASE_WAIT,
};

// CoreHandle is defined below with CoreController.

use ice_config::{clear_pid, write_pid};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// High-level runtime status exposed to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error,
}

/// Snapshot returned by status queries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoreState {
    pub status: CoreStatus,
    pub message: Option<String>,
    pub inbound_host: Option<String>,
    pub inbound_port: Option<u16>,
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            status: CoreStatus::Stopped,
            message: None,
            inbound_host: None,
            inbound_port: None,
        }
    }
}

/// Paths and endpoints needed to manage sing-box.
#[derive(Debug, Clone)]
pub struct CorePaths {
    pub binary: PathBuf,
    pub config: PathBuf,
    pub log_file: PathBuf,
    pub pid_file: PathBuf,
    /// Mixed inbound shown to UI after Running.
    pub inbound_host: String,
    pub inbound_port: u16,
    /// Clash API listen used for healthcheck (TCP connect).
    pub clash_api_host: String,
    pub clash_api_port: u16,
    /// When true, mixed inbound binds `0.0.0.0` (LAN share); port probe must check wildcard.
    pub allow_lan: bool,
}

impl CorePaths {
    pub fn health_endpoints(&self) -> HealthEndpoints {
        HealthEndpoints {
            host: self.clash_api_host.clone(),
            port: self.clash_api_port,
        }
    }
}

/// How reload finished when `Ok`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadOutcome {
    /// Clash API PUT succeeded and post-reload healthcheck passed; process kept.
    HotReloaded,
    /// Hot reload failed; process was restarted from `config.json` and is healthy.
    Restarted,
}

/// Operations that may be requested against the state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreOp {
    Start,
    Stop,
    Reload,
}

/// Whether `op` is allowed from `status` (architecture §7.1).
pub fn is_op_allowed(status: CoreStatus, op: CoreOp) -> bool {
    match (status, op) {
        (CoreStatus::Stopped | CoreStatus::Error, CoreOp::Start) => true,
        (CoreStatus::Running, CoreOp::Stop) => true,
        (CoreStatus::Starting, CoreOp::Stop) => true,
        (CoreStatus::Error | CoreStatus::Stopped, CoreOp::Stop) => true, // idempotent
        (CoreStatus::Running, CoreOp::Reload) => true,
        (CoreStatus::Starting | CoreStatus::Stopping, _) => false,
        (CoreStatus::Stopped | CoreStatus::Error, CoreOp::Reload) => false,
        (CoreStatus::Running, CoreOp::Start) => false,
    }
}

fn reject_op(status: CoreStatus, op: CoreOp) -> CoreError {
    CoreError::invalid_state(format!("cannot {op:?} while status is {status:?}"))
}

/// Controller with injectable spawn / health / reload for tests.
pub struct CoreController<S: ProcessSpawner = CommandSpawner, H: HealthProbe = TcpHealthProbe> {
    state: CoreState,
    child: Option<Box<dyn ManagedProcess>>,
    spawner: S,
    health: H,
    reloader: Box<dyn ConfigReloader>,
    health_timeout: Duration,
    stop_grace: Duration,
    /// Set when reload + restart both failed; shell must restore system proxy.
    needs_proxy_restore: bool,
    /// When set, healthchecks abort early so quit is not blocked by the 5s probe.
    health_cancel: Option<Arc<AtomicBool>>,
}

impl Default for CoreController<CommandSpawner, TcpHealthProbe> {
    fn default() -> Self {
        Self::new()
    }
}

impl CoreController<CommandSpawner, TcpHealthProbe> {
    pub fn new() -> Self {
        Self::with_deps(
            CommandSpawner,
            TcpHealthProbe,
            Box::new(SignalReloader),
            HEALTHCHECK_TIMEOUT,
            STOP_GRACE_TIMEOUT,
        )
    }
}

impl<S: ProcessSpawner, H: HealthProbe + 'static> CoreController<S, H> {
    pub fn with_deps(
        spawner: S,
        health: H,
        reloader: Box<dyn ConfigReloader>,
        health_timeout: Duration,
        stop_grace: Duration,
    ) -> Self {
        Self {
            state: CoreState::default(),
            child: None,
            spawner,
            health,
            reloader,
            health_timeout,
            stop_grace,
            needs_proxy_restore: false,
            health_cancel: None,
        }
    }

    /// Install a shared cancel flag checked during healthchecks (desktop quit path).
    pub fn set_health_cancel(&mut self, cancel: Arc<AtomicBool>) {
        self.health_cancel = Some(cancel);
    }

    pub fn state(&self) -> &CoreState {
        &self.state
    }

    pub fn needs_proxy_restore(&self) -> bool {
        self.needs_proxy_restore
    }

    pub fn clear_needs_proxy_restore(&mut self) {
        self.needs_proxy_restore = false;
    }

    /// Enter Starting from Stopped/Error without spawning (for G2.2 / guards).
    pub fn begin_start(&mut self) -> Result<(), CoreError> {
        if !is_op_allowed(self.state.status, CoreOp::Start) {
            return Err(reject_op(self.state.status, CoreOp::Start));
        }
        self.state.status = CoreStatus::Starting;
        self.state.message = None;
        Ok(())
    }

    pub fn start(&mut self, paths: &CorePaths) -> Result<(), CoreError> {
        if !is_op_allowed(self.state.status, CoreOp::Start) {
            return Err(reject_op(self.state.status, CoreOp::Start));
        }

        if !paths.binary.is_file() {
            return Err(CoreError::NotFound(format!(
                "sing-box binary not found at {}",
                paths.binary.display()
            )));
        }

        ensure_listen_ports_free(paths)?;

        self.state.status = CoreStatus::Starting;
        self.state.message = None;
        self.state.inbound_host = None;
        self.state.inbound_port = None;
        self.needs_proxy_restore = false;

        if let Err(err) = self.spawn_and_probe(paths) {
            let msg = err.to_string();
            self.fail(msg.clone());
            return Err(match err {
                CoreError::HealthcheckFailed(_) => CoreError::HealthcheckFailed(msg),
                CoreError::NotFound(_) => err,
                CoreError::SpawnFailed(_) => CoreError::SpawnFailed(msg),
                other => other,
            });
        }

        self.state.status = CoreStatus::Running;
        self.state.message = None;
        self.state.inbound_host = Some(paths.inbound_host.clone());
        self.state.inbound_port = Some(paths.inbound_port);
        tracing::info!(
            host = %paths.inbound_host,
            port = paths.inbound_port,
            "sing-box ready on {}:{}",
            paths.inbound_host,
            paths.inbound_port
        );
        Ok(())
    }

    pub fn stop(&mut self, pid_file: &Path) -> Result<(), CoreError> {
        match self.state.status {
            CoreStatus::Stopping => {
                return Err(reject_op(self.state.status, CoreOp::Stop));
            }
            CoreStatus::Starting => {
                self.state.status = CoreStatus::Stopping;
                self.kill_child_and_clear_pid(pid_file);
                self.state.status = CoreStatus::Stopped;
                self.state.message = None;
                self.clear_inbound();
                self.needs_proxy_restore = false;
                return Ok(());
            }
            CoreStatus::Stopped if self.child.is_none() => {
                let _ = clear_pid(pid_file);
                self.clear_inbound();
                return Ok(());
            }
            CoreStatus::Stopped | CoreStatus::Running | CoreStatus::Error => {}
        }

        self.state.status = CoreStatus::Stopping;
        self.kill_child_and_clear_pid(pid_file);
        self.state.status = CoreStatus::Stopped;
        self.state.message = None;
        self.clear_inbound();
        self.needs_proxy_restore = false;
        Ok(())
    }

    /// Hot-reload via SIGHUP (Unix): sing-box rebuilds itself from `config.json`
    /// in place, keeping the process alive. On failure restart the process
    /// **without** touching system proxy.
    ///
    /// If restart also fails → `Error` and `needs_proxy_restore() == true` for the shell.
    pub fn reload(&mut self, paths: &CorePaths) -> Result<ReloadOutcome, CoreError> {
        if !is_op_allowed(self.state.status, CoreOp::Reload) {
            return Err(reject_op(self.state.status, CoreOp::Reload));
        }

        let signal_result = match self.child.as_mut() {
            Some(child) => self.reloader.reload(child.as_mut()),
            None => Err(CoreError::SpawnFailed(
                "no managed sing-box process to reload".into(),
            )),
        };

        match signal_result {
            Ok(()) => {
                if self.wait_health(&paths.health_endpoints()).is_ok() {
                    tracing::info!("sing-box hot reload ok");
                    return Ok(ReloadOutcome::HotReloaded);
                }
                tracing::warn!("reload signal ok but healthcheck failed; restarting process");
            }
            Err(err) => {
                tracing::warn!(error = %err, "signal reload failed; restarting process");
            }
        }

        self.restart_in_place(paths)
    }

    /// Stop child and start again from `config.json`. Does **not** restore system proxy.
    fn restart_in_place(&mut self, paths: &CorePaths) -> Result<ReloadOutcome, CoreError> {
        // Keep logical Running intent until success/failure; tear down process only.
        self.kill_child_and_clear_pid(&paths.pid_file);

        #[cfg(windows)]
        {
            std::thread::sleep(WINDOWS_PORT_RELEASE_WAIT);
        }

        match self.spawn_and_probe(paths) {
            Ok(()) => {
                self.state.status = CoreStatus::Running;
                self.state.message = None;
                self.state.inbound_host = Some(paths.inbound_host.clone());
                self.state.inbound_port = Some(paths.inbound_port);
                self.needs_proxy_restore = false;
                tracing::info!("sing-box restarted after reload failure");
                Ok(ReloadOutcome::Restarted)
            }
            Err(err) => {
                let msg = err.to_string();
                self.fail(msg.clone());
                self.needs_proxy_restore = true;
                Err(match err {
                    CoreError::HealthcheckFailed(_) => CoreError::HealthcheckFailed(msg),
                    other => other,
                })
            }
        }
    }

    fn spawn_and_probe(&mut self, paths: &CorePaths) -> Result<(), CoreError> {
        let child = match self
            .spawner
            .spawn(&paths.binary, &paths.config, &paths.log_file)
        {
            Ok(c) => c,
            Err(err) => {
                return Err(err);
            }
        };

        let pid = child.id();
        if let Err(err) = write_pid(&paths.pid_file, pid) {
            let mut child = child;
            let _ = stop_process(child.as_mut(), Duration::from_millis(200));
            let _ = clear_pid(&paths.pid_file);
            return Err(CoreError::SpawnFailed(format!("write pid: {err}")));
        }

        self.child = Some(child);

        if let Err(err) = self.wait_health_while_running(paths) {
            self.kill_child_and_clear_pid(&paths.pid_file);
            return Err(err);
        }
        Ok(())
    }

    /// Probe clash API while watching for an early sing-box exit (config/bind errors).
    fn wait_health_while_running(&mut self, paths: &CorePaths) -> Result<(), CoreError> {
        let endpoints = paths.health_endpoints();

        // Unit tests inject Immediate/Failing/Sequence probes and leave `health_cancel`
        // unset; run the probe on a helper thread so we can still poll for early exit
        // without needing a real TCP listener or consuming SequenceHealthProbe twice.
        if self.health_cancel.is_none() {
            if let Some(err) = self.early_exit_health_error(paths) {
                return Err(err);
            }
            let probe = self.health.clone();
            let ep = endpoints.clone();
            let timeout = self.health_timeout;
            let handle = std::thread::spawn(move || probe.wait_ready(&ep, timeout));
            loop {
                if let Some(err) = self.early_exit_health_error(paths) {
                    // Leave the probe thread to finish on its own (at most health_timeout).
                    return Err(err);
                }
                if handle.is_finished() {
                    return match handle.join().unwrap_or_else(|_| {
                        Err(CoreError::HealthcheckFailed(
                            "health probe thread panicked".into(),
                        ))
                    }) {
                        Ok(()) => Ok(()),
                        Err(err) => {
                            Err(self.healthcheck_err_with_exit_hint(paths, err.to_string()))
                        }
                    };
                }
                std::thread::sleep(HEALTHCHECK_POLL_INTERVAL);
            }
        }

        let deadline = Instant::now() + self.health_timeout;
        let mut last_err = String::from("not attempted");

        while Instant::now() < deadline {
            if self
                .health_cancel
                .as_ref()
                .is_some_and(|c| c.load(std::sync::atomic::Ordering::SeqCst))
            {
                return Err(CoreError::HealthcheckFailed(
                    "cancelled while waiting for clash API".into(),
                ));
            }

            if let Some(err) = self.early_exit_health_error(paths) {
                return Err(err);
            }

            match try_tcp_connect_once(&endpoints) {
                Ok(()) => return Ok(()),
                Err(e) => last_err = e,
            }
            std::thread::sleep(HEALTHCHECK_POLL_INTERVAL);
        }

        Err(self.healthcheck_err_with_exit_hint(
            paths,
            format!(
                "timeout after {}ms waiting for {}:{}: {last_err}",
                self.health_timeout.as_millis(),
                endpoints.host,
                endpoints.port
            ),
        ))
    }

    fn early_exit_health_error(&mut self, paths: &CorePaths) -> Option<CoreError> {
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(Some(code)) => {
                let excerpt = singbox_log_failure_excerpt(&paths.log_file);
                Some(CoreError::HealthcheckFailed(format!(
                    "sing-box 启动后立即退出 (code {code}){}",
                    if excerpt.is_empty() {
                        String::new()
                    } else {
                        format!(": {excerpt}")
                    }
                )))
            }
            Ok(None) => None,
            Err(e) => Some(CoreError::SpawnFailed(format!("try_wait: {e}"))),
        }
    }

    fn healthcheck_err_with_exit_hint(&mut self, paths: &CorePaths, base: String) -> CoreError {
        if let Some(err) = self.early_exit_health_error(paths) {
            return err;
        }
        let excerpt = singbox_log_failure_excerpt(&paths.log_file);
        if excerpt.is_empty() {
            CoreError::HealthcheckFailed(base)
        } else {
            CoreError::HealthcheckFailed(format!("{base}; {excerpt}"))
        }
    }

    fn wait_health(&self, endpoints: &crate::HealthEndpoints) -> Result<(), CoreError> {
        match &self.health_cancel {
            // Cancel-aware path used by the desktop shell so quit can abort the 5s probe.
            Some(cancel) => {
                health::wait_tcp_ready_until(endpoints, self.health_timeout, Some(cancel.as_ref()))
            }
            None => self.health.wait_ready(endpoints, self.health_timeout),
        }
    }

    /// On app start: if pid file points at a live sing-box process, kill it and enter Stopped.
    pub fn reclaim_orphan_pid(&mut self, pid_file: &Path) -> Result<(), CoreError> {
        let Some(pid) = ice_config::read_pid(pid_file)
            .map_err(|e| CoreError::SpawnFailed(format!("read pid: {e}")))?
        else {
            return Ok(());
        };

        if pid_is_alive(pid) {
            if looks_like_singbox_process(pid) {
                tracing::warn!(pid, "reclaiming orphan sing-box pid");
                force_kill_pid(pid);
            } else {
                tracing::warn!(
                    pid,
                    "pid file points at unrelated process; clearing pid file only"
                );
            }
        }
        let _ = clear_pid(pid_file);
        self.child = None;
        self.state.status = CoreStatus::Stopped;
        self.clear_inbound();
        Ok(())
    }

    fn kill_child_and_clear_pid(&mut self, pid_file: &Path) {
        if let Some(mut child) = self.child.take() {
            let _ = stop_process(child.as_mut(), self.stop_grace);
        }
        let _ = clear_pid(pid_file);
    }

    fn fail(&mut self, message: String) {
        self.state.status = CoreStatus::Error;
        self.state.message = Some(message);
        self.clear_inbound();
    }

    fn clear_inbound(&mut self) {
        self.state.inbound_host = None;
        self.state.inbound_port = None;
    }

    /// If the managed child exited unexpectedly, transition to `Error` and clear pid file.
    /// Returns `true` when the child was reaped.
    pub fn reap_exited_child(&mut self, pid_file: &Path) -> bool {
        let Some(child) = self.child.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(Some(code)) => {
                self.child = None;
                let _ = clear_pid(pid_file);
                if self.state.status == CoreStatus::Running {
                    self.state.status = CoreStatus::Error;
                    self.state.message =
                        Some(format!("sing-box exited unexpectedly (code {code})"));
                    self.clear_inbound();
                }
                true
            }
            Ok(None) | Err(_) => false,
        }
    }
}

/// Object-safe facade for shell orchestration / tests.
pub trait CoreHandle: Send {
    fn state(&self) -> CoreState;
    fn start(&mut self, paths: &CorePaths) -> Result<(), CoreError>;
    fn stop(&mut self, pid_file: &Path) -> Result<(), CoreError>;
    fn reload(&mut self, paths: &CorePaths) -> Result<ReloadOutcome, CoreError>;
    fn needs_proxy_restore(&self) -> bool;
    fn clear_needs_proxy_restore(&mut self);
    fn reap_exited_child(&mut self, pid_file: &Path) -> bool;
}

impl<S: ProcessSpawner + 'static, H: HealthProbe + 'static> CoreHandle for CoreController<S, H> {
    fn state(&self) -> CoreState {
        self.state().clone()
    }

    fn start(&mut self, paths: &CorePaths) -> Result<(), CoreError> {
        CoreController::start(self, paths)
    }

    fn stop(&mut self, pid_file: &Path) -> Result<(), CoreError> {
        CoreController::stop(self, pid_file)
    }

    fn reload(&mut self, paths: &CorePaths) -> Result<ReloadOutcome, CoreError> {
        CoreController::reload(self, paths)
    }

    fn needs_proxy_restore(&self) -> bool {
        CoreController::needs_proxy_restore(self)
    }

    fn clear_needs_proxy_restore(&mut self) {
        CoreController::clear_needs_proxy_restore(self);
    }

    fn reap_exited_child(&mut self, pid_file: &Path) -> bool {
        CoreController::reap_exited_child(self, pid_file)
    }
}

#[cfg(test)]
impl<S: ProcessSpawner, H: HealthProbe + 'static> CoreController<S, H> {
    /// Test helper: simulate a running core whose child already exited.
    pub(crate) fn inject_exited_child_for_test(&mut self) {
        struct ExitedChild;
        impl ManagedProcess for ExitedChild {
            fn id(&self) -> u32 {
                1
            }
            fn request_terminate(&mut self) -> std::io::Result<()> {
                Ok(())
            }
            fn force_kill(&mut self) -> std::io::Result<()> {
                Ok(())
            }
            fn try_wait(&mut self) -> std::io::Result<Option<i32>> {
                Ok(Some(1))
            }
        }
        self.child = Some(Box::new(ExitedChild));
        self.state.status = CoreStatus::Running;
        self.state.inbound_host = Some("127.0.0.1".into());
        self.state.inbound_port = Some(17890);
    }
}

fn ensure_listen_ports_free(paths: &CorePaths) -> Result<(), CoreError> {
    // Probe the hosts we will healthcheck / show in the UI. allow_lan binds mixed on
    // 0.0.0.0, but a listener on that port is still reachable via 127.0.0.1.
    let checks = [
        ("mixed", paths.inbound_host.as_str(), paths.inbound_port),
        (
            "clash API",
            paths.clash_api_host.as_str(),
            paths.clash_api_port,
        ),
    ];
    for (label, host, port) in checks {
        if tcp_port_is_in_use(host, port) {
            return Err(CoreError::SpawnFailed(format!(
                "{label} 端口 {host}:{port} 已被占用，请关闭占用该端口的程序（如其他代理软件）或在设置中更换端口"
            )));
        }
    }
    // Only when LAN share is on does sing-box bind 0.0.0.0; probing wildcard while
    // allow_lan is off false-positives if another process holds the port on a
    // non-probe address (e.g. another loopback alias or a LAN NIC).
    if paths.allow_lan && !tcp_bind_available("0.0.0.0", paths.inbound_port) {
        return Err(CoreError::SpawnFailed(format!(
            "mixed 端口 {}:{} 无法绑定（可能已被占用），请关闭占用程序或在设置中更换端口",
            "0.0.0.0", paths.inbound_port
        )));
    }
    Ok(())
}

fn try_tcp_connect_once(endpoints: &HealthEndpoints) -> Result<(), String> {
    if !ice_config::is_loopback_host(&endpoints.host) {
        return Err(format!(
            "healthcheck host must be loopback, got {}",
            endpoints.host
        ));
    }
    let addr_str = endpoints.socket_addr_hint();
    let addrs: Vec<_> = addr_str
        .to_socket_addrs()
        .map_err(|e| format!("resolve {addr_str}: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("no addresses for {addr_str}"));
    }
    let mut last = String::from("not attempted");
    for addr in addrs {
        match std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200)) {
            Ok(_) => return Ok(()),
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

fn singbox_log_failure_excerpt(log_file: &Path) -> String {
    let Ok(mut file) = File::open(log_file) else {
        return String::new();
    };
    let Ok(len) = file.seek(SeekFrom::End(0)) else {
        return String::new();
    };
    let window = 8 * 1024u64;
    let start = len.saturating_sub(window);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut raw = Vec::new();
    if file.read_to_end(&mut raw).is_err() {
        return String::new();
    }
    // A fixed byte window may start mid-character; skip leading UTF-8 continuations.
    let text = String::from_utf8_lossy(skip_partial_utf8_prefix(&raw));
    let clean = strip_ansi_light(&text);
    clean
        .lines()
        .rev()
        .find(|line| {
            let upper = line.to_ascii_uppercase();
            upper.contains("FATAL") || upper.contains("BIND:") || upper.contains("LISTEN ")
        })
        .map(|line| line.trim().to_string())
        .unwrap_or_default()
}

/// Skip leading UTF-8 continuation bytes so a mid-rune window still decodes.
fn skip_partial_utf8_prefix(bytes: &[u8]) -> &[u8] {
    let mut i = 0;
    while i < bytes.len() && bytes[i] & 0xc0 == 0x80 {
        i += 1;
    }
    &bytes[i..]
}

fn strip_ansi_light(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c2 in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c2) {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn pid_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid as i32, 0) };
        rc == 0
    }
    #[cfg(windows)]
    {
        unsafe {
            let handle = windows_sys::Win32::System::Threading::OpenProcess(
                windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            );
            if handle.is_null() {
                return false;
            }
            windows_sys::Win32::Foundation::CloseHandle(handle);
            true
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

fn looks_like_singbox_process(pid: u32) -> bool {
    #[cfg(unix)]
    {
        use std::process::Command;
        let output = match Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .output()
        {
            Ok(o) if o.status.success() => o,
            _ => return false,
        };
        let cmd = String::from_utf8_lossy(&output.stdout);
        cmd.contains("sing-box")
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut buf = [0u16; 512];
            let mut size = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
            CloseHandle(handle);
            if ok == 0 {
                return false;
            }
            let path = String::from_utf16_lossy(&buf[..size as usize]);
            path.to_ascii_lowercase().contains("sing-box")
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

fn force_kill_pid(pid: u32) {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
            libc::kill(pid as i32, libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .creation_flags(0x08000000)
            .status();
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ice_config::read_pid;
    use std::fs;
    use std::sync::atomic::Ordering;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-core-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn marker_binary(dir: &Path) -> PathBuf {
        let bin = dir.join("sing-box");
        fs::write(&bin, b"#!/bin/true\n").expect("bin");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&bin).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&bin, perms).unwrap();
        }
        bin
    }

    fn free_loopback_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind ephemeral")
            .local_addr()
            .expect("local_addr")
            .port()
    }

    fn paths_in(dir: &Path, binary: PathBuf) -> CorePaths {
        let inbound_port = free_loopback_port();
        let mut clash_api_port = free_loopback_port();
        while clash_api_port == inbound_port {
            clash_api_port = free_loopback_port();
        }
        CorePaths {
            binary,
            config: dir.join("config.json"),
            log_file: dir.join("logs/sing-box.log"),
            pid_file: dir.join("sing-box.pid"),
            inbound_host: "127.0.0.1".into(),
            inbound_port,
            clash_api_host: "127.0.0.1".into(),
            clash_api_port,
            allow_lan: false,
        }
    }

    fn mock_ctrl<S: ProcessSpawner, H: HealthProbe + 'static>(
        spawner: S,
        health: H,
        reloader: MockReloader,
    ) -> CoreController<S, H> {
        CoreController::with_deps(
            spawner,
            health,
            Box::new(reloader),
            Duration::from_millis(50),
            Duration::from_millis(50),
        )
    }

    // --- G2.1 ---

    #[test]
    fn g2_1_illegal_transitions_leave_state_unchanged() {
        let cases = [
            (CoreStatus::Stopped, CoreOp::Reload),
            (CoreStatus::Error, CoreOp::Reload),
            (CoreStatus::Starting, CoreOp::Start),
            (CoreStatus::Starting, CoreOp::Reload),
            (CoreStatus::Stopping, CoreOp::Start),
            (CoreStatus::Stopping, CoreOp::Reload),
            (CoreStatus::Stopping, CoreOp::Stop),
            (CoreStatus::Running, CoreOp::Start),
        ];

        for (status, op) in cases {
            assert!(
                !is_op_allowed(status, op),
                "{status:?} should reject {op:?}"
            );
        }

        let mut core = mock_ctrl(
            MockSpawner::default(),
            ImmediateHealthProbe,
            MockReloader::default(),
        );
        let before = core.state().clone();
        let err = core
            .reload(&paths_in(&temp_root("reload"), PathBuf::from("/nope")))
            .expect_err("reload");
        assert_eq!(err.code().as_str(), "core.invalid_state");
        assert_eq!(core.state(), &before);

        core.begin_start().unwrap();
        let before = core.state().clone();
        let err = core
            .start(&paths_in(&temp_root("start"), PathBuf::from("/nope")))
            .expect_err("start while starting");
        assert_eq!(err.code().as_str(), "core.invalid_state");
        assert_eq!(core.state().status, before.status);
    }

    #[test]
    fn g2_1_allowed_ops_table() {
        assert!(is_op_allowed(CoreStatus::Stopped, CoreOp::Start));
        assert!(is_op_allowed(CoreStatus::Error, CoreOp::Start));
        assert!(is_op_allowed(CoreStatus::Running, CoreOp::Stop));
        assert!(is_op_allowed(CoreStatus::Running, CoreOp::Reload));
        assert!(is_op_allowed(CoreStatus::Starting, CoreOp::Stop));
        assert!(is_op_allowed(CoreStatus::Stopped, CoreOp::Stop));
        assert!(is_op_allowed(CoreStatus::Error, CoreOp::Stop));
    }

    #[test]
    fn g2_8_stop_during_starting_allowed() {
        let dir = temp_root("stop-starting");
        let paths = paths_in(&dir, PathBuf::from("/unused"));
        let mut core = mock_ctrl(
            MockSpawner::default(),
            ImmediateHealthProbe,
            MockReloader::default(),
        );
        core.begin_start().expect("begin");
        assert_eq!(core.state().status, CoreStatus::Starting);
        core.stop(&paths.pid_file).expect("stop while starting");
        assert_eq!(core.state().status, CoreStatus::Stopped);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn g2_2_stopped_start_enters_starting() {
        let mut core = mock_ctrl(
            MockSpawner::default(),
            ImmediateHealthProbe,
            MockReloader::default(),
        );
        assert_eq!(core.state().status, CoreStatus::Stopped);
        core.begin_start().expect("begin");
        assert_eq!(core.state().status, CoreStatus::Starting);
    }

    #[test]
    fn g2_3_healthcheck_timeout_kills_child_and_errors() {
        let dir = temp_root("hc-fail");
        let bin = marker_binary(&dir);
        let paths = paths_in(&dir, bin);
        fs::write(&paths.config, b"{}").unwrap();

        let spawner = MockSpawner::with_start_pid(7777);
        let killed = spawner.killed_pids.clone();
        let mut core = mock_ctrl(spawner, FailingHealthProbe, MockReloader::default());

        let err = core.start(&paths).expect_err("health");
        assert_eq!(err.code().as_str(), "core.healthcheck_failed");
        assert_eq!(core.state().status, CoreStatus::Error);
        assert!(
            !killed.lock().unwrap().is_empty(),
            "expected mock child to be killed after healthcheck failure"
        );
        assert!(
            read_pid(&paths.pid_file).unwrap().is_none() || !paths.pid_file.exists(),
            "pid file must be cleared"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reap_exited_child_sets_error_status() {
        let dir = temp_root("reap");
        let pid_file = dir.join("sing-box.pid");
        fs::write(&pid_file, b"1").unwrap();

        let mut core = mock_ctrl(
            MockSpawner::default(),
            ImmediateHealthProbe,
            MockReloader::default(),
        );
        core.inject_exited_child_for_test();
        assert!(core.reap_exited_child(&pid_file));
        assert_eq!(core.state().status, CoreStatus::Error);
        assert!(core
            .state()
            .message
            .as_deref()
            .unwrap_or("")
            .contains("exited unexpectedly"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn g2_4_stop_is_idempotent() {
        let dir = temp_root("stop");
        let bin = marker_binary(&dir);
        let paths = paths_in(&dir, bin);
        fs::write(&paths.config, b"{}").unwrap();

        let mut core = mock_ctrl(
            MockSpawner::default(),
            ImmediateHealthProbe,
            MockReloader::default(),
        );
        core.start(&paths).expect("start");
        assert_eq!(core.state().status, CoreStatus::Running);

        core.stop(&paths.pid_file).expect("stop1");
        assert_eq!(core.state().status, CoreStatus::Stopped);
        assert!(core.state().inbound_host.is_none());
        assert!(core.state().inbound_port.is_none());

        core.stop(&paths.pid_file).expect("stop2");
        assert_eq!(core.state().status, CoreStatus::Stopped);
        assert!(core.state().inbound_host.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn g2_5_missing_binary_no_pid_file() {
        let dir = temp_root("nobin");
        let paths = paths_in(&dir, dir.join("missing-binary"));
        fs::write(&paths.config, b"{}").unwrap();

        let mut core = mock_ctrl(
            MockSpawner::default(),
            ImmediateHealthProbe,
            MockReloader::default(),
        );
        let err = core.start(&paths).expect_err("missing");
        assert_eq!(err.code().as_str(), "core.not_found");
        assert!(!paths.pid_file.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn start_rejects_when_mixed_port_already_in_use() {
        let dir = temp_root("port-busy");
        let bin = marker_binary(&dir);
        let paths = paths_in(&dir, bin);
        fs::write(&paths.config, b"{}").unwrap();
        let listener = std::net::TcpListener::bind(format!("127.0.0.1:{}", paths.inbound_port))
            .expect("occupy mixed port");

        let mut core = mock_ctrl(
            MockSpawner::default(),
            ImmediateHealthProbe,
            MockReloader::default(),
        );
        let err = core.start(&paths).expect_err("port busy");
        assert_eq!(err.code().as_str(), "core.spawn_failed");
        assert!(
            err.to_string().contains("已被占用"),
            "message should mention port conflict: {err}"
        );
        drop(listener);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn g2_6_pid_written_on_start_cleared_on_stop() {
        let dir = temp_root("pid");
        let bin = marker_binary(&dir);
        let paths = paths_in(&dir, bin);
        fs::write(&paths.config, b"{}").unwrap();

        let mut core = mock_ctrl(
            MockSpawner::with_start_pid(9999),
            ImmediateHealthProbe,
            MockReloader::default(),
        );
        core.start(&paths).expect("start");
        assert_eq!(read_pid(&paths.pid_file).unwrap(), Some(9999));

        core.stop(&paths.pid_file).expect("stop");
        assert!(read_pid(&paths.pid_file).unwrap().is_none() || !paths.pid_file.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn g2_7_real_singbox_spawn_if_present() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo = manifest.join("../..");
        let third = repo.join("third_party/sing-box");
        let Ok(binary) = resolve_singbox_binary(&third, None) else {
            eprintln!("skip G2.7: no sing-box binary under {}", third.display());
            return;
        };

        let dir = temp_root("real");
        let example = repo.join("configs/examples/minimal-direct.json");
        let config = dir.join("config.json");
        fs::copy(&example, &config).expect("copy config");
        fs::create_dir_all(dir.join("logs")).unwrap();

        let paths = CorePaths {
            binary,
            config,
            log_file: dir.join("logs/sing-box.log"),
            pid_file: dir.join("sing-box.pid"),
            inbound_host: "127.0.0.1".into(),
            inbound_port: 17890,
            clash_api_host: "127.0.0.1".into(),
            clash_api_port: 19090,
            allow_lan: false,
        };

        let mut core = CoreController::new();
        core.start(&paths).expect("real start");
        assert_eq!(core.state().status, CoreStatus::Running);
        assert!(paths.pid_file.exists());
        assert!(paths.log_file.exists());

        core.stop(&paths.pid_file).expect("real stop");
        assert_eq!(core.state().status, CoreStatus::Stopped);
        assert!(core.state().inbound_port.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    // --- G3 ---

    #[test]
    fn g3_1_reload_2xx_keeps_process_running() {
        let dir = temp_root("g3-hot");
        let bin = marker_binary(&dir);
        let paths = paths_in(&dir, bin);
        fs::write(&paths.config, br#"{"ok":true}"#).unwrap();

        let spawner = MockSpawner::with_start_pid(100);
        let killed = spawner.killed_pids.clone();
        let reloader = MockReloader::new(MockReloadMode::Ok);
        let mut core = mock_ctrl(spawner, ImmediateHealthProbe, reloader.clone());

        core.start(&paths).expect("start");
        let pid_before = read_pid(&paths.pid_file).unwrap();
        assert_eq!(core.state().status, CoreStatus::Running);

        let outcome = core.reload(&paths).expect("reload");
        assert_eq!(outcome, ReloadOutcome::HotReloaded);
        assert_eq!(core.state().status, CoreStatus::Running);
        assert_eq!(reloader.call_count(), 1);
        assert!(
            killed.lock().unwrap().is_empty(),
            "hot reload must not kill process: {:?}",
            killed.lock().unwrap()
        );
        assert_eq!(read_pid(&paths.pid_file).unwrap(), pid_before);
        assert!(!core.needs_proxy_restore());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn g3_2_reload_5xx_restarts_without_proxy_restore_flag() {
        let dir = temp_root("g3-5xx");
        let bin = marker_binary(&dir);
        let paths = paths_in(&dir, bin);
        fs::write(&paths.config, br#"{"ok":true}"#).unwrap();

        let spawner = MockSpawner::with_start_pid(200);
        let killed = spawner.killed_pids.clone();
        let mut core = mock_ctrl(
            spawner,
            ImmediateHealthProbe,
            MockReloader::new(MockReloadMode::Http5xx),
        );

        core.start(&paths).expect("start");
        assert_eq!(read_pid(&paths.pid_file).unwrap(), Some(200));

        let outcome = core.reload(&paths).expect("restart ok");
        assert_eq!(outcome, ReloadOutcome::Restarted);
        assert_eq!(core.state().status, CoreStatus::Running);
        assert!(
            killed.lock().unwrap().contains(&200),
            "old pid must be stopped"
        );
        assert_eq!(read_pid(&paths.pid_file).unwrap(), Some(201));
        assert!(!core.needs_proxy_restore());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn g3_3_reload_timeout_same_restart_fallback() {
        let dir = temp_root("g3-timeout");
        let bin = marker_binary(&dir);
        let paths = paths_in(&dir, bin);
        fs::write(&paths.config, br#"{"ok":true}"#).unwrap();

        let mut core = mock_ctrl(
            MockSpawner::with_start_pid(300),
            ImmediateHealthProbe,
            MockReloader::new(MockReloadMode::Timeout),
        );
        core.start(&paths).expect("start");
        let outcome = core.reload(&paths).expect("restart");
        assert_eq!(outcome, ReloadOutcome::Restarted);
        assert_eq!(core.state().status, CoreStatus::Running);
        assert!(!core.needs_proxy_restore());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn g3_4_reload_and_restart_health_fail_sets_needs_proxy_restore() {
        let dir = temp_root("g3-fail");
        let bin = marker_binary(&dir);
        let paths = paths_in(&dir, bin);
        fs::write(&paths.config, br#"{"ok":true}"#).unwrap();

        let health = SequenceHealthProbe::new(vec![
            Ok(()), // start
            Err(CoreError::HealthcheckFailed("restart probe fail".into())),
        ]);
        let mut core = mock_ctrl(
            MockSpawner::with_start_pid(400),
            health,
            MockReloader::new(MockReloadMode::Http5xx),
        );
        core.start(&paths).expect("start");

        let err = core.reload(&paths).expect_err("should fail");
        assert_eq!(err.code().as_str(), "core.healthcheck_failed");
        assert_eq!(core.state().status, CoreStatus::Error);
        assert!(
            core.needs_proxy_restore(),
            "shell must restore proxy after failed restart"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn g3_5_stopped_reload_invalid_state() {
        let mut core = mock_ctrl(
            MockSpawner::default(),
            ImmediateHealthProbe,
            MockReloader::default(),
        );
        let before = core.state().clone();
        let err = core
            .reload(&paths_in(&temp_root("g3-stop"), PathBuf::from("/x")))
            .expect_err("invalid");
        assert_eq!(err.code().as_str(), "core.invalid_state");
        assert_eq!(core.state(), &before);
    }

    #[test]
    fn g3_6_real_singbox_hot_reload_if_present() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo = manifest.join("../..");
        let third = repo.join("third_party/sing-box");
        let Ok(binary) = resolve_singbox_binary(&third, None) else {
            eprintln!("skip G3.6: no sing-box binary under {}", third.display());
            return;
        };

        let dir = temp_root("g3-real");
        let mut cfg: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(repo.join("configs/examples/minimal-direct.json")).unwrap(),
        )
        .unwrap();
        // Unique ports to avoid clash with other tests / apps.
        cfg["inbounds"][0]["listen_port"] = serde_json::json!(27891);
        cfg["experimental"]["clash_api"]["external_controller"] =
            serde_json::json!("127.0.0.1:29091");
        let config = dir.join("config.json");
        fs::write(&config, serde_json::to_string_pretty(&cfg).unwrap()).unwrap();
        fs::create_dir_all(dir.join("logs")).unwrap();

        let paths = CorePaths {
            binary,
            config: config.clone(),
            log_file: dir.join("logs/sing-box.log"),
            pid_file: dir.join("sing-box.pid"),
            inbound_host: "127.0.0.1".into(),
            inbound_port: 27891,
            clash_api_host: "127.0.0.1".into(),
            clash_api_port: 29091,
            allow_lan: false,
        };

        let mut core = CoreController::new();
        core.start(&paths).expect("start");
        #[cfg(unix)]
        let pid_before = read_pid(&paths.pid_file).unwrap();

        cfg["log"]["level"] = serde_json::json!("debug");
        fs::write(&config, serde_json::to_string_pretty(&cfg).unwrap()).unwrap();

        let outcome = core.reload(&paths).expect("reload");
        assert_eq!(core.state().status, CoreStatus::Running);
        // SIGHUP keeps the process (Unix); Windows has no in-process reload and the
        // controller restarts the process from config.json (Slice 4c §9.1 / §9.2).
        #[cfg(unix)]
        assert_eq!(outcome, ReloadOutcome::HotReloaded);
        #[cfg(unix)]
        assert_eq!(read_pid(&paths.pid_file).unwrap(), pid_before);
        #[cfg(windows)]
        assert_eq!(outcome, ReloadOutcome::Restarted);

        // Clash API should still answer. After SIGHUP the HTTP listener can briefly
        // drop in-flight connections while sing-box rebuilds it, so retry until the
        // controller actually serves (TCP connect alone is not enough).
        let url = format!("http://127.0.0.1:{}/configs", paths.clash_api_port);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let body = loop {
            match ureq::get(&url).call() {
                Ok(resp) => match resp.into_string() {
                    Ok(body) => break body,
                    Err(e) => {
                        if std::time::Instant::now() >= deadline {
                            panic!("clash API not ready after reload: {e}");
                        }
                    }
                },
                Err(e) => {
                    if std::time::Instant::now() >= deadline {
                        panic!("clash API not ready after reload: {e}");
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        assert!(body.contains("log-level"), "configs body: {body}");

        core.stop(&paths.pid_file).expect("stop");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mock_spawner_increments_pid() {
        let s = MockSpawner::with_start_pid(1);
        assert_eq!(s.next_pid.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn strip_ansi_light_preserves_utf8() {
        let raw = "\u{1b}[31m致命错误\u{1b}[0m bind: address already in use";
        assert_eq!(
            strip_ansi_light(raw),
            "致命错误 bind: address already in use"
        );
    }

    #[test]
    fn skip_partial_utf8_prefix_drops_leading_continuations() {
        // UTF-8 for "中" is E4 B8 AD; start mid-character with B8 AD + rest.
        let full = "中国".as_bytes();
        let mid = &full[1..];
        let skipped = skip_partial_utf8_prefix(mid);
        assert_eq!(std::str::from_utf8(skipped).unwrap(), "国");
    }

    #[test]
    fn ensure_ports_skips_wildcard_when_allow_lan_off() {
        // Hold the mixed port on another loopback alias so 0.0.0.0 cannot bind,
        // while 127.0.0.1:port stays free — the false-positive case.
        let Ok(holder) = std::net::TcpListener::bind("127.0.0.2:0") else {
            eprintln!("skip: 127.0.0.2 bind unavailable on this host");
            return;
        };
        let port = holder.local_addr().unwrap().port();
        let mut clash_port = free_loopback_port();
        while clash_port == port {
            clash_port = free_loopback_port();
        }
        let dir = temp_root("ports-lan");
        let paths = CorePaths {
            binary: dir.join("x"),
            config: dir.join("c.json"),
            log_file: dir.join("l.log"),
            pid_file: dir.join("p.pid"),
            inbound_host: "127.0.0.1".into(),
            inbound_port: port,
            clash_api_host: "127.0.0.1".into(),
            clash_api_port: clash_port,
            allow_lan: false,
        };
        ensure_listen_ports_free(&paths).expect("loopback-only probe must succeed");

        let lan = CorePaths {
            allow_lan: true,
            ..paths
        };
        let err = ensure_listen_ports_free(&lan).expect_err("wildcard probe must fail");
        assert!(
            err.to_string().contains("0.0.0.0"),
            "unexpected error: {err}"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
