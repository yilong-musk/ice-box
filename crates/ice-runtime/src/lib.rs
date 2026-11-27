// SPDX-License-Identifier: GPL-3.0-or-later

//! Headless shared runtime for the ice-box desktop shell and CLI.
//!
//! Owns `AppState`, configuration orchestration, capture coordination,
//! application use cases, background watchdogs, and the local authenticated
//! MCP/control server — with zero GUI or Tauri dependencies.

#[cfg(test)]
mod acceptance;
pub mod application;
pub mod autostart;
pub mod capture;
pub mod core_snapshot;
pub mod core_watch;
pub mod helper_install;
pub mod instance;
pub mod log_tail;
pub mod log_view;
pub mod mcp;
pub mod orchestrate;
pub mod proc_memory;
pub mod proxy_terminal;
pub mod rule_set_watch;
pub mod runtime;
pub mod runtime_status;
#[cfg(target_os = "windows")]
pub mod session_end;
pub mod shutdown;
pub mod subscription_watch;
pub mod windows_elevation;
pub mod workers;

use crate::capture::CaptureController;
use crate::core_snapshot::{wrap_core, CoreSnapshotHub};
use crate::runtime::init_logging;
use ice_config::{load_settings_detailed, AppPaths, ErrorCode};
use ice_core::{purge_invalid_pid_file, CoreController, CoreHandle, TrafficMonitor};
use ice_proxy_sys::{create_system_proxy, ProxyEndpoints, SystemProxy};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

pub use application::{AppHost, AppResources};

/// Host shell capabilities reported in status snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HostCapabilities {
    pub launch_at_login_supported: bool,
    pub in_app_update_supported: bool,
    pub tray_display_supported: bool,
}

impl HostCapabilities {
    pub fn desktop(in_app_update_supported: bool) -> Self {
        Self {
            launch_at_login_supported: autostart::SUPPORTED,
            in_app_update_supported,
            tray_display_supported: cfg!(target_os = "macos"),
        }
    }

    pub fn headless() -> Self {
        Self {
            launch_at_login_supported: false,
            in_app_update_supported: false,
            tray_display_supported: false,
        }
    }
}

/// Shared poisoned-mutex error for command, capture, and shutdown paths.
pub fn lock_poisoned(context: &str) -> ice_config::AppError {
    ice_config::AppError::new(
        ice_config::ErrorCode::LockPoisoned,
        format!("internal lock poisoned: {context}"),
    )
}

/// RFC 3339 timestamp for "now", used in log lines and pending records.
/// The epoch stands in if the system clock is outside RFC 3339's year range.
pub fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

/// Panic log path, resolved after `AppPaths` is available in startup. Panics are
/// written here so a crash on Windows (where the GUI release binary has no
/// console and stderr is discarded) still leaves a trace in `ice-box.log`.
static PANIC_LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Record the panic log file path once data-dir resolution succeeds.
pub fn set_panic_log_path(path: PathBuf) {
    let _ = PANIC_LOG_PATH.set(path);
}

/// Route panics to stderr (if any) and append them to the app log file.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic payload".to_string()
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let line = format!("{payload} (at {location})");
        let _ = writeln!(std::io::stderr(), "ice-box PANIC: {line}");
        if let Some(path) = PANIC_LOG_PATH.get() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(file, "{} PANIC {line}", now_rfc3339());
            }
        }
    }));
}

pub struct AppState {
    pub paths: AppPaths,
    pub core: Mutex<Box<dyn CoreHandle>>,
    /// Published on every core transition; `collect_status` never takes `core`.
    pub core_snapshot: Arc<CoreSnapshotHub>,
    pub runtime_status: runtime_status::RuntimeReadModel,
    pub workers: workers::WorkerSupervisor,
    pub proxy: Mutex<Box<dyn SystemProxy>>,
    /// Serializes config mutations (subscriptions, settings, start/stop, node select).
    pub orchestrate: Mutex<()>,
    /// Shown in UI/status when startup proxy crash recovery failed.
    pub proxy_recovery_warning: Mutex<Vec<ice_config::UiMessage>>,
    /// Memoized `is_proxy_live_applied` result (endpoints, checked-at, value); avoids a
    /// `networksetup` subprocess storm from 2s status polling. Invalidated by the
    /// `start` command and on endpoints change (cache key).
    pub proxy_applied_cache: Mutex<Option<(ProxyEndpoints, Instant, bool)>>,
    /// Platform backend capability; set once at process start (`Noop` is false).
    pub system_proxy_available: bool,
    /// Host shell capability flags (desktop vs headless).
    pub capabilities: HostCapabilities,
    /// Set by quit so an in-flight auto-start healthcheck aborts instead of blocking for the full timeout.
    pub shutdown_requested: Arc<AtomicBool>,
    /// Held for app lifetime; released explicitly before an updater restart so
    /// the relaunched binary cannot race on `instance.lock`.
    instance_lock: Mutex<Option<std::fs::File>>,
    /// Persistent Clash `/traffic` stream; survives home-page unmounts.
    pub traffic: TrafficMonitor,
    /// TUN capture runtime controller (`docs/tun.md`): owns the active backend,
    /// the capture state machine, and the recovery journal.
    pub capture: CaptureController,
    /// mtime-keyed cache of the active profile's nodes and groups.
    pub profile_cache: Mutex<Option<application::ProfileCacheEntry>>,
    /// ice-subscription parse cache (SUB-6). Shared with CaptureController.
    pub profile_parse_cache: Arc<ice_engine::ProfileCache>,
    /// Subscription auto-update watchdog liveness (SUB-4).
    pub subscription_watchdog_alive: Arc<AtomicBool>,
    /// Memoized helper-daemon reachability probe (TTL'd, invalidated by install/uninstall).
    pub helper_probe_cache: Mutex<Option<(Instant, bool)>>,
    /// Memoized Windows TUN scheduled-task pin probe (TTL'd, invalidated by ensure/remove).
    pub tun_task_cache: Mutex<Option<(Instant, bool)>>,
    /// Whether the running core supports live mode switches via the Clash API (`PATCH /configs`).
    pub clash_live_mode_cache: Mutex<bool>,
    /// The UI's post-paint capture restore runs once per process.
    pub launch_proxy_restore_attempted: Arc<AtomicBool>,
    /// Shared Clash group cache, Home interest flag, and the slow memory sample.
    pub live_cache: application::LiveCache,
}

impl AppState {
    /// Initialize `AppState` for a real desktop or headless process.
    pub fn bootstrap(
        paths: AppPaths,
        instance_lock: std::fs::File,
        resource_dir: Option<PathBuf>,
        capabilities: HostCapabilities,
    ) -> Result<Arc<Self>, String> {
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let core = bootstrap_data_dir(&paths, shutdown_requested.clone())?;
        let (core, core_snapshot) = wrap_core(core);
        let settings_reset_warning: Vec<ice_config::UiMessage> =
            load_settings_detailed(&paths.settings())
                .reset_reason
                .map(|reason| ErrorCode::SettingsReset.ui_message_detail(reason))
                .into_iter()
                .collect();
        let proxy = create_system_proxy();
        let system_proxy_available = proxy.is_available();
        let profile_parse_cache = Arc::new(ice_engine::ProfileCache::new());
        let capture = CaptureController::with_profile_cache(
            paths.clone(),
            resource_dir,
            Arc::clone(&profile_parse_cache),
        );
        Ok(Arc::new(Self {
            paths,
            core,
            core_snapshot,
            runtime_status: runtime_status::RuntimeReadModel::default(),
            workers: workers::WorkerSupervisor::default(),
            proxy: Mutex::new(proxy),
            orchestrate: Mutex::new(()),
            proxy_recovery_warning: Mutex::new(settings_reset_warning),
            proxy_applied_cache: Mutex::new(None),
            system_proxy_available,
            capabilities,
            shutdown_requested,
            instance_lock: Mutex::new(Some(instance_lock)),
            traffic: TrafficMonitor::new(),
            capture,
            profile_cache: Mutex::new(None),
            profile_parse_cache,
            subscription_watchdog_alive: Arc::new(AtomicBool::new(true)),
            helper_probe_cache: Mutex::new(None),
            tun_task_cache: Mutex::new(None),
            clash_live_mode_cache: Mutex::new(true),
            launch_proxy_restore_attempted: Arc::new(AtomicBool::new(false)),
            live_cache: application::LiveCache::default(),
        }))
    }

    /// Drop the single-instance lock file early (before an in-app update
    /// relaunch spawns the replacement executable).
    pub fn release_instance_lock(&self) {
        if let Ok(mut slot) = self.instance_lock.lock() {
            slot.take();
        }
    }
}

pub fn bootstrap_data_dir(
    paths: &AppPaths,
    shutdown_requested: Arc<AtomicBool>,
) -> Result<Box<dyn CoreHandle>, String> {
    paths
        .ensure_dirs()
        .map_err(|e| format!("ensure data dirs: {e}"))?;

    if let Err(err) = init_logging(Some(&paths.app_log())) {
        eprintln!("ice-box: logging init: {err}");
    }

    if let Err(err) = purge_invalid_pid_file(&paths.pid()) {
        tracing::warn!(error = %err, "failed to purge invalid pid file");
    }

    let mut core = CoreController::new();
    core.set_health_cancel(shutdown_requested);
    Ok(Box::new(core))
}

/// Run the lightweight MCP stdio bridge used by Agent clients.
///
/// The bridge never owns application state; it connects to the MCP listener in
/// the running desktop app or headless daemon so callers share one data
/// directory, one core, and one OS capture session.
pub fn run_mcp_stdio_bridge(args: impl IntoIterator<Item = std::ffi::OsString>) -> i32 {
    mcp::run_stdio_bridge(args)
}

#[cfg(test)]
pub(crate) fn test_instance_lock(paths: &AppPaths) -> Mutex<Option<std::fs::File>> {
    use fs2::FileExt;
    use std::fs::OpenOptions;
    paths.ensure_dirs().expect("dirs");
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(paths.root().join("instance.lock"))
        .expect("lock file");
    file.try_lock_exclusive().expect("lock");
    Mutex::new(Some(file))
}

/// A free loopback port for tests that bind real listeners.
#[cfg(test)]
pub(crate) fn free_loopback_port() -> u16 {
    const RANGE_START: u32 = 20_000;
    const RANGE_END: u32 = 23_000;
    static NEXT: std::sync::Mutex<u32> = std::sync::Mutex::new(RANGE_START);
    let mut next = NEXT.lock().expect("port counter lock");
    for _ in RANGE_START..RANGE_END {
        let port = *next as u16;
        *next = if *next + 1 >= RANGE_END {
            RANGE_START
        } else {
            *next + 1
        };
        if !ice_core::tcp_port_is_in_use("127.0.0.1", port) {
            return port;
        }
    }
    panic!("no free loopback port in {RANGE_START}..{RANGE_END}");
}

/// Default settings with random free loopback ports for the mixed and clash
/// API inbounds.
#[cfg(test)]
pub(crate) fn test_settings() -> ice_config::AppSettings {
    ice_config::AppSettings {
        mixed_port: free_loopback_port(),
        clash_api_port: free_loopback_port(),
        ..ice_config::AppSettings::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ice_config::write_json_atomic;
    use ice_proxy_sys::{
        recover_if_applied, ProxyBackup, ProxyBackupFile, ProxyEndpoints, ProxySysError,
        SystemProxy,
    };
    use std::cell::Cell;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct CountingProxy {
        restore_calls: Cell<usize>,
        apply_calls: Cell<usize>,
    }

    impl SystemProxy for CountingProxy {
        fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
            Ok(ProxyBackup::default())
        }

        fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
            self.apply_calls.set(self.apply_calls.get() + 1);
            Ok(())
        }

        fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
            self.restore_calls.set(self.restore_calls.get() + 1);
            Ok(())
        }
    }

    #[test]
    fn bootstrap_purges_invalid_pid_and_recovers_applied_proxy() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-bootstrap-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let paths = AppPaths::new(&dir);
        paths.ensure_dirs().expect("dirs");
        fs::write(paths.pid(), b"not-a-pid").expect("pid");

        let record = ProxyBackupFile {
            applied: true,
            pending_apply: false,
            applied_at: None,
            endpoints: ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            },
            backup: ProxyBackup::default(),
        };
        write_json_atomic(&paths.proxy_backup(), &record).expect("backup");

        purge_invalid_pid_file(&paths.pid()).expect("purge");
        assert!(!paths.pid().exists());

        let proxy = CountingProxy::default();
        let did = recover_if_applied(&paths.proxy_backup(), &proxy).expect("recover");
        assert!(did);
        assert_eq!(proxy.restore_calls.get(), 1);
        assert_eq!(proxy.apply_calls.get(), 0);

        let after: ProxyBackupFile =
            serde_json::from_str(&fs::read_to_string(paths.proxy_backup()).unwrap()).unwrap();
        assert!(!after.applied);
        assert!(paths.proxy_backup().exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_has_no_tauri_or_gui_deps() {
        let manifest = include_str!("../Cargo.toml");
        for forbidden in ["tauri", "objc2", "webkit", "gtk"] {
            assert!(
                !manifest.contains(forbidden),
                "{forbidden} must not appear in ice-runtime"
            );
        }
    }
}
