//! Child process spawn / terminate helpers.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::CoreError;

/// How long to wait after SIGTERM before SIGKILL.
pub const STOP_GRACE_TIMEOUT: Duration = Duration::from_secs(3);

pub trait ManagedProcess: Send {
    fn id(&self) -> u32;
    fn request_terminate(&mut self) -> io::Result<()>;
    fn force_kill(&mut self) -> io::Result<()>;
    fn try_wait(&mut self) -> io::Result<Option<i32>>;
}

pub struct RealChild {
    child: Child,
}

impl ManagedProcess for RealChild {
    fn id(&self) -> u32 {
        self.child.id()
    }

    fn request_terminate(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            let rc = unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) };
            if rc == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
        #[cfg(windows)]
        {
            // Windows has no portable SIGTERM; kill is the terminate path.
            self.child.kill()
        }
        #[cfg(not(any(unix, windows)))]
        {
            self.child.kill()
        }
    }

    fn force_kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }

    fn try_wait(&mut self) -> io::Result<Option<i32>> {
        match self.child.try_wait()? {
            Some(status) => Ok(Some(status.code().unwrap_or(-1))),
            None => Ok(None),
        }
    }
}

pub trait ProcessSpawner: Send {
    fn spawn(
        &self,
        binary: &Path,
        config: &Path,
        log_file: &Path,
    ) -> Result<Box<dyn ManagedProcess>, CoreError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CommandSpawner;

impl ProcessSpawner for CommandSpawner {
    fn spawn(
        &self,
        binary: &Path,
        config: &Path,
        log_file: &Path,
    ) -> Result<Box<dyn ManagedProcess>, CoreError> {
        if !binary.is_file() {
            return Err(CoreError::NotFound(format!(
                "sing-box binary not found at {}",
                binary.display()
            )));
        }
        if !config.is_file() {
            return Err(CoreError::SpawnFailed(format!(
                "config not found at {}",
                config.display()
            )));
        }
        if let Some(parent) = log_file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CoreError::SpawnFailed(format!("create log dir {}: {e}", parent.display()))
            })?;
        }

        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file)
            .map_err(|e| CoreError::SpawnFailed(format!("open log {}: {e}", log_file.display())))?;
        let log_err = log
            .try_clone()
            .map_err(|e| CoreError::SpawnFailed(format!("clone log handle: {e}")))?;

        let mut command = Command::new(binary);
        command
            .arg("run")
            .arg("-c")
            .arg(config)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: do not pop a console window for the child,
            // and closing the console must not take the core down with it.
            command.creation_flags(0x08000000);
        }
        let child = command.spawn().map_err(|e| {
            CoreError::SpawnFailed(format!(
                "spawn {} -c {}: {e}",
                binary.display(),
                config.display()
            ))
        })?;

        Ok(Box::new(RealChild { child }))
    }
}

/// Whether an io error means the process is already gone (ESRCH). The
/// liveness probe and the signal can race with an exit, so a failed signal
/// to an already-dead pid is a successful stop.
fn is_esrch(err: &io::Error) -> bool {
    #[cfg(unix)]
    {
        err.raw_os_error() == Some(libc::ESRCH)
    }
    #[cfg(not(unix))]
    {
        let _ = err;
        false
    }
}

/// Terminate with grace period, then force kill. Idempotent if already exited.
///
/// Fails instead of pretending success when the process cannot be signalled
/// (e.g. an adopted root-owned pid: TERM/KILL return EPERM, and only the
/// privileged coordinator may terminate it). The caller must surface such a
/// failure rather than report the process as stopped.
pub fn stop_process(child: &mut dyn ManagedProcess, grace: Duration) -> Result<(), CoreError> {
    match child.try_wait() {
        Ok(Some(_)) => return Ok(()),
        Ok(None) => {}
        Err(e) => return Err(CoreError::SpawnFailed(format!("try_wait: {e}"))),
    }

    if let Err(e) = child.request_terminate() {
        if is_esrch(&e) {
            // Exited between the liveness probe and the signal.
            return Ok(());
        }
        return Err(CoreError::SpawnFailed(format!("request_terminate: {e}")));
    }
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(CoreError::SpawnFailed(format!("try_wait: {e}"))),
        }
    }

    if let Err(e) = child.force_kill() {
        if is_esrch(&e) {
            return Ok(());
        }
        return Err(CoreError::SpawnFailed(format!("force_kill: {e}")));
    }
    match child.try_wait() {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(CoreError::SpawnFailed(
            "process survived TERM and KILL; termination is unconfirmed".into(),
        )),
        Err(e) => Err(CoreError::SpawnFailed(format!("try_wait: {e}"))),
    }
}

/// A process the controller did not spawn itself (TUN slice: the elevated
/// core is started by the `CoreCoordinator` helper/`sudo` path and then
/// adopted so the normal lifecycle, health probes, reload and watchdog
/// reaping keep working). Identity is the pid only.
///
/// `try_wait` probes liveness via `kill(pid, 0)`; the exit code is
/// unavailable, so a detected exit reports `-1`.
#[derive(Debug, Clone)]
pub struct PidProcess {
    pid: u32,
}

impl PidProcess {
    pub fn new(pid: u32) -> Self {
        Self { pid }
    }
}

impl ManagedProcess for PidProcess {
    fn id(&self) -> u32 {
        self.pid
    }

    fn request_terminate(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            let rc = unsafe { libc::kill(self.pid as i32, libc::SIGTERM) };
            if rc == 0 {
                Ok(())
            } else {
                let err = io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EPERM) {
                    // The process belongs to another user (the elevated root
                    // core); this process cannot signal it. The coordinator
                    // (privileged helper / sudo stop) owns termination.
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!(
                            "pid {0} is owned by another user; terminate it via the privileged coordinator",
                            self.pid
                        ),
                    ));
                }
                Err(err)
            }
        }
        #[cfg(not(unix))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "PidProcess termination requires a unix host",
            ))
        }
    }

    fn force_kill(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            let rc = unsafe { libc::kill(self.pid as i32, libc::SIGKILL) };
            if rc == 0 {
                Ok(())
            } else {
                let err = io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EPERM) {
                    // See `request_terminate`: a root-owned process is not
                    // signalable by this process; the coordinator owns it.
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!(
                            "pid {0} is owned by another user; terminate it via the privileged coordinator",
                            self.pid
                        ),
                    ));
                }
                Err(err)
            }
        }
        #[cfg(not(unix))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "PidProcess termination requires a unix host",
            ))
        }
    }

    fn try_wait(&mut self) -> io::Result<Option<i32>> {
        #[cfg(unix)]
        {
            // `kill(pid, 0)` is a pure liveness probe:
            // - 0     → the process exists and we may signal it → alive.
            // - EPERM → the process exists but belongs to another user (the
            //   elevated root core adopted from the helper/`sudo` path). It is
            //   alive; signals cannot reach it from this process, so liveness
            //   is the only thing `try_wait` can report.
            // - ESRCH → the process is gone (or never existed).
            let rc = unsafe { libc::kill(self.pid as i32, 0) };
            if rc == 0 {
                Ok(None)
            } else {
                let err = io::Error::last_os_error();
                match err.raw_os_error() {
                    Some(libc::ESRCH) => Ok(Some(-1)),
                    Some(libc::EPERM) => Ok(None),
                    _ => Err(err),
                }
            }
        }
        #[cfg(not(unix))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "PidProcess liveness requires a unix host",
            ))
        }
    }
}

/// Mock process for unit tests.
pub struct MockProcess {
    pub id: u32,
    pub alive: bool,
    pub terminate_calls: u32,
    pub kill_calls: u32,
}

impl MockProcess {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            alive: true,
            terminate_calls: 0,
            kill_calls: 0,
        }
    }
}

impl ManagedProcess for MockProcess {
    fn id(&self) -> u32 {
        self.id
    }

    fn request_terminate(&mut self) -> io::Result<()> {
        self.terminate_calls += 1;
        self.alive = false;
        Ok(())
    }

    fn force_kill(&mut self) -> io::Result<()> {
        self.kill_calls += 1;
        self.alive = false;
        Ok(())
    }

    fn try_wait(&mut self) -> io::Result<Option<i32>> {
        if self.alive {
            Ok(None)
        } else {
            Ok(Some(0))
        }
    }
}

/// Spawner that returns a mock process (and optionally fails).
pub struct MockSpawner {
    pub next_pid: std::sync::atomic::AtomicU32,
    pub fail: bool,
    pub killed_pids: std::sync::Arc<std::sync::Mutex<Vec<u32>>>,
    pub spawn_count: std::sync::atomic::AtomicU32,
}

impl Default for MockSpawner {
    fn default() -> Self {
        Self {
            next_pid: std::sync::atomic::AtomicU32::new(4242),
            fail: false,
            killed_pids: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            spawn_count: std::sync::atomic::AtomicU32::new(0),
        }
    }
}

impl MockSpawner {
    pub fn with_start_pid(pid: u32) -> Self {
        Self {
            next_pid: std::sync::atomic::AtomicU32::new(pid),
            ..Self::default()
        }
    }
}

impl ProcessSpawner for MockSpawner {
    fn spawn(
        &self,
        _binary: &Path,
        _config: &Path,
        _log_file: &Path,
    ) -> Result<Box<dyn ManagedProcess>, CoreError> {
        if self.fail {
            return Err(CoreError::SpawnFailed("mock spawn failed".into()));
        }
        self.spawn_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let pid = self
            .next_pid
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let killed = self.killed_pids.clone();
        Ok(Box::new(TrackingMock {
            inner: MockProcess::new(pid),
            killed,
        }))
    }
}

struct TrackingMock {
    inner: MockProcess,
    killed: std::sync::Arc<std::sync::Mutex<Vec<u32>>>,
}

impl ManagedProcess for TrackingMock {
    fn id(&self) -> u32 {
        self.inner.id()
    }

    fn request_terminate(&mut self) -> io::Result<()> {
        let id = self.inner.id();
        self.inner.request_terminate()?;
        self.killed.lock().expect("lock").push(id);
        Ok(())
    }

    fn force_kill(&mut self) -> io::Result<()> {
        let id = self.inner.id();
        self.inner.force_kill()?;
        self.killed.lock().expect("lock").push(id);
        Ok(())
    }

    fn try_wait(&mut self) -> io::Result<Option<i32>> {
        self.inner.try_wait()
    }
}

/// Open log in append mode (used by tests to assert file creation).
#[allow(dead_code)]
pub fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn pid_process_reports_liveness_for_own_and_exited_processes() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let mut pid_proc = PidProcess::new(child.id());
        assert_eq!(
            pid_proc.try_wait().expect("try_wait live"),
            None,
            "a running process reports alive"
        );

        // Terminate and reap so the pid is really gone (ESRCH).
        let _ = child.kill();
        child.wait().expect("wait reap");
        let mut gone = false;
        for _ in 0..200 {
            if pid_proc.try_wait().expect("try_wait gone") == Some(-1) {
                gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(gone, "a reaped process must report exited");
    }

    #[cfg(unix)]
    #[test]
    fn pid_process_terminate_fails_with_permission_denied_for_foreign_pid() {
        // PID 1 (launchd/init) exists but belongs to root. When this test
        // runs as root it *can* signal PID 1, so only assert the EPERM
        // contract from a non-root process.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let mut pid_proc = PidProcess::new(1);
        let err = pid_proc.request_terminate().expect_err("EPERM");
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        let err = pid_proc.force_kill().expect_err("EPERM");
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        // Liveness still resolves: the process is alive, just not signalable.
        assert_eq!(pid_proc.try_wait().expect("EPERM is alive"), None);
    }

    #[cfg(unix)]
    #[test]
    fn stop_process_propagates_permission_denied_for_foreign_pid() {
        // PID 1 (launchd/init) exists but belongs to root. A non-root
        // process cannot signal it, so stop_process must fail instead of
        // reporting the process as stopped.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let mut pid_proc = PidProcess::new(1);
        let err = stop_process(&mut pid_proc, Duration::from_millis(10)).expect_err("EPERM");
        assert!(
            err.to_string().contains("request_terminate"),
            "stop must surface the signal failure: {err}"
        );
    }
}
