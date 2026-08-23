//! Reload strategies for the running sing-box process.
//!
//! sing-box 1.13.x does not implement config reload through its Clash API
//! (`PUT /configs` is a 204 stub), so the only in-place reload is **SIGHUP**:
//! `sing-box run -c <config>` re-validates and rebuilds the whole service from
//! the same config path while keeping the process (and PID) alive.

use std::time::Duration;

use crate::error::CoreError;
use crate::process::ManagedProcess;

/// Wait before re-spawn on Windows so the previous listener can release the port.
pub const WINDOWS_PORT_RELEASE_WAIT: Duration = Duration::from_millis(500);

pub trait ConfigReloader: Send {
    /// Ask the running sing-box process to pick up `config.json` in place.
    /// Any error means the caller falls back to a full process restart.
    fn reload(&self, process: &mut dyn ManagedProcess) -> Result<(), CoreError>;
}

/// SIGHUP reloader for Unix. On other platforms reloads are unsupported and the
/// caller falls back to a process restart.
#[derive(Debug, Default, Clone, Copy)]
pub struct SignalReloader;

impl ConfigReloader for SignalReloader {
    fn reload(&self, process: &mut dyn ManagedProcess) -> Result<(), CoreError> {
        #[cfg(unix)]
        {
            let pid = process.id();
            let rc = unsafe { libc::kill(pid as i32, libc::SIGHUP) };
            if rc != 0 {
                return Err(CoreError::SpawnFailed(format!(
                    "send SIGHUP to sing-box (pid {pid}): {}",
                    std::io::Error::last_os_error()
                )));
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = process;
            Err(CoreError::SpawnFailed(
                "signal reload unsupported on this platform".into(),
            ))
        }
    }
}

/// Mock reloader for unit tests.
#[derive(Debug, Clone)]
pub struct MockReloader {
    pub mode: MockReloadMode,
    pub calls: std::sync::Arc<std::sync::atomic::AtomicU32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockReloadMode {
    Ok,
    Http5xx,
    Timeout,
}

impl Default for MockReloader {
    fn default() -> Self {
        Self {
            mode: MockReloadMode::Ok,
            calls: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }
}

impl MockReloader {
    pub fn new(mode: MockReloadMode) -> Self {
        Self {
            mode,
            calls: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    pub fn call_count(&self) -> u32 {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl ConfigReloader for MockReloader {
    fn reload(&self, _process: &mut dyn ManagedProcess) -> Result<(), CoreError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match self.mode {
            MockReloadMode::Ok => Ok(()),
            MockReloadMode::Http5xx => {
                Err(CoreError::SpawnFailed("mock reload signal failed".into()))
            }
            MockReloadMode::Timeout => {
                Err(CoreError::SpawnFailed("mock reload signal timeout".into()))
            }
        }
    }
}
