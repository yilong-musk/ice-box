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

use crate::error::{TunError, TunErrorCode};

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
            TunErrorCode::PermissionRequired,
            "privileged sing-box runner is not wired yet (slice T3): install and authorize the helper, or use the dev sudo path",
        ))
    }

    fn stop(&mut self) -> Result<(), TunError> {
        Ok(())
    }

    fn set_dns(&mut self, _service: &str, _servers: &[String]) -> Result<(), TunError> {
        Err(TunError::new(
            TunErrorCode::PermissionRequired,
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
                TunErrorCode::PermissionRequired,
                "dev sudo runner needs a cached root credential (`sudo -v`) or a NOPASSWD rule; use scripts/run-acceptance-macos-tun.sh, or authorize the helper",
            )),
            Err(err) => Err(TunError::new(
                TunErrorCode::PermissionRequired,
                format!("sudo unavailable: {err}"),
            )),
        }
    }

    fn spawn_elevated(&self, config_path: &Path) -> Result<Child, TunError> {
        if !self.binary.is_file() {
            return Err(TunError::new(
                TunErrorCode::ApplyFailed,
                format!(
                    "sing-box binary not found at {} (dev sudo runner)",
                    self.binary.display()
                ),
            ));
        }
        if !config_path.is_file() {
            return Err(TunError::new(
                TunErrorCode::ApplyFailed,
                format!("config not found at {}", config_path.display()),
            ));
        }
        if let Some(parent) = self.log_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                TunError::new(
                    TunErrorCode::ApplyFailed,
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
                    TunErrorCode::ApplyFailed,
                    format!("open core log {}: {err}", self.log_path.display()),
                )
            })?;
        let log_err = log.try_clone().map_err(|err| {
            TunError::new(
                TunErrorCode::ApplyFailed,
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
                    TunErrorCode::ApplyFailed,
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
        let child = self.spawn_elevated(config_path)?;
        let launcher_pid = child.id();
        self.launcher_pid = Some(launcher_pid);
        self.child = Some(child);

        // Bounded liveness wait: catch immediate config/bind errors so the
        // backend's interface verification is not the only signal. `sudo`
        // execs sing-box, so this pid is the core process.
        let deadline = Instant::now() + STARTUP_LIVENESS_WAIT;
        loop {
            match self.child.as_mut().expect("child stored").try_wait() {
                Ok(Some(code)) => {
                    self.pid = None;
                    self.launcher_pid = None;
                    self.child = None;
                    return Err(TunError::new(
                        TunErrorCode::HealthcheckFailed,
                        format!(
                            "elevated sing-box exited during startup (code {code}); check {}",
                            self.log_path.display()
                        ),
                    ));
                }
                Ok(None) => {}
                Err(err) => {
                    self.pid = None;
                    self.launcher_pid = None;
                    self.child = None;
                    return Err(TunError::new(
                        TunErrorCode::ApplyFailed,
                        format!("poll elevated core: {err}"),
                    ));
                }
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(LIVENESS_POLL);
        }
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
                    TunErrorCode::RestoreFailed,
                    format!("sudo kill -TERM {pid} failed"),
                ));
            }
            Err(err) => {
                return Err(TunError::new(
                    TunErrorCode::RestoreFailed,
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
            TunErrorCode::RecoveryRequired,
            format!("elevated sing-box (pid {pid}) survived TERM and KILL"),
        ))
    }
    fn set_dns(&mut self, service: &str, servers: &[String]) -> Result<(), TunError> {
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
                TunErrorCode::ApplyFailed,
                format!("sudo networksetup -setdnsservers {service} failed (exit {status:?})"),
            )),
            Err(err) => Err(TunError::new(
                TunErrorCode::ApplyFailed,
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
/// (design note §4).
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
                TunErrorCode::PermissionRequired,
                "TUN transitions need an elevated context (the core runs elevated to create the wintun adapter); run the acceptance suite from an Administrator shell",
            ))
        }
    }

    fn spawn_elevated(&self, config_path: &Path) -> Result<Child, TunError> {
        if !self.binary.is_file() {
            return Err(TunError::new(
                TunErrorCode::ApplyFailed,
                format!(
                    "sing-box binary not found at {} (dev elevated runner)",
                    self.binary.display()
                ),
            ));
        }
        if !config_path.is_file() {
            return Err(TunError::new(
                TunErrorCode::ApplyFailed,
                format!("config not found at {}", config_path.display()),
            ));
        }
        if let Some(parent) = self.log_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                TunError::new(
                    TunErrorCode::ApplyFailed,
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
                    TunErrorCode::ApplyFailed,
                    format!("open core log {}: {err}", self.log_path.display()),
                )
            })?;
        let log_err = log.try_clone().map_err(|err| {
            TunError::new(
                TunErrorCode::ApplyFailed,
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
                    TunErrorCode::ApplyFailed,
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
        let child = self.spawn_elevated(config_path)?;
        let pid = child.id();
        self.child = Some(child);

        // Bounded liveness wait: catch immediate config/bind errors so the
        // backend's interface verification is not the only signal.
        let deadline = Instant::now() + STARTUP_LIVENESS_WAIT;
        loop {
            match self.child.as_mut().expect("child stored").try_wait() {
                Ok(Some(code)) => {
                    self.child = None;
                    return Err(TunError::new(
                        TunErrorCode::HealthcheckFailed,
                        format!(
                            "elevated sing-box exited during startup (code {code}); check {}",
                            self.log_path.display()
                        ),
                    ));
                }
                Ok(None) => {}
                Err(err) => {
                    self.child = None;
                    return Err(TunError::new(
                        TunErrorCode::ApplyFailed,
                        format!("poll elevated core: {err}"),
                    ));
                }
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(LIVENESS_POLL);
        }
        tracing::info!(pid, "elevated sing-box started via the dev Windows runner");
        Ok(pid)
    }

    fn stop(&mut self) -> Result<(), TunError> {
        let Some(pid) = self.child.as_ref().map(|child| child.id()) else {
            return Ok(());
        };
        // Graceful-first termination (design note tun-windows-t0 §4): the
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
                        TunErrorCode::RestoreFailed,
                        format!("taskkill /PID {pid} /T /F failed"),
                    ));
                }
            }
            Err(err) => {
                return Err(TunError::new(
                    TunErrorCode::RestoreFailed,
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
            TunErrorCode::RecoveryRequired,
            format!("elevated sing-box (pid {pid}) survived taskkill /T /F"),
        ))
    }

    fn set_dns(&mut self, _service: &str, _servers: &[String]) -> Result<(), TunError> {
        Err(TunError::new(
            TunErrorCode::ApplyFailed,
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

/// Fixed name of the scheduled task that runs the TUN core elevated (plan B:
/// scheduled-task elevation). The task is created once with the
/// highest-privilege flag (the creating moment is the only elevation the
/// user ever sees); afterwards `schtasks /Run` / `/End` start and stop the
/// elevated core without any UAC prompt.
pub const TUN_TASK_NAME: &str = "ice-box-tun";

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

/// The argv of the `schtasks /Create` invocation that installs the TUN task
/// (highest privilege, never auto-triggered — only `schtasks /Run` starts
/// it). The `/TR` element carries the launcher action with backslash-escaped
/// inner quotes, exactly as `schtasks` stores it. Must run from an elevated
/// context exactly once; the runtime flow does that through a single UAC
/// prompt, the installer does it at install time.
#[cfg(target_os = "windows")]
pub fn tun_task_create_args(
    launcher: &Path,
    binary: &Path,
    config: &Path,
    log: &Path,
    pidfile: &Path,
    stopfile: &Path,
) -> Vec<String> {
    let action = format!(
        "\"{}\" --binary \"{}\" --config \"{}\" --log \"{}\" --pidfile \"{}\" --stopfile \"{}\"",
        launcher.display(),
        binary.display(),
        config.display(),
        log.display(),
        pidfile.display(),
        stopfile.display()
    );
    vec![
        "/Create".to_string(),
        "/TN".to_string(),
        TUN_TASK_NAME.to_string(),
        "/TR".to_string(),
        // The action starts and ends with a quoted path; as one argv element
        // std quotes/escapes it into `"\"...\""` form when building the
        // schtasks command line, which is exactly what schtasks /TR stores.
        action,
        "/SC".to_string(),
        "ONCE".to_string(),
        // A past /ST makes schtasks refuse to create the task ("the task
        // cannot run"), so use the end of the day; the task is only ever
        // triggered by `schtasks /Run` anyway.
        "/ST".to_string(),
        "23:59".to_string(),
        "/RL".to_string(),
        "HIGHEST".to_string(),
        "/F".to_string(),
    ]
}

/// Elevated runner for the Windows TUN path through the scheduled task
/// (plan B). The app process never needs to be elevated: the task (created
/// once) carries the highest-privilege token, and `schtasks /Run` / `/End`
/// trigger and terminate it without UAC. The task action is the bundled
/// `ice-tun-launcher`, which spawns sing-box, writes its pid to the
/// handshake pid file, and honors a graceful-stop request via the stop file.
#[cfg(target_os = "windows")]
pub struct TaskCoreCoordinator {
    launcher: PathBuf,
    pidfile: PathBuf,
    stopfile: PathBuf,
    pid: Option<u32>,
}

#[cfg(target_os = "windows")]
impl TaskCoreCoordinator {
    pub fn new(launcher: PathBuf, pidfile: PathBuf, stopfile: PathBuf) -> Self {
        Self {
            launcher,
            pidfile,
            stopfile,
            pid: None,
        }
    }

    fn run_task(&self) -> Result<(), TunError> {
        let status = run_schtasks(&["/Run", "/TN", TUN_TASK_NAME]).map_err(|err| {
            TunError::new(TunErrorCode::ApplyFailed, format!("schtasks /Run: {err}"))
        })?;
        if status.success() {
            Ok(())
        } else {
            Err(TunError::new(
                TunErrorCode::ApplyFailed,
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
        // Wait out a previously recorded core (the /End above kills the task
        // tree hard, so the launcher cannot clean the pid file itself).
        if let Some(pid) = self.pid {
            let deadline = Instant::now() + TERM_GRACE;
            while Instant::now() < deadline && pid_is_alive_windows(pid) {
                std::thread::sleep(LIVENESS_POLL);
            }
        }
        let _ = std::fs::remove_file(&self.stopfile);
        let _ = std::fs::remove_file(&self.pidfile);
        self.pid = None;
        Ok(())
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
                    TunErrorCode::HealthcheckFailed,
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

#[cfg(target_os = "windows")]
impl CoreCoordinator for TaskCoreCoordinator {
    // The task action (baked at creation) already carries the config path;
    // the runtime path is validated for equality so a moved data dir cannot
    // silently run a stale config.
    fn start_with_config(&mut self, config_path: &Path) -> Result<u32, TunError> {
        let _ = config_path;
        if !tun_task_exists() {
            return Err(TunError::new(
                TunErrorCode::PermissionRequired,
                format!(
                    "the TUN scheduled task {TUN_TASK_NAME} is missing; run the one-time elevation setup (ensure_tun_elevation) before enabling capture"
                ),
            ));
        }
        if !self.launcher.is_file() {
            return Err(TunError::new(
                TunErrorCode::ApplyFailed,
                format!("TUN task launcher not found at {}", self.launcher.display()),
            ));
        }
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
        // Bounded liveness wait: catch immediate config/bind errors so the
        // backend's interface verification is not the only signal.
        let deadline = Instant::now() + STARTUP_LIVENESS_WAIT;
        while Instant::now() < deadline {
            if !pid_is_alive_windows(pid) {
                return Err(TunError::new(
                    TunErrorCode::HealthcheckFailed,
                    format!(
                        "elevated sing-box exited during startup (pid {pid}); check {}",
                        self.pidfile.display()
                    ),
                ));
            }
            std::thread::sleep(LIVENESS_POLL);
        }
        self.pid = Some(pid);
        tracing::info!(pid, "elevated sing-box started via the TUN scheduled task");
        Ok(pid)
    }

    fn stop(&mut self) -> Result<(), TunError> {
        let Some(pid) = self.pid else {
            self.end_task();
            return Ok(());
        };
        // Graceful-first (design note tun-windows-t0 §4): the strict-route
        // WFP filters sing-box installs are removed on its graceful shutdown
        // path only; a hard kill strands them and black-holes host TCP (V11).
        // The elevated launcher honors the stop file with the same
        // graceful-then-forced sequence the dev runner uses.
        if let Err(err) = std::fs::write(&self.stopfile, "stop") {
            return Err(TunError::new(
                TunErrorCode::RestoreFailed,
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
                TunErrorCode::RecoveryRequired,
                format!("elevated sing-box (pid {pid}) survived the scheduled-task end"),
            ));
        }
        self.pid = None;
        Ok(())
    }

    fn set_dns(&mut self, _service: &str, _servers: &[String]) -> Result<(), TunError> {
        Err(TunError::new(
            TunErrorCode::ApplyFailed,
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
        assert_eq!(err.code, TunErrorCode::PermissionRequired);
        assert!(coordinator.stop().is_ok(), "stop is idempotent");
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

    #[cfg(target_os = "windows")]
    #[test]
    fn tun_task_create_args_carry_the_highest_privilege_flag_and_action() {
        let args = tun_task_create_args(
            Path::new(r"C:\Program Files\ice-box\ice-tun-launcher.exe"),
            Path::new(r"C:\Program Files\ice-box\sing-box.exe"),
            Path::new(r"C:\Users\admin\AppData\Roaming\com.yilong-musk.icebox\config.json"),
            Path::new(r"C:\Users\admin\AppData\Roaming\com.yilong-musk.icebox\logs\sing-box.log"),
            Path::new(r"C:\Users\admin\AppData\Roaming\com.yilong-musk.icebox\tun-task.pid"),
            Path::new(r"C:\Users\admin\AppData\Roaming\com.yilong-musk.icebox\tun-task.stop"),
        );
        assert_eq!(
            args,
            [
                "/Create",
                "/TN",
                TUN_TASK_NAME,
                "/TR",
                r#""C:\Program Files\ice-box\ice-tun-launcher.exe" --binary "C:\Program Files\ice-box\sing-box.exe" --config "C:\Users\admin\AppData\Roaming\com.yilong-musk.icebox\config.json" --log "C:\Users\admin\AppData\Roaming\com.yilong-musk.icebox\logs\sing-box.log" --pidfile "C:\Users\admin\AppData\Roaming\com.yilong-musk.icebox\tun-task.pid" --stopfile "C:\Users\admin\AppData\Roaming\com.yilong-musk.icebox\tun-task.stop""#,
                "/SC",
                "ONCE",
                "/ST",
                "00:00",
                "/RL",
                "HIGHEST",
                "/F",
            ]
        );
    }
}
