//! Core coordination for the native sing-box ownership path (plan §5 T2).
//!
//! macOS T0 lock (§24.5.2): adapter creation, address assignment, and route
//! installation are privileged, so the bundled sing-box must run elevated.
//! Production uses the privileged helper daemon (installed once via
//! launchd); a `sudo` wrapper is dev-only. `ice-tun-sys` never spawns the
//! core itself (architecture §22 keeps it free of `ice-core`); the
//! orchestration layer injects a `CoreCoordinator` that runs the core as
//! root, and sing-box owns the adapter / addresses / routes.

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
}
