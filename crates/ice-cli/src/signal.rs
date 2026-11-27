// SPDX-License-Identifier: GPL-3.0-or-later

//! Cross-platform graceful termination signals for the headless daemon.
//!
//! - Unix (macOS and Linux): async-signal-safe self-pipe catching `SIGINT`,
//!   `SIGTERM`, and `SIGHUP` so the main thread wakes immediately and runs the
//!   bounded `request_headless_quit` cleanup.
//! - Windows: `SetConsoleCtrlHandler` catching `CTRL_C_EVENT`,
//!   `CTRL_BREAK_EVENT`, `CTRL_CLOSE_EVENT`, `CTRL_LOGOFF_EVENT`, and
//!   `CTRL_SHUTDOWN_EVENT`, paired with `ice_runtime::session_end::install`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Handle that blocks until an OS termination signal arrives (or until
/// `trigger()` is called programmatically in tests).
#[derive(Clone)]
pub struct ShutdownSignal {
    triggered: Arc<(Mutex<bool>, Condvar)>,
}

impl ShutdownSignal {
    pub fn new() -> Self {
        Self {
            triggered: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    /// Wake any thread waiting in [`Self::wait`].
    pub fn trigger(&self) {
        let (lock, cvar) = &*self.triggered;
        let mut fired = lock.lock().unwrap_or_else(|e| e.into_inner());
        *fired = true;
        cvar.notify_all();
    }

    pub fn is_triggered(&self) -> bool {
        let (lock, _) = &*self.triggered;
        *lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Block until triggered.
    pub fn wait(&self) {
        let (lock, cvar) = &*self.triggered;
        let mut fired = lock.lock().unwrap_or_else(|e| e.into_inner());
        while !*fired {
            fired = cvar.wait(fired).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Block until triggered or `timeout` elapses. Returns `true` if triggered.
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let (lock, cvar) = &*self.triggered;
        let fired = lock.lock().unwrap_or_else(|e| e.into_inner());
        if *fired {
            return true;
        }
        let (fired, _) = cvar
            .wait_timeout(fired, timeout)
            .unwrap_or_else(|e| e.into_inner());
        *fired
    }
}

impl Default for ShutdownSignal {
    fn default() -> Self {
        Self::new()
    }
}

/// Install OS signal handlers that trigger `signal`. Idempotent per process.
pub fn install_shutdown_handlers(signal: ShutdownSignal) -> Result<(), String> {
    static INSTALLED: AtomicBool = AtomicBool::new(false);
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    platform::install(signal)
}

#[cfg(unix)]
mod platform {
    use super::ShutdownSignal;
    use std::os::fd::RawFd;
    use std::sync::atomic::{AtomicI32, Ordering};

    static PIPE_WR: AtomicI32 = AtomicI32::new(-1);

    #[cfg(target_os = "linux")]
    unsafe fn errno_ptr() -> *mut libc::c_int {
        libc::__errno_location()
    }

    #[cfg(target_os = "macos")]
    unsafe fn errno_ptr() -> *mut libc::c_int {
        libc::__error()
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    unsafe fn errno_ptr() -> *mut libc::c_int {
        libc::__errno_location()
    }

    extern "C" fn handle_signal(sig: libc::c_int) {
        // SAFETY: preserve thread-local errno across async-signal-safe write().
        let saved_errno = unsafe { *errno_ptr() };
        let fd = PIPE_WR.load(Ordering::Relaxed);
        if fd >= 0 {
            let byte = (sig & 0xff) as u8;
            unsafe {
                let _ = libc::write(fd, (&byte as *const u8).cast::<libc::c_void>(), 1);
            }
        }
        unsafe {
            *errno_ptr() = saved_errno;
        }
    }

    fn create_self_pipe() -> Result<(RawFd, RawFd), std::io::Error> {
        let mut fds = [-1 as RawFd; 2];
        // SAFETY: `fds` is a valid 2-element output array for `pipe`.
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        for fd in fds {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } != 0
            {
                let err = std::io::Error::last_os_error();
                unsafe {
                    libc::close(fds[0]);
                    libc::close(fds[1]);
                }
                return Err(err);
            }
        }
        let flags = unsafe { libc::fcntl(fds[1], libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fds[1], libc::F_SETFL, flags | libc::O_NONBLOCK) } != 0
        {
            let err = std::io::Error::last_os_error();
            unsafe {
                libc::close(fds[0]);
                libc::close(fds[1]);
            }
            return Err(err);
        }
        Ok((fds[0], fds[1]))
    }

    fn register_handlers() -> Result<(), std::io::Error> {
        for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            // SAFETY: valid sigaction setup with an async-signal-safe handler.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = handle_signal as *const () as usize;
                action.sa_flags = libc::SA_RESTART;
                libc::sigemptyset(&mut action.sa_mask);
                if libc::sigaction(sig, &action, std::ptr::null_mut()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
        }
        Ok(())
    }

    fn reset_handlers_to_default() {
        for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = libc::SIG_DFL;
                libc::sigemptyset(&mut action.sa_mask);
                let _ = libc::sigaction(sig, &action, std::ptr::null_mut());
            }
        }
    }

    pub fn install(signal: ShutdownSignal) -> Result<(), String> {
        let (read_fd, write_fd) =
            create_self_pipe().map_err(|e| format!("create signal pipe: {e}"))?;
        PIPE_WR.store(write_fd, Ordering::SeqCst);
        if let Err(err) = register_handlers() {
            PIPE_WR.store(-1, Ordering::SeqCst);
            reset_handlers_to_default();
            unsafe {
                libc::close(read_fd);
                libc::close(write_fd);
            }
            return Err(format!("register signal handlers: {err}"));
        }
        std::thread::Builder::new()
            .name("ice-cli-signal".into())
            .spawn(move || {
                let mut buf = [0u8; 1];
                loop {
                    let n =
                        unsafe { libc::read(read_fd, buf.as_mut_ptr().cast::<libc::c_void>(), 1) };
                    if n > 0 {
                        let sig = i32::from(buf[0]);
                        tracing::info!(signal = sig, "received termination signal");
                        PIPE_WR.store(-1, Ordering::SeqCst);
                        reset_handlers_to_default();
                        signal.trigger();
                        break;
                    }
                    if n == 0 {
                        break;
                    }
                    if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                        break;
                    }
                }
                unsafe {
                    libc::close(read_fd);
                    libc::close(write_fd);
                }
            })
            .map_err(|e| format!("spawn signal watcher: {e}"))?;
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use super::ShutdownSignal;
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{BOOL, TRUE};
    use windows_sys::Win32::System::Console::{
        SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_C_EVENT, CTRL_LOGOFF_EVENT,
        CTRL_SHUTDOWN_EVENT,
    };

    static SIGNAL: OnceLock<ShutdownSignal> = OnceLock::new();

    unsafe extern "system" fn ctrl_handler(ctrl_type: u32) -> BOOL {
        match ctrl_type {
            CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT
            | CTRL_SHUTDOWN_EVENT => {
                if let Some(signal) = SIGNAL.get() {
                    signal.trigger();
                }
                TRUE
            }
            _ => 0,
        }
    }

    pub fn install(signal: ShutdownSignal) -> Result<(), String> {
        let _ = SIGNAL.set(signal);
        // SAFETY: `ctrl_handler` matches the Win32 `PHANDLER_ROUTINE` contract.
        let ok = unsafe { SetConsoleCtrlHandler(Some(ctrl_handler), TRUE) };
        if ok == 0 {
            return Err(format!(
                "SetConsoleCtrlHandler failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}
