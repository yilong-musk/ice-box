// SPDX-License-Identifier: GPL-3.0-or-later

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
        let _ = crate::rotate_sized_log(log_file, crate::CORE_LOG_MAX_BYTES, crate::CORE_LOG_KEEP);

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

/// Shared pid liveness used by [`PidProcess::try_wait`] and orphan reclaim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PidLiveness {
    Alive,
    Exited(i32),
}

/// Whether `pid` is still a live (non-zombie) process. `ERROR_ACCESS_DENIED` /
/// `EPERM` count as alive.
pub fn pid_is_alive(pid: u32) -> bool {
    matches!(query_pid_liveness(pid), Ok(PidLiveness::Alive))
}

pub fn query_pid_liveness(pid: u32) -> io::Result<PidLiveness> {
    #[cfg(unix)]
    {
        if pid_is_zombie(pid) {
            return Ok(PidLiveness::Exited(-1));
        }
        let rc = unsafe { libc::kill(pid as i32, 0) };
        if rc == 0 {
            return Ok(PidLiveness::Alive);
        }
        let err = io::Error::last_os_error();
        match err.raw_os_error() {
            Some(libc::ESRCH) => Ok(PidLiveness::Exited(-1)),
            Some(libc::EPERM) => Ok(PidLiveness::Alive),
            _ => Err(err),
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{
            CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, STILL_ACTIVE,
        };
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return match io::Error::last_os_error().raw_os_error() {
                Some(err) if err == ERROR_INVALID_PARAMETER as i32 => Ok(PidLiveness::Exited(-1)),
                Some(err) if err == ERROR_ACCESS_DENIED as i32 => Ok(PidLiveness::Alive),
                _ => Err(io::Error::last_os_error()),
            };
        }
        let mut exit_code: u32 = 0;
        let queried = unsafe { GetExitCodeProcess(handle, &mut exit_code) };
        unsafe { CloseHandle(handle) };
        if queried == 0 {
            return Err(io::Error::last_os_error());
        }
        if exit_code == STILL_ACTIVE as u32 {
            Ok(PidLiveness::Alive)
        } else {
            Ok(PidLiveness::Exited(exit_code as i32))
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "pid liveness requires a unix or windows host",
        ))
    }
}

#[cfg(unix)]
fn waitid_reports_exited(pid: u32) -> bool {
    unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        let rc = libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        );
        rc == 0 && {
            // Linux exposes `si_pid()` as a method; macOS/BSD keep a field.
            #[cfg(target_os = "linux")]
            let reported = info.si_pid();
            #[cfg(not(target_os = "linux"))]
            let reported = info.si_pid;
            reported == pid as libc::pid_t
        }
    }
}

#[cfg(target_os = "linux")]
fn pid_is_zombie(pid: u32) -> bool {
    if waitid_reports_exited(pid) {
        return true;
    }
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    let Some(rparen) = stat.rfind(')') else {
        return false;
    };
    stat[rparen + 1..].trim_start().starts_with('Z')
}

#[cfg(target_os = "macos")]
fn pid_is_zombie(pid: u32) -> bool {
    if waitid_reports_exited(pid) {
        return true;
    }
    unsafe {
        let mut info: libc::proc_bsdinfo = std::mem::zeroed();
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        let n = libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        );
        n == size && info.pbi_status == libc::SZOMB
    }
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn pid_is_zombie(pid: u32) -> bool {
    waitid_reports_exited(pid)
}

/// A process the controller did not spawn itself (TUN slice: the elevated
/// core is started by the `CoreCoordinator` helper/`sudo` path and then
/// adopted so the normal lifecycle, health probes, reload and watchdog
/// reaping keep working). Identity is the pid plus a platform start-key so a
/// reused pid is not signalled.
///
/// `try_wait` probes liveness: `kill(pid, 0)` on unix, `OpenProcess` +
/// `GetExitCodeProcess` on Windows; the exit code is unavailable on unix, so
/// a detected exit reports `-1`.
#[derive(Debug, Clone)]
pub struct PidProcess {
    pid: u32,
    start_key: Option<String>,
}

impl PidProcess {
    pub fn new(pid: u32) -> Self {
        Self {
            pid,
            start_key: process_start_key(pid),
        }
    }

    fn identity_still_holds(&self) -> io::Result<bool> {
        match (&self.start_key, process_start_key(self.pid)) {
            (Some(expected), Some(actual)) if expected == &actual => Ok(true),
            (Some(_), Some(_)) => Err(io::Error::other(format!(
                "pid {} was reused; refusing to signal the new process",
                self.pid
            ))),
            // Process is gone (or the start key is unavailable): treat as
            // already exited rather than signalling a stranger.
            (Some(_), None) => Ok(false),
            (None, _) => Ok(true),
        }
    }
}

impl ManagedProcess for PidProcess {
    fn id(&self) -> u32 {
        self.pid
    }

    fn request_terminate(&mut self) -> io::Result<()> {
        if !self.identity_still_holds()? {
            return Ok(());
        }
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
        #[cfg(windows)]
        {
            // Graceful-first (mirrors the coordinator's taskkill `/T` without
            // `/F`: WM_CLOSE, which sing-box treats as a shutdown signal and
            // uses to remove its WFP filters and routes). A pid that is
            // already gone is a no-op. CREATE_NO_WINDOW: a console child of
            // the GUI app would flash a black window.
            use std::os::windows::process::CommandExt;
            let status = Command::new("taskkill")
                .args(["/PID", &self.pid.to_string(), "/T"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
                .status();
            match status {
                Ok(_) => Ok(()),
                Err(err) => Err(io::Error::new(
                    err.kind(),
                    format!("taskkill /PID {} /T: {err}", self.pid),
                )),
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "PidProcess termination requires a unix or windows host",
            ))
        }
    }

    fn force_kill(&mut self) -> io::Result<()> {
        if !self.identity_still_holds()? {
            return Ok(());
        }
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
        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::{
                CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER,
            };
            use windows_sys::Win32::System::Threading::{
                OpenProcess, TerminateProcess, PROCESS_TERMINATE,
            };
            let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, self.pid) };
            if handle.is_null() {
                return match io::Error::last_os_error().raw_os_error() {
                    // The pid is already gone.
                    Some(err) if err == ERROR_INVALID_PARAMETER as i32 => Ok(()),
                    // The pid exists but belongs to another token; the
                    // privileged coordinator owns termination.
                    Some(err) if err == ERROR_ACCESS_DENIED as i32 => Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!(
                            "pid {0} is owned by another token; terminate it via the privileged coordinator",
                            self.pid
                        ),
                    )),
                    _ => Err(io::Error::last_os_error()),
                };
            }
            let ok = unsafe { TerminateProcess(handle, 1) };
            unsafe { CloseHandle(handle) };
            if ok == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "PidProcess termination requires a unix or windows host",
            ))
        }
    }

    fn try_wait(&mut self) -> io::Result<Option<i32>> {
        match query_pid_liveness(self.pid)? {
            PidLiveness::Alive => Ok(None),
            PidLiveness::Exited(code) => Ok(Some(code)),
        }
    }
}

/// Platform start-time token for `pid`. Compared before signalling an adopted
/// process so a recycled pid is not killed.
pub(crate) fn process_start_key(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let rparen = stat.rfind(')')?;
        let fields: Vec<&str> = stat[rparen + 1..].split_whitespace().collect();
        // Field 22 (starttime) is index 19 after `(comm)`.
        fields.get(19).map(|s| (*s).to_string())
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        let output = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "lstart="])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if key.is_empty() {
            None
        } else {
            Some(key)
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut created = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut exited = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut kernel = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let mut user = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let ok = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user);
            CloseHandle(handle);
            if ok == 0 {
                return None;
            }
            let ticks = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
            Some(ticks.to_string())
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        None
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
    // Every test in this module exercises PidProcess behavior (liveness /
    // signals), so the parent-import is needed on both unix and windows.
    #[cfg(any(unix, windows))]
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

    #[cfg(windows)]
    #[test]
    fn pid_process_windows_liveness_reports_alive_and_gone() {
        let mut child = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"])
            .spawn()
            .expect("spawn powershell");
        let mut pid_proc = PidProcess::new(child.id());
        assert_eq!(
            pid_proc.try_wait().expect("try_wait live"),
            None,
            "a running process reports alive"
        );

        let _ = child.kill();
        child.wait().expect("wait reap");
        // Windows keeps the exit code queryable while `child` still holds the
        // process handle, so a reaped process reports its real exit code
        // (any `Some`), unlike unix where the code is unavailable (-1).
        let mut gone = false;
        for _ in 0..200 {
            if pid_proc.try_wait().expect("try_wait gone").is_some() {
                gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(gone, "a reaped process must report exited");

        // A pid that never existed reads as gone, not as an error.
        let mut phantom = PidProcess::new(4_000_000_000);
        assert_eq!(
            phantom.try_wait().expect("phantom pid"),
            Some(-1),
            "a non-existent pid must report exited"
        );
    }

    #[cfg(windows)]
    #[test]
    fn pid_process_windows_terminate_stops_the_process() {
        let mut child = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"])
            .spawn()
            .expect("spawn powershell");
        let mut pid_proc = PidProcess::new(child.id());

        pid_proc.force_kill().expect("force_kill");
        let mut gone = false;
        for _ in 0..200 {
            if pid_proc.try_wait().expect("try_wait gone").is_some() {
                gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(gone, "force_kill must terminate the process");
        let _ = child.wait();
    }

    #[cfg(unix)]
    #[test]
    fn pid_process_try_wait_reports_zombie_as_exited() {
        unsafe {
            let pid = libc::fork();
            assert!(pid >= 0, "fork");
            if pid == 0 {
                libc::_exit(0);
            }
            let mut pid_proc = PidProcess::new(pid as u32);
            let mut saw_exit = false;
            for _ in 0..200 {
                if pid_proc.try_wait().expect("try_wait") == Some(-1) {
                    saw_exit = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            libc::waitpid(pid, std::ptr::null_mut(), 0);
            assert!(saw_exit, "an unreaped zombie must report exited");
        }
    }

    #[cfg(windows)]
    #[test]
    fn pid_is_alive_treats_system_process_as_alive() {
        assert!(
            pid_is_alive(4),
            "pid 4 (System) is ACCESS_DENIED and must count as alive"
        );
    }
}
