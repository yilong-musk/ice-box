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

/// Terminate with grace period, then force kill. Idempotent if already exited.
pub fn stop_process(child: &mut dyn ManagedProcess, grace: Duration) -> Result<(), CoreError> {
    match child.try_wait() {
        Ok(Some(_)) => return Ok(()),
        Ok(None) => {}
        Err(e) => return Err(CoreError::SpawnFailed(format!("try_wait: {e}"))),
    }

    let _ = child.request_terminate();
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(CoreError::SpawnFailed(format!("try_wait: {e}"))),
        }
    }

    let _ = child.force_kill();
    // Best-effort reap
    let _ = child.try_wait();
    Ok(())
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
