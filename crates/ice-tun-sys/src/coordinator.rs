// SPDX-License-Identifier: GPL-3.0-or-later

//! Core coordination for the native sing-box ownership path (plan §5 T2).
//!
//! macOS T0 lock (§24.5.2): adapter creation, address assignment, and route
//! installation are privileged, so the bundled sing-box must run elevated.
//! Production uses the privileged helper daemon (installed once via
//! launchd); a `sudo` wrapper is dev-only. `ice-tun-sys` never spawns the
//! core itself (architecture §22 keeps it free of `ice-core`); the
//! orchestration layer injects a `CoreCoordinator` that runs the core as
//! root, and sing-box owns the adapter / addresses / routes.

#[cfg(unix)]
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::{ErrorCode, TunError};

/// Coordinates the elevated sing-box process for the native path. The
/// coordinator is the *only* thing that can start / stop the core; the
/// backend journals and verifies what sing-box owns.
pub trait CoreCoordinator {
    /// Start the core with the given runtime config (elevated) and return the
    /// spawned process id. Returns once the core is up and the TUN adapter
    /// exists. `PermissionRequired` when elevation is missing; never retried
    /// automatically.
    fn start_with_config(&mut self, config_path: &Path) -> Result<u32, TunError>;

    /// Stop the core (SIGTERM; sing-box removes its routes and interface).
    /// Idempotent: OK when the core is already stopped.
    fn stop(&mut self) -> Result<(), TunError>;

    /// Set the DNS servers of one named network service (elevated; macOS
    /// `networksetup`). An empty `servers` list clears the override so the
    /// service falls back to its DHCP resolvers. Implementations must run
    /// the command with an argv list — never a shell.
    fn set_dns(&mut self, service: &str, servers: &[String]) -> Result<(), TunError>;
}

/// T2 placeholder: the real privileged runner (helper IPC or the dev `sudo`
/// wrapper) is wired by orchestration in slice T3. Until then, a TUN
/// transition fails cleanly with `tun.permission_required` and no OS
/// mutation happens.
#[derive(Debug, Default)]
pub struct DeferredCoreCoordinator;

impl CoreCoordinator for DeferredCoreCoordinator {
    fn start_with_config(&mut self, _config_path: &Path) -> Result<u32, TunError> {
        Err(TunError::new(
            ErrorCode::TunPermissionRequired,
            "privileged sing-box runner is not wired yet (slice T3): install and authorize the helper, or use the dev sudo path",
        ))
    }

    fn stop(&mut self) -> Result<(), TunError> {
        Ok(())
    }

    fn set_dns(&mut self, _service: &str, _servers: &[String]) -> Result<(), TunError> {
        Err(TunError::new(
            ErrorCode::TunPermissionRequired,
            "privileged DNS mutation is not wired yet (no elevated runner): install and authorize the helper, or use the dev sudo path",
        ))
    }
}

/// Dev-only elevated runner (plan §5 T3 exit gate, macOS live gate).
///
/// Runs the bundled core as root through `sudo -n` so the native sing-box
/// path can be exercised on a real host before the helper (T5)
/// exists. Explicit opt-in only: `create_backend` wires it when
/// `ICE_BOX_TUN_DEV_SUDO` is set; otherwise the fail-closed
/// [`DeferredCoreCoordinator`] stays in place and no OS mutation happens.
///
/// `sudo -n` never prompts: it succeeds only with a cached root credential
/// (`sudo -v` in a terminal) or a NOPASSWD rule, otherwise the transition
/// fails with `tun.permission_required` before any OS mutation. `sudo`
/// execs the command, so the spawned pid is the sing-box pid.
///
/// `stop` also goes through `sudo`: a non-root shell cannot signal a
/// root-owned process, so TERM/KILL are issued as root and liveness is
/// probed with `kill(pid, 0)` (EPERM while alive, ESRCH once gone).
pub struct SudoCoreCoordinator {
    binary: PathBuf,
    log_path: PathBuf,
    pid: Option<u32>,
    launcher_pid: Option<u32>,
    /// Keep the sudo child handle so it can be reaped after the root core
    /// exits. A liveness probe alone cannot distinguish a stale monitor
    /// process from the actual sing-box process on macOS.
    child: Option<Child>,
}

/// How long to wait for the elevated core to stay alive during startup
/// (config/bind errors surface as an early exit) before handing over to the
/// backend's interface verification.
const STARTUP_LIVENESS_WAIT: Duration = Duration::from_millis(2000);
const LIVENESS_POLL: Duration = Duration::from_millis(100);
/// Bounded wait for the root-owned core to die after SIGTERM, then SIGKILL.
const TERM_GRACE: Duration = Duration::from_secs(5);
const KILL_GRACE: Duration = Duration::from_secs(2);

/// Shared verify path for Child-based coordinators (ARCH-5): callers implement
/// only the spawn seam; this waits until the process is still alive after
/// [`STARTUP_LIVENESS_WAIT`].
fn verify_then_start_child(
    spawn: impl FnOnce() -> Result<Child, TunError>,
    log_path: &Path,
) -> Result<Child, TunError> {
    let mut child = spawn()?;
    wait_for_child_liveness(&mut child, log_path)?;
    Ok(child)
}

fn wait_for_child_liveness(child: &mut Child, log_path: &Path) -> Result<(), TunError> {
    let deadline = Instant::now() + STARTUP_LIVENESS_WAIT;
    loop {
        match child.try_wait() {
            Ok(Some(code)) => {
                return Err(TunError::new(
                    ErrorCode::TunHealthcheckFailed,
                    format!(
                        "elevated sing-box exited during startup (code {code}); check {}",
                        log_path.display()
                    ),
                ));
            }
            Ok(None) => {}
            Err(err) => {
                return Err(TunError::new(
                    ErrorCode::TunApplyFailed,
                    format!("poll elevated core: {err}"),
                ));
            }
        }
        if Instant::now() >= deadline {
            return Ok(());
        }
        std::thread::sleep(LIVENESS_POLL);
    }
}

/// Pid-file / scheduled-task handshake: the process is not a local `Child`.
/// Compiled on every host so unit tests can cover the wait; Windows TUN
/// start is the production caller.
#[allow(dead_code)]
fn wait_for_pid_liveness(
    pid: u32,
    log_hint: &Path,
    is_alive: impl Fn(u32) -> bool,
) -> Result<(), TunError> {
    let deadline = Instant::now() + STARTUP_LIVENESS_WAIT;
    while Instant::now() < deadline {
        if !is_alive(pid) {
            return Err(TunError::new(
                ErrorCode::TunHealthcheckFailed,
                format!(
                    "elevated sing-box exited during startup (pid {pid}); check {}",
                    log_hint.display()
                ),
            ));
        }
        std::thread::sleep(LIVENESS_POLL);
    }
    Ok(())
}

impl SudoCoreCoordinator {
    pub fn new(binary: PathBuf, log_path: PathBuf) -> Self {
        Self {
            binary,
            log_path,
            pid: None,
            launcher_pid: None,
            child: None,
        }
    }

    /// Read-only preflight: `sudo -n true`. Fails with
    /// `tun.permission_required` when the cached credential / NOPASSWD rule
    /// is missing, before any process or OS mutation.
    fn check_permission(&self) -> Result<(), TunError> {
        let status = Command::new("sudo")
            .args(["-n", "true"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match status {
            Ok(status) if status.success() => Ok(()),
            Ok(_) => Err(TunError::new(
                ErrorCode::TunPermissionRequired,
                "dev sudo runner needs a cached root credential (`sudo -v`) or a NOPASSWD rule; use scripts/run-acceptance-macos-tun.sh, or authorize the helper",
            )),
            Err(err) => Err(TunError::new(
                ErrorCode::TunPermissionRequired,
                format!("sudo unavailable: {err}"),
            )),
        }
    }

    fn spawn_elevated(&self, config_path: &Path) -> Result<Child, TunError> {
        if !self.binary.is_file() {
            return Err(TunError::new(
                ErrorCode::TunApplyFailed,
                format!(
                    "sing-box binary not found at {} (dev sudo runner)",
                    self.binary.display()
                ),
            ));
        }
        if !config_path.is_file() {
            return Err(TunError::new(
                ErrorCode::TunApplyFailed,
                format!("config not found at {}", config_path.display()),
            ));
        }
        if let Some(parent) = self.log_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                TunError::new(
                    ErrorCode::TunApplyFailed,
                    format!("create log dir {}: {err}", parent.display()),
                )
            })?;
        }
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .map_err(|err| {
                TunError::new(
                    ErrorCode::TunApplyFailed,
                    format!("open core log {}: {err}", self.log_path.display()),
                )
            })?;
        let log_err = log.try_clone().map_err(|err| {
            TunError::new(
                ErrorCode::TunApplyFailed,
                format!("clone core log handle: {err}"),
            )
        })?;
        Command::new("sudo")
            .arg("-n")
            .arg(&self.binary)
            .arg("run")
            .arg("-c")
            .arg(config_path)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .spawn()
            .map_err(|err| {
                TunError::new(
                    ErrorCode::TunApplyFailed,
                    format!(
                        "spawn sudo -n {} run -c {}: {err}",
                        self.binary.display(),
                        config_path.display()
                    ),
                )
            })
    }
}

impl CoreCoordinator for SudoCoreCoordinator {
    fn start_with_config(&mut self, config_path: &Path) -> Result<u32, TunError> {
        self.check_permission()?;
        let child =
            match verify_then_start_child(|| self.spawn_elevated(config_path), &self.log_path) {
                Ok(child) => child,
                Err(err) => {
                    self.pid = None;
                    self.launcher_pid = None;
                    self.child = None;
                    return Err(err);
                }
            };
        let launcher_pid = child.id();
        self.launcher_pid = Some(launcher_pid);
        self.child = Some(child);

        // Depending on the host sudo policy, sudo may remain as a monitor
        // process while sing-box runs as its root-owned child. Track the
        // actual sing-box pid so TERM/KILL cannot leave that child behind.
        let pid = find_singbox_pid(launcher_pid, &self.binary, config_path).unwrap_or(launcher_pid);
        self.pid = Some(pid);
        tracing::info!(
            pid,
            launcher_pid,
            "elevated sing-box started via dev sudo runner"
        );
        Ok(pid)
    }

    fn stop(&mut self) -> Result<(), TunError> {
        let Some(pid) = self.pid else {
            return Ok(());
        };
        // A non-root shell cannot signal a root-owned process; terminate as
        // root. `sudo -n kill` fails harmlessly when the core already exited.
        let term = Command::new("sudo")
            .args(["-n", "kill", "-TERM", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match term {
            Ok(status) if status.success() || !pid_is_alive(pid) => {}
            Ok(_) => {
                return Err(TunError::new(
                    ErrorCode::TunRestoreFailed,
                    format!("sudo kill -TERM {pid} failed"),
                ));
            }
            Err(err) => {
                return Err(TunError::new(
                    ErrorCode::TunRestoreFailed,
                    format!("sudo kill -TERM {pid}: {err}"),
                ));
            }
        }
        #[cfg(unix)]
        self.signal_launcher(libc::SIGTERM, pid);

        let deadline = Instant::now() + TERM_GRACE;
        while Instant::now() < deadline {
            if !pid_is_alive(pid) && self.reap_child_if_exited() {
                self.pid = None;
                self.launcher_pid = None;
                return Ok(());
            }
            std::thread::sleep(LIVENESS_POLL);
        }

        let _ = Command::new("sudo")
            .args(["-n", "kill", "-KILL", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        #[cfg(unix)]
        self.signal_launcher(libc::SIGKILL, pid);
        let deadline = Instant::now() + KILL_GRACE;
        while Instant::now() < deadline {
            if !pid_is_alive(pid) && self.reap_child_if_exited() {
                self.pid = None;
                self.launcher_pid = None;
                return Ok(());
            }
            std::thread::sleep(LIVENESS_POLL);
        }
        // A sudo monitor can outlive its command briefly. Once the root core
        // is gone, reap/terminate only that user-owned launcher process.
        if !pid_is_alive(pid) {
            if let Some(child) = self.child.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            self.child = None;
            self.pid = None;
            self.launcher_pid = None;
            return Ok(());
        }
        Err(TunError::new(
            ErrorCode::TunRecoveryRequired,
            format!("elevated sing-box (pid {pid}) survived TERM and KILL"),
        ))
    }
    fn set_dns(&mut self, service: &str, servers: &[String]) -> Result<(), TunError> {
        crate::helper_protocol::validate_set_dns(service, servers)?;
        self.check_permission()?;
        let mut args = vec!["-n", "networksetup", "-setdnsservers", service];
        if servers.is_empty() {
            // `networksetup` treats the literal "Empty" as "no DNS servers".
            args.push("Empty");
        } else {
            for server in servers {
                args.push(server);
            }
        }
        let status = Command::new("sudo")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match status {
            Ok(status) if status.success() => Ok(()),
            Ok(_) => Err(TunError::new(
                ErrorCode::TunApplyFailed,
                format!("sudo networksetup -setdnsservers {service} failed (exit {status:?})"),
            )),
            Err(err) => Err(TunError::new(
                ErrorCode::TunApplyFailed,
                format!("run sudo networksetup -setdnsservers {service}: {err}"),
            )),
        }
    }
}

impl SudoCoreCoordinator {
    #[cfg(unix)]
    fn signal_launcher(&mut self, signal: i32, core_pid: u32) {
        let Some(launcher_pid) = self.launcher_pid else {
            return;
        };
        if launcher_pid == core_pid {
            return;
        }
        let launcher_alive = self
            .child
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)));
        if launcher_alive {
            let _ = Command::new("sudo")
                .args(["-n", "kill", &signal.to_string(), &launcher_pid.to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    fn reap_child_if_exited(&mut self) -> bool {
        let Some(child) = self.child.as_mut() else {
            return self.pid.is_none_or(|pid| !pid_is_alive(pid));
        };
        match child.try_wait() {
            Ok(Some(_)) => {
                self.child = None;
                true
            }
            Ok(None) => false,
            Err(_) => !self.pid.is_some_and(pid_is_alive),
        }
    }
}

#[cfg(unix)]
fn find_singbox_pid(launcher_pid: u32, binary: &Path, config_path: &Path) -> Option<u32> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid=,command="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let binary = binary.to_string_lossy();
    let config = config_path.to_string_lossy();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse::<u32>().ok()?;
            let _ppid = fields.next()?.parse::<u32>().ok()?;
            let command = fields.collect::<Vec<_>>().join(" ");
            (pid != launcher_pid
                && command.contains(binary.as_ref())
                && command.contains(config.as_ref()))
            .then_some(pid)
        })
        .next()
}

#[cfg(not(unix))]
fn find_singbox_pid(_launcher_pid: u32, _binary: &Path, _config_path: &Path) -> Option<u32> {
    None
}

/// Unix liveness probe: `kill(pid, 0)` succeeds for our own processes and
/// returns EPERM for root-owned ones (alive); ESRCH means gone.
#[cfg(unix)]
fn pid_is_alive(pid: u32) -> bool {
    let rc = unsafe { libc::kill(pid as i32, 0) };
    rc == 0 || io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Non-Unix fallback: the dev `sudo` runner cannot spawn a process on these
/// platforms, so a pid is never tracked and liveness is never consulted in
/// practice; report the process as gone (fail toward completion).
#[cfg(not(unix))]
fn pid_is_alive(_pid: u32) -> bool {
    false
}

/// Elevated runner for the Windows TUN path (`windows_tun_ready` green since
/// 2026-09-03; the app process is not elevated, so the core runs elevated).
///
/// The Windows TUN path requires an Administrator context: the wintun driver
/// is embedded in the bundled sing-box binary, and `WintunCreateAdapter`
/// needs admin. This runner requires the *current* process to already be
/// elevated (the live acceptance suite runs from an Administrator shell) and
/// spawns sing-box directly; the child inherits the elevation. `stop` is
/// graceful-first (`taskkill /T`, which the core uses to remove its WFP
/// filters and routes) with a forced `/F` fallback after `TERM_GRACE` — the
/// strict-route WFP filters must not be stranded, they black-hole host TCP
/// (`docs/tun.md`).
#[cfg(target_os = "windows")]
pub struct WindowsElevatedCoreCoordinator {
    binary: PathBuf,
    log_path: PathBuf,
    child: Option<Child>,
}

#[cfg(target_os = "windows")]
impl WindowsElevatedCoreCoordinator {
    pub fn new(binary: PathBuf, log_path: PathBuf) -> Self {
        Self {
            binary,
            log_path,
            child: None,
        }
    }

    /// Read-only preflight: the current process must carry an elevated
    /// (Administrator) token. Fails with `tun.permission_required` before
    /// any process or OS mutation.
    fn check_elevation(&self) -> Result<(), TunError> {
        if process_is_elevated() {
            Ok(())
        } else {
            Err(TunError::new(
                ErrorCode::TunPermissionRequired,
                "TUN transitions need an elevated context (the core runs elevated to create the wintun adapter); run the acceptance suite from an Administrator shell",
            ))
        }
    }

    fn spawn_elevated(&self, config_path: &Path) -> Result<Child, TunError> {
        if !self.binary.is_file() {
            return Err(TunError::new(
                ErrorCode::TunApplyFailed,
                format!(
                    "sing-box binary not found at {} (dev elevated runner)",
                    self.binary.display()
                ),
            ));
        }
        if !config_path.is_file() {
            return Err(TunError::new(
                ErrorCode::TunApplyFailed,
                format!("config not found at {}", config_path.display()),
            ));
        }
        if let Some(parent) = self.log_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                TunError::new(
                    ErrorCode::TunApplyFailed,
                    format!("create log dir {}: {err}", parent.display()),
                )
            })?;
        }
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .map_err(|err| {
                TunError::new(
                    ErrorCode::TunApplyFailed,
                    format!("open core log {}: {err}", self.log_path.display()),
                )
            })?;
        let log_err = log.try_clone().map_err(|err| {
            TunError::new(
                ErrorCode::TunApplyFailed,
                format!("clone core log handle: {err}"),
            )
        })?;
        Command::new(&self.binary)
            .arg("run")
            .arg("-c")
            .arg(config_path)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .spawn()
            .map_err(|err| {
                TunError::new(
                    ErrorCode::TunApplyFailed,
                    format!(
                        "spawn {} run -c {}: {err}",
                        self.binary.display(),
                        config_path.display()
                    ),
                )
            })
    }
}

#[cfg(target_os = "windows")]
impl CoreCoordinator for WindowsElevatedCoreCoordinator {
    fn start_with_config(&mut self, config_path: &Path) -> Result<u32, TunError> {
        self.check_elevation()?;
        let child =
            match verify_then_start_child(|| self.spawn_elevated(config_path), &self.log_path) {
                Ok(child) => child,
                Err(err) => {
                    self.child = None;
                    return Err(err);
                }
            };
        let pid = child.id();
        self.child = Some(child);
        tracing::info!(pid, "elevated sing-box started via the dev Windows runner");
        Ok(pid)
    }

    fn stop(&mut self) -> Result<(), TunError> {
        let Some(pid) = self.child.as_ref().map(|child| child.id()) else {
            return Ok(());
        };
        // Graceful-first termination (`docs/tun.md`): the
        // strict-route WFP filters sing-box installs are removed on its
        // graceful shutdown path only. A hard `/F` kill strands them, which
        // black-holes every non-loopback TCP connection on the host (V11
        // observation: curl 000 / ping OK / stale filters). Send the close
        // request first (taskkill without `/F` delivers WM_CLOSE, which the
        // core treats as a signal and uses to clean up its filters and
        // routes), then fall back to the forced kill after `TERM_GRACE`.
        let close = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match close {
            Ok(status) if status.success() => {}
            Ok(_) => {
                // A graceful close request can legitimately fail for a
                // console-only process; the forced fallback below decides.
            }
            Err(_) => {}
        }

        // Bounded wait for the process tree to die on its own (graceful path).
        let deadline = Instant::now() + TERM_GRACE;
        while Instant::now() < deadline {
            match self.child.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(_)) => {
                        self.child = None;
                        return Ok(());
                    }
                    Ok(None) => {}
                    Err(_) => {
                        self.child = None;
                        return Ok(());
                    }
                },
                None => return Ok(()),
            }
            std::thread::sleep(LIVENESS_POLL);
        }

        // Graceful close did not finish the tree; force-kill (WFP filters may
        // strand — the journal + recovery handle residue, and the next apply
        // recreates the filters).
        let kill = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match kill {
            Ok(status) if status.success() => {}
            Ok(_) => {
                // taskkill fails harmlessly when the process already exited;
                // the handle decides.
                let still_alive = self
                    .child
                    .as_mut()
                    .is_some_and(|child| matches!(child.try_wait(), Ok(None)));
                if still_alive {
                    return Err(TunError::new(
                        ErrorCode::TunRestoreFailed,
                        format!("taskkill /PID {pid} /T /F failed"),
                    ));
                }
            }
            Err(err) => {
                return Err(TunError::new(
                    ErrorCode::TunRestoreFailed,
                    format!("taskkill /PID {pid}: {err}"),
                ));
            }
        }
        // Bounded wait for the process tree to die after the forced kill.
        let deadline = Instant::now() + TERM_GRACE;
        while Instant::now() < deadline {
            match self.child.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(_)) => {
                        self.child = None;
                        return Ok(());
                    }
                    Ok(None) => {}
                    Err(_) => {
                        self.child = None;
                        return Ok(());
                    }
                },
                None => return Ok(()),
            }
            std::thread::sleep(LIVENESS_POLL);
        }
        Err(TunError::new(
            ErrorCode::TunRecoveryRequired,
            format!("elevated sing-box (pid {pid}) survived taskkill /T /F"),
        ))
    }

    fn set_dns(&mut self, _service: &str, _servers: &[String]) -> Result<(), TunError> {
        Err(TunError::new(
            ErrorCode::TunApplyFailed,
            "system DNS mutation is not supported on the Windows dev runner",
        ))
    }
}

/// Whether the current process carries an elevated (Administrator) token.
#[cfg(target_os = "windows")]
pub fn process_is_elevated() -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation: TOKEN_ELEVATION = std::mem::zeroed();
        let mut size = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        );
        let _ = CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

/// Interactive user SID (`S-1-5-21-…`), including when the process token is
/// the UAC-filtered (medium IL) token of an Administrator.
#[cfg(target_os = "windows")]
pub fn current_user_sid_string() -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut needed = 0u32;
        let _ = GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        if needed == 0 {
            let _ = CloseHandle(token);
            return None;
        }
        let mut buf = vec![0u8; needed as usize];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr() as *mut _,
            needed,
            &mut needed,
        );
        let _ = CloseHandle(token);
        if ok == 0 {
            return None;
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid_str = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid_str) == 0 || sid_str.is_null() {
            return None;
        }
        let mut len = 0usize;
        while *sid_str.add(len) != 0 {
            len += 1;
        }
        let value = String::from_utf16_lossy(std::slice::from_raw_parts(sid_str, len));
        let _ = LocalFree(sid_str as _);
        Some(value)
    }
}

/// Whether the interactive user is a member of Administrators, including the
/// UAC-filtered token where the group is `SE_GROUP_USE_FOR_DENY_ONLY`.
/// Standard users are false — over-the-shoulder UAC cannot make TUN work.
#[cfg(target_os = "windows")]
pub fn current_user_is_local_admin() -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Security::{
        CreateWellKnownSid, EqualSid, GetTokenInformation, TokenGroups,
        WinBuiltinAdministratorsSid, SECURITY_MAX_SID_SIZE, SID_AND_ATTRIBUTES, TOKEN_GROUPS,
        TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut needed = 0u32;
        let _ = GetTokenInformation(token, TokenGroups, std::ptr::null_mut(), 0, &mut needed);
        if needed == 0 {
            let _ = CloseHandle(token);
            return false;
        }
        let mut buf = vec![0u8; needed as usize];
        let ok = GetTokenInformation(
            token,
            TokenGroups,
            buf.as_mut_ptr() as *mut _,
            needed,
            &mut needed,
        );
        let _ = CloseHandle(token);
        if ok == 0 {
            return false;
        }
        let mut admin_sid = [0u8; SECURITY_MAX_SID_SIZE as usize];
        let mut admin_len = SECURITY_MAX_SID_SIZE;
        if CreateWellKnownSid(
            WinBuiltinAdministratorsSid,
            std::ptr::null_mut(),
            admin_sid.as_mut_ptr() as *mut _,
            &mut admin_len,
        ) == 0
        {
            return false;
        }
        let groups = &*(buf.as_ptr() as *const TOKEN_GROUPS);
        let first = std::ptr::addr_of!(groups.Groups) as *const SID_AND_ATTRIBUTES;
        for i in 0..groups.GroupCount as usize {
            let saa = &*first.add(i);
            if EqualSid(saa.Sid, admin_sid.as_ptr() as *mut _) != 0 {
                return true;
            }
        }
        false
    }
}

/// Signal the elevated launcher to stop the core (named event with a DACL).
#[cfg(target_os = "windows")]
pub fn signal_tun_stop_event() -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenEventW, SetEvent, EVENT_MODIFY_STATE};

    fn wide_z(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
        value.as_ref().encode_wide().chain(Some(0)).collect()
    }

    let name = wide_z(ice_tun_pin::TUN_STOP_EVENT_NAME);
    let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let ok = unsafe { SetEvent(handle) };
    unsafe {
        let _ = CloseHandle(handle);
    }
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Fixed name of the scheduled task that runs the TUN core elevated (plan B:
/// scheduled-task elevation). The task is created once with the
/// highest-privilege flag (the creating moment is the only elevation the
/// user ever sees); afterwards `schtasks /Run` / `/End` start and stop the
/// elevated core without any UAC prompt.
pub const TUN_TASK_NAME: &str = ice_tun_pin::TUN_TASK_NAME;

/// Run `schtasks` with `CREATE_NO_WINDOW` (the calls come from the GUI app /
/// the status poll; without it every invocation flashes a console window).
#[cfg(target_os = "windows")]
fn run_schtasks(args: &[&str]) -> std::io::Result<std::process::ExitStatus> {
    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;
    Command::new("schtasks")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .status()
}

/// Whether the TUN scheduled task exists (exit-code probe; the zh-CN
/// `schtasks /Query` output is never parsed). Always `false` on non-Windows
/// hosts (no task concept there).
pub fn tun_task_exists() -> bool {
    #[cfg(target_os = "windows")]
    {
        let status = run_schtasks(&["/Query", "/TN", TUN_TASK_NAME]);
        matches!(status, Ok(status) if status.success())
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Whether the TUN task XML carries a parseable binary pin. Cheaper than
/// [`tun_task_pin_matches`] (no file hashes); used by the status poll so a
/// pin-less leftover task is not reported as ready. Always `false` on
/// non-Windows hosts.
pub fn tun_task_has_pin() -> bool {
    #[cfg(target_os = "windows")]
    {
        query_tun_task_xml()
            .as_deref()
            .and_then(ice_tun_pin::extract_tun_task_pin_from_xml)
            .is_some()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Write the Unicode XML that [`tun_task_xml_create_args`] imports. `schtasks
/// /Create /XML` requires UTF-16 LE with a BOM.
pub fn write_tun_task_xml(
    xml_path: &Path,
    launcher: &Path,
    data_dir: &Path,
    pin: &str,
    user_sid: &str,
) -> std::io::Result<()> {
    let xml = ice_tun_pin::render_tun_task_xml(launcher, data_dir, pin, user_sid);
    std::fs::write(xml_path, ice_tun_pin::encode_utf16_le_bom(&xml))
}

/// The argv of the `schtasks /Create /XML` invocation that installs the TUN
/// task. Privilege, the on-demand action, and the SHA-256 pin live in the
/// XML — `schtasks /D` is a day-of-week flag and cannot store a description.
/// Must run from an elevated context exactly once; the runtime flow does
/// that through a single UAC prompt.
pub fn tun_task_xml_create_args(xml_path: &Path) -> Vec<String> {
    vec![
        "/Create".to_string(),
        "/TN".to_string(),
        TUN_TASK_NAME.to_string(),
        "/XML".to_string(),
        xml_path.display().to_string(),
        "/F".to_string(),
    ]
}

/// Quote argv for a Windows command line (ShellExecute `lpParameters` or
/// `cmd`). Arguments with spaces or quotes are wrapped; embedded quotes
/// become `\"`.
pub fn quote_windows_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.chars().any(|ch| ch == ' ' || ch == '"') {
                format!("\"{}\"", arg.replace('"', "\\\""))
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Rebuild an argv list into a `cmd`-friendly `schtasks …` command line.
pub fn schtasks_command_line(args: &[String]) -> String {
    format!("schtasks {}", quote_windows_args(args))
}

/// Elevate `exe` with the `runas` verb and wait for it to exit.
///
/// `exe` must be a GUI-subsystem binary (this project's
/// `ice-tun-launcher.exe`). Elevating `cmd.exe` / `schtasks.exe` (console
/// subsystem) flashes a black window because ShellExecute cannot pass
/// `CREATE_NO_WINDOW`; the GUI launcher then runs `schtasks` with that flag.
/// Returns the child exit code. UAC cancel is `io::Error` with raw OS error
/// 1223 (`ERROR_CANCELLED`).
#[cfg(target_os = "windows")]
pub fn run_elevated_wait(exe: &Path, args: &[String]) -> std::io::Result<u32> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    fn wide_z(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
        value.as_ref().encode_wide().chain(Some(0)).collect()
    }

    let file = wide_z(exe);
    let verb = wide_z("runas");
    let params = quote_windows_args(args);
    let params_w = wide_z(&params);

    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = if params.is_empty() {
        std::ptr::null()
    } else {
        params_w.as_ptr()
    };
    info.nShow = SW_HIDE;

    // ShellExecuteEx can load COM shell extensions; MSDN requires CoInitialize.
    let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
    let uninit = hr == 0 || hr == 1; // S_OK or S_FALSE
    let result = (|| {
        let ok = unsafe { ShellExecuteExW(&mut info) };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            return Err(std::io::Error::from_raw_os_error(err as i32));
        }
        if info.hProcess.is_null() {
            return Err(std::io::Error::other(
                "elevated process handle was not returned",
            ));
        }
        let wait = unsafe { WaitForSingleObject(info.hProcess, INFINITE) };
        if wait != WAIT_OBJECT_0 {
            unsafe {
                let _ = CloseHandle(info.hProcess);
            }
            return Err(std::io::Error::other(
                "waiting for the elevated process failed",
            ));
        }
        let mut code = 0u32;
        let got = unsafe { GetExitCodeProcess(info.hProcess, &mut code) };
        unsafe {
            let _ = CloseHandle(info.hProcess);
        }
        if got == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(code)
    })();
    if uninit {
        unsafe { CoUninitialize() };
    }
    result
}

/// Whether the TUN scheduled task exists and its description pin matches the
/// on-disk launcher and sibling `sing-box.exe`. Always `false` on
/// non-Windows hosts.
pub fn tun_task_pin_matches(launcher: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        verify_task_for_ensure(launcher).is_ok()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = launcher;
        false
    }
}

/// Read the scheduled-task XML and refuse a replaced protected launcher or core.
/// Start hashes only the Program Files copies (what `schtasks /Run` executes).
#[cfg(target_os = "windows")]
fn verify_task_binaries(_launcher: &Path) -> Result<(), TunError> {
    let (pin, protected, protected_core) = load_verified_task_pin()?;
    pin_must_match(&pin, &protected, &protected_core, "protected")
}

/// Ensure hashes the per-user copies (detect app updates) and the protected
/// copies (detect tampering). Missing either side triggers re-elevation.
#[cfg(target_os = "windows")]
fn verify_task_for_ensure(user_launcher: &Path) -> Result<(), TunError> {
    let (pin, protected, protected_core) = load_verified_task_pin()?;
    pin_must_match(&pin, &protected, &protected_core, "protected")?;
    if ice_tun_pin::path_is_protected_launcher(user_launcher, &ice_tun_pin::program_files_dir()) {
        return Ok(());
    }
    let core = ice_tun_pin::core_beside_launcher(user_launcher).ok_or_else(|| {
        TunError::new(
            ErrorCode::TunApplyFailed,
            format!(
                "TUN task launcher path {} has no parent",
                user_launcher.display()
            ),
        )
    })?;
    pin_must_match(&pin, user_launcher, &core, "bundled")
}

#[cfg(target_os = "windows")]
fn load_verified_task_pin() -> Result<(ice_tun_pin::TunTaskPin, PathBuf, PathBuf), TunError> {
    let xml = query_tun_task_xml().ok_or_else(|| {
        TunError::new(
            ErrorCode::TunPermissionRequired,
            format!("the TUN scheduled task {TUN_TASK_NAME} XML could not be read"),
        )
    })?;
    let pin = ice_tun_pin::extract_tun_task_pin_from_xml(&xml).ok_or_else(|| {
        TunError::new(
            ErrorCode::TunPermissionRequired,
            format!(
                "the TUN scheduled task {TUN_TASK_NAME} is missing the binary pin; run the one-time elevation setup (ensure_tun_elevation) before enabling capture"
            ),
        )
    })?;
    let program_files = ice_tun_pin::program_files_dir();
    let protected = ice_tun_pin::protected_launcher_path(&program_files);
    ice_tun_pin::verify_task_command(&xml, &protected)
        .map_err(|msg| TunError::new(ErrorCode::TunPermissionRequired, msg))?;
    let sid = current_user_sid_string().ok_or_else(|| {
        TunError::new(
            ErrorCode::TunPermissionRequired,
            "cannot resolve the interactive user SID",
        )
    })?;
    ice_tun_pin::verify_task_user_id(&xml, &sid)
        .map_err(|msg| TunError::new(ErrorCode::TunPermissionRequired, msg))?;
    let protected_core = ice_tun_pin::core_beside_launcher(&protected).ok_or_else(|| {
        TunError::new(
            ErrorCode::TunApplyFailed,
            format!(
                "TUN protected launcher path {} has no parent",
                protected.display()
            ),
        )
    })?;
    Ok((pin, protected, protected_core))
}

#[cfg(target_os = "windows")]
fn pin_must_match(
    pin: &ice_tun_pin::TunTaskPin,
    launcher: &Path,
    core: &Path,
    label: &str,
) -> Result<(), TunError> {
    match ice_tun_pin::pin_matches_files(pin, launcher, core) {
        Ok(true) => Ok(()),
        Ok(false) => Err(TunError::new(
            ErrorCode::TunApplyFailed,
            format!(
                "{label} TUN launcher or {} does not match the scheduled-task sha256 pin; refusing to start",
                core.display()
            ),
        )),
        Err(err) => Err(TunError::new(ErrorCode::TunApplyFailed, err)),
    }
}

#[cfg(target_os = "windows")]
fn query_tun_task_xml() -> Option<String> {
    use std::os::windows::process::CommandExt;
    let output = Command::new("schtasks")
        .args(["/Query", "/TN", TUN_TASK_NAME, "/XML"])
        .stdin(Stdio::null())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(ice_tun_pin::decode_schtasks_output(&output.stdout))
}

/// Elevated runner for the Windows TUN path through the scheduled task
/// (plan B). The app process never needs to be elevated: the task (created
/// once) carries the highest-privilege token, and `schtasks /Run` / `/End`
/// trigger and terminate it without UAC. The task action is the protected
/// `ice-tun-launcher`, which spawns sing-box, writes its pid to the
/// handshake pid file, and honors a graceful-stop request via a named event.
#[cfg(target_os = "windows")]
pub struct TaskCoreCoordinator {
    launcher: PathBuf,
    pidfile: PathBuf,
    pid: Option<u32>,
}

#[cfg(target_os = "windows")]
impl TaskCoreCoordinator {
    pub fn new(launcher: PathBuf, pidfile: PathBuf) -> Self {
        Self {
            launcher,
            pidfile,
            pid: None,
        }
    }

    fn run_task(&self) -> Result<(), TunError> {
        let status = run_schtasks(&["/Run", "/TN", TUN_TASK_NAME]).map_err(|err| {
            TunError::new(ErrorCode::TunApplyFailed, format!("schtasks /Run: {err}"))
        })?;
        if status.success() {
            Ok(())
        } else {
            Err(TunError::new(
                ErrorCode::TunApplyFailed,
                format!(
                    "schtasks /Run failed (exit {}): the TUN task may be missing or disabled",
                    status.code().unwrap_or(-1)
                ),
            ))
        }
    }

    fn end_task(&self) {
        let _ = run_schtasks(&["/End", "/TN", TUN_TASK_NAME]);
    }

    fn reset_handshake(&mut self) -> Result<(), TunError> {
        self.wait_until_previous_instance_gone();
        if protected_launcher_is_running() {
            return Err(TunError::new(
                ErrorCode::TunApplyFailed,
                "previous TUN task instance is still running after schtasks /End; a new /Run would be ignored",
            ));
        }
        let _ = std::fs::remove_file(&self.pidfile);
        self.pid = None;
        Ok(())
    }

    fn wait_until_previous_instance_gone(&self) {
        if let Some(pid) = self.pid {
            let deadline = Instant::now() + TERM_GRACE;
            while Instant::now() < deadline && pid_is_alive_windows(pid) {
                std::thread::sleep(LIVENESS_POLL);
            }
        }
        if let Ok(contents) = std::fs::read_to_string(&self.pidfile) {
            if let Ok(pid) = contents.trim().parse::<u32>() {
                if pid != 0 {
                    let deadline = Instant::now() + TERM_GRACE;
                    while Instant::now() < deadline && pid_is_alive_windows(pid) {
                        std::thread::sleep(LIVENESS_POLL);
                    }
                }
            }
        }
        let deadline = Instant::now() + TERM_GRACE;
        while Instant::now() < deadline && protected_launcher_is_running() {
            std::thread::sleep(LIVENESS_POLL);
        }
    }

    fn wait_for_pidfile(&self) -> Result<u32, TunError> {
        // The task start + launcher spawn take a moment; the launcher writes
        // the pid file within seconds of `schtasks /Run`.
        let deadline = Instant::now() + STARTUP_LIVENESS_WAIT * 10;
        loop {
            if let Ok(contents) = std::fs::read_to_string(&self.pidfile) {
                if let Ok(pid) = contents.trim().parse::<u32>() {
                    if pid != 0 && pid_is_alive_windows(pid) {
                        return Ok(pid);
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(TunError::new(
                    ErrorCode::TunHealthcheckFailed,
                    format!(
                        "TUN task started but no live core pid appeared in {}",
                        self.pidfile.display()
                    ),
                ));
            }
            std::thread::sleep(LIVENESS_POLL);
        }
    }
}

/// Pure liveness probe for a pid from an unelevated process: the elevated
/// core is queryable via `PROCESS_QUERY_LIMITED_INFORMATION` across
/// integrity levels (`GetExitCodeProcess`), but not signalable.
#[cfg(target_os = "windows")]
fn pid_is_alive_windows(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(err) if err == ERROR_ACCESS_DENIED as i32 => true,
            Some(err) if err == ERROR_INVALID_PARAMETER as i32 => false,
            _ => false,
        };
    }
    let mut exit_code: u32 = 0;
    let queried = unsafe { GetExitCodeProcess(handle, &mut exit_code) };
    unsafe { CloseHandle(handle) };
    queried != 0 && exit_code == STILL_ACTIVE as u32
}

/// Whether a process whose image is the protected Program Files launcher is
/// still running. Used after `schtasks /End` (asynchronous) so `/Run` is not
/// silently ignored under `MultipleInstancesPolicy=IgnoreNew`.
#[cfg(target_os = "windows")]
fn protected_launcher_is_running() -> bool {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let expected = ice_tun_pin::protected_launcher_path(&ice_tun_pin::program_files_dir());
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot.is_null() || snapshot == INVALID_HANDLE_VALUE {
        return false;
    }
    let mut entry = unsafe { std::mem::zeroed::<PROCESSENTRY32W>() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut running = false;
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while more {
        let handle =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID) };
        if !handle.is_null() {
            let mut buf = [0u16; 32768];
            let mut size = buf.len() as u32;
            if unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size) } != 0
                && size > 0
            {
                let path = std::ffi::OsString::from_wide(&buf[..size as usize]);
                if ice_tun_pin::command_matches_launcher(&path.to_string_lossy(), &expected) {
                    running = true;
                }
            }
            unsafe {
                let _ = CloseHandle(handle);
            }
        }
        if running {
            break;
        }
        more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe {
        let _ = CloseHandle(snapshot);
    }
    running
}

#[cfg(target_os = "windows")]
impl CoreCoordinator for TaskCoreCoordinator {
    // The task action (baked at creation) already carries the config path;
    // the runtime path is validated for equality so a moved data dir cannot
    // silently run a stale config.
    fn start_with_config(&mut self, config_path: &Path) -> Result<u32, TunError> {
        if !tun_task_exists() {
            return Err(TunError::new(
                ErrorCode::TunPermissionRequired,
                format!(
                    "the TUN scheduled task {TUN_TASK_NAME} is missing; run the one-time elevation setup (ensure_tun_elevation) before enabling capture"
                ),
            ));
        }
        if !self.launcher.is_file() {
            return Err(TunError::new(
                ErrorCode::TunApplyFailed,
                format!("TUN task launcher not found at {}", self.launcher.display()),
            ));
        }
        verify_task_binaries(&self.launcher)?;
        let xml = query_tun_task_xml().ok_or_else(|| {
            TunError::new(
                ErrorCode::TunApplyFailed,
                format!("the TUN scheduled task {TUN_TASK_NAME} XML could not be read"),
            )
        })?;
        ice_tun_pin::task_config_path_matches(&xml, config_path)
            .map_err(|msg| TunError::new(ErrorCode::TunApplyFailed, msg))?;
        self.end_task();
        self.reset_handshake()?;
        self.run_task()?;
        let pid = match self.wait_for_pidfile() {
            Ok(pid) => pid,
            Err(err) => {
                self.end_task();
                let _ = std::fs::remove_file(&self.pidfile);
                return Err(err);
            }
        };
        wait_for_pid_liveness(pid, &self.pidfile, pid_is_alive_windows)?;
        self.pid = Some(pid);
        tracing::info!(pid, "elevated sing-box started via the TUN scheduled task");
        Ok(pid)
    }

    fn stop(&mut self) -> Result<(), TunError> {
        let Some(pid) = self.pid else {
            self.end_task();
            return Ok(());
        };
        // Graceful-first (`docs/tun.md`): the strict-route
        // WFP filters sing-box installs are removed on its graceful shutdown
        // path only; a hard kill strands them and black-holes host TCP (V11).
        // The elevated launcher honors the named stop event with the same
        // graceful-then-forced sequence the dev runner uses.
        if let Err(err) = signal_tun_stop_event() {
            return Err(TunError::new(
                ErrorCode::TunRestoreFailed,
                format!("request TUN core stop: {err}"),
            ));
        }
        let deadline = Instant::now() + TERM_GRACE;
        while Instant::now() < deadline && pid_is_alive_windows(pid) {
            std::thread::sleep(LIVENESS_POLL);
        }
        if !pid_is_alive_windows(pid) {
            self.pid = None;
            return Ok(());
        }
        // Graceful stop did not finish the tree (launcher gone); end the task
        // hard (WFP residue is handled by the journal + recovery, and the
        // next apply recreates the filters).
        self.end_task();
        let deadline = Instant::now() + TERM_GRACE;
        while Instant::now() < deadline && pid_is_alive_windows(pid) {
            std::thread::sleep(LIVENESS_POLL);
        }
        if pid_is_alive_windows(pid) {
            return Err(TunError::new(
                ErrorCode::TunRecoveryRequired,
                format!("elevated sing-box (pid {pid}) survived the scheduled-task end"),
            ));
        }
        self.pid = None;
        Ok(())
    }

    fn set_dns(&mut self, _service: &str, _servers: &[String]) -> Result<(), TunError> {
        Err(TunError::new(
            ErrorCode::TunApplyFailed,
            "system DNS mutation is not supported on the Windows TUN task runner",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deferred_coordinator_fails_cleanly_without_mutation() {
        let mut coordinator = DeferredCoreCoordinator;
        let err = coordinator
            .start_with_config(Path::new("/nonexistent/config.json"))
            .expect_err("deferred runner must fail");
        assert_eq!(err.code, ErrorCode::TunPermissionRequired);
        assert!(coordinator.stop().is_ok(), "stop is idempotent");
    }

    #[test]
    fn wait_for_child_liveness_reports_early_exit() {
        let mut child = if cfg!(windows) {
            Command::new("cmd")
                .args(["/C", "exit", "1"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn exiting process")
        } else {
            Command::new("true")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn exiting process")
        };
        let err = wait_for_child_liveness(&mut child, Path::new("sing-box.log"))
            .expect_err("exited child must fail liveness");
        assert_eq!(err.code, ErrorCode::TunHealthcheckFailed);
    }

    #[test]
    fn wait_for_pid_liveness_reports_dead_pid() {
        let err = wait_for_pid_liveness(1, Path::new("pidfile"), |_| false)
            .expect_err("dead pid must fail liveness");
        assert_eq!(err.code, ErrorCode::TunHealthcheckFailed);
    }

    #[test]
    fn sudo_coordinator_stop_is_idempotent_before_start() {
        let mut coordinator = SudoCoreCoordinator::new(
            PathBuf::from("/nonexistent/sing-box"),
            PathBuf::from("/nonexistent/log"),
        );
        assert!(coordinator.stop().is_ok());
        assert_eq!(coordinator.pid, None);
    }

    #[cfg(unix)]
    #[test]
    fn pid_liveness_probe_distinguishes_alive_and_gone() {
        assert!(pid_is_alive(0), "pid 0 (self/kernel) is alive");
        // A huge positive pid that no kernel can have assigned. NOTE: must
        // not be u32::MAX — as i32 that is -1, the "all processes" pid.
        assert!(
            !pid_is_alive(i32::MAX as u32 - 1),
            "an impossible pid must report gone (ESRCH)"
        );
    }

    #[test]
    fn tun_task_xml_create_args_import_the_unicode_task_file() {
        let xml_path =
            Path::new(r"C:\Users\admin\AppData\Roaming\com.yilong-musk.icebox\ice-box-tun.xml");
        let args = tun_task_xml_create_args(xml_path);
        assert_eq!(
            args,
            [
                "/Create",
                "/TN",
                TUN_TASK_NAME,
                "/XML",
                r"C:\Users\admin\AppData\Roaming\com.yilong-musk.icebox\ice-box-tun.xml",
                "/F",
            ]
        );
        assert_eq!(TUN_TASK_NAME, ice_tun_pin::TUN_TASK_NAME);
    }

    #[test]
    fn write_tun_task_xml_is_utf16_le_with_a_persisted_pin() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-tun-xml-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let xml_path = dir.join("ice-box-tun.xml");
        let launcher = Path::new(r"C:\Program Files\ice-box\ice-tun-launcher.exe");
        let pin = ice_tun_pin::format_tun_task_pin(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        );
        write_tun_task_xml(&xml_path, launcher, &dir, &pin, "S-1-5-21-1-2-3-1001").expect("write");
        let bytes = std::fs::read(&xml_path).expect("read");
        let xml = ice_tun_pin::decode_schtasks_output(&bytes);
        assert_eq!(
            ice_tun_pin::extract_tun_task_pin_from_xml(&xml)
                .expect("pin")
                .launcher_sha256,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert!(ice_tun_pin::command_matches_launcher(
            &ice_tun_pin::extract_tun_task_command_from_xml(&xml).expect("command"),
            launcher
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn task_xml_command_mismatch_is_rejected_even_when_pin_matches() {
        let launcher = Path::new(r"C:\Program Files\ice-box\ice-tun-launcher.exe");
        let data_dir = Path::new(r"C:\Users\admin\AppData\Roaming\com.yilong-musk.icebox");
        let pin = ice_tun_pin::format_tun_task_pin(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        );
        let xml = ice_tun_pin::render_tun_task_xml(launcher, data_dir, &pin, "S-1-5-21-1-2-3-1001");
        assert!(ice_tun_pin::extract_tun_task_pin_from_xml(&xml).is_some());
        ice_tun_pin::verify_task_command(&xml, launcher).expect("protected command");
        ice_tun_pin::verify_task_user_id(&xml, "S-1-5-21-1-2-3-1001").expect("user id");
        let err = ice_tun_pin::verify_task_command(
            &xml,
            Path::new(r"C:\Users\admin\ice-tun-launcher.exe"),
        )
        .expect_err("user-dir command");
        assert!(err.to_lowercase().contains("command"), "{err}");
        ice_tun_pin::task_config_path_matches(&xml, &data_dir.join("config.json"))
            .expect("matching data dir");
        let mismatch =
            ice_tun_pin::task_config_path_matches(&xml, Path::new(r"D:\elsewhere\config.json"))
                .expect_err("different data dir");
        assert!(mismatch.contains("different config path"), "{mismatch}");
    }

    #[test]
    fn quote_windows_args_quotes_spaces_but_not_apostrophes() {
        let args = [
            "--install".to_string(),
            "--user-sid".to_string(),
            r#"C:\Users\O'Brien\ice-box-tun.xml"#.to_string(),
        ];
        let line = quote_windows_args(&args);
        assert!(line.contains(r"C:\Users\O'Brien\ice-box-tun.xml"));
        assert!(
            !line.contains(r#""C:\Users\O'Brien\ice-box-tun.xml""#),
            "apostrophe-only paths do not need quotes"
        );

        let spaced = quote_windows_args(&[
            "--xml".to_string(),
            r"C:\Program Files\ice-box\ice-box-tun.xml".to_string(),
        ]);
        assert!(spaced.contains(r#""C:\Program Files\ice-box\ice-box-tun.xml""#));
    }

    #[test]
    fn schtasks_command_line_quotes_spaces_but_not_apostrophes() {
        let args = [
            "/Create".to_string(),
            "/XML".to_string(),
            r#"C:\Users\O'Brien\ice-box-tun.xml"#.to_string(),
        ];
        let command = schtasks_command_line(&args);
        assert!(command.contains(r"C:\Users\O'Brien\ice-box-tun.xml"));
        assert!(
            !command.contains(r#""C:\Users\O'Brien\ice-box-tun.xml""#),
            "apostrophe-only paths do not need cmd quotes"
        );

        let spaced = schtasks_command_line(&[
            "/XML".to_string(),
            r"C:\Program Files\ice-box\ice-box-tun.xml".to_string(),
        ]);
        assert!(spaced.contains(r#""C:\Program Files\ice-box\ice-box-tun.xml""#));
    }
}
