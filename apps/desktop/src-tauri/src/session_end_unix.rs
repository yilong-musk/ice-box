// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux-only: run graceful shutdown (stopping core/TUN and restoring the
//! system proxy) when the desktop session terminates via `SIGTERM` or `SIGHUP`.
//!
//! GTK/Tauri may exit before `RunEvent::ExitRequested` completes when the
//! display manager or systemd user session sends `SIGTERM`/`SIGHUP` on logout
//! or shutdown. Leaving GNOME `gsettings` or KDE `kioslaverc` pointing at the
//! dead local mixed port, or leaving the root TUN helper core running, would
//! break networking on the next login.
//!
//! A classic async-signal-safe self-pipe notifies a dedicated watcher thread
//! which resets the signal handlers to `SIG_DFL`, arms a hard `_exit`
//! watchdog, runs `shutdown::request_tray_quit` within a bounded deadline,
//! falls back to direct `proxy-backup.json` restore if needed, and exits.

use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::Duration;

use ice_proxy_sys::SystemProxy;
use tauri::{AppHandle, Manager};

use crate::shutdown::QuitOutcome;

/// Maximum time to wait for `shutdown::request_tray_quit` before falling back
/// to direct `proxy-backup.json` restoration.
const GRACEFUL_QUIT_TIMEOUT: Duration = Duration::from_secs(6);

/// Hard deadline after which the watchdog terminates the process via `_exit`,
/// ensuring logout/shutdown can never stall until systemd's 90s SIGKILL.
const HARD_EXIT_DEADLINE: Duration = Duration::from_secs(10);

/// Installed once per process; a second call is a no-op.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Write end of the self-pipe, written from the async-signal-safe handler.
static SIGNAL_PIPE_WR: AtomicI32 = AtomicI32::new(-1);

/// Write the received signal number to the non-blocking self-pipe while
/// preserving thread-local `errno`. 100% async-signal-safe.
fn write_signal_byte_preserving_errno(fd: RawFd, sig: libc::c_int) {
    // SAFETY: `__errno_location` returns a valid thread-local pointer on Linux.
    // Saving and restoring `errno` ensures a non-blocking `write` returning
    // `EAGAIN` cannot clobber the interrupted thread's `errno`.
    let saved_errno = unsafe { *libc::__errno_location() };
    if fd >= 0 {
        let byte = (sig & 0xff) as u8;
        // SAFETY: `write` of 1 byte to a non-blocking pipe fd is async-signal-safe.
        unsafe {
            let _ = libc::write(fd, (&byte as *const u8).cast::<libc::c_void>(), 1);
        }
    }
    // SAFETY: restoring the previously saved `errno` value.
    unsafe {
        *libc::__errno_location() = saved_errno;
    }
}

extern "C" fn handle_termination_signal(sig: libc::c_int) {
    write_signal_byte_preserving_errno(SIGNAL_PIPE_WR.load(Ordering::Relaxed), sig);
}

/// Install the `SIGTERM`/`SIGHUP` session-end hook. Must be called once on startup.
pub fn install(app_handle: AppHandle, backup_path: PathBuf) {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }

    let (read_fd, write_fd) = match create_self_pipe() {
        Ok(pair) => pair,
        Err(err) => {
            tracing::warn!(error = %err, "session-end signal pipe creation failed");
            return;
        }
    };

    SIGNAL_PIPE_WR.store(write_fd, Ordering::SeqCst);

    if let Err(err) = register_signal_handlers() {
        tracing::warn!(error = %err, "session-end signal handler registration failed");
        SIGNAL_PIPE_WR.store(-1, Ordering::SeqCst);
        reset_signal_handlers_to_default();
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
        return;
    }

    let spawned = std::thread::Builder::new()
        .name("ice-box-session-end".into())
        .spawn(move || watch_pipe(read_fd, app_handle, backup_path));
    if let Err(err) = spawned {
        tracing::warn!(error = %err, "session-end proxy restore thread failed to start");
        SIGNAL_PIPE_WR.store(-1, Ordering::SeqCst);
        reset_signal_handlers_to_default();
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
    }
}

fn create_self_pipe() -> Result<(RawFd, RawFd), std::io::Error> {
    let mut fds = [-1 as RawFd; 2];
    // SAFETY: `fds` is a valid 2-element array for `pipe2`.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // Make the write end non-blocking so the signal handler can never stall.
    let flags = unsafe { libc::fcntl(fds[1], libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fds[1], libc::F_SETFL, flags | libc::O_NONBLOCK) } != 0 {
        let err = std::io::Error::last_os_error();
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
        return Err(err);
    }
    Ok((fds[0], fds[1]))
}

fn register_signal_handlers() -> Result<(), std::io::Error> {
    for sig in [libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: `sigaction` is initialized zeroed with a valid `extern "C"` handler.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = handle_termination_signal as *const () as usize;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            if libc::sigaction(sig, &action, std::ptr::null_mut()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

fn reset_signal_handlers_to_default() {
    for sig in [libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: `SIG_DFL` is a valid disposition for `sigaction`.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = libc::SIG_DFL;
            libc::sigemptyset(&mut action.sa_mask);
            let _ = libc::sigaction(sig, &action, std::ptr::null_mut());
        }
    }
}

fn hard_exit_code(sig: libc::c_int) -> libc::c_int {
    if (1..=127).contains(&sig) {
        128 + sig
    } else {
        0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionEndAction {
    GracefulStopped,
    FallbackRestored(bool),
}

fn run_session_end_cleanup<Q, F>(
    graceful_quit: Q,
    fallback_restore: F,
    quit_timeout: Duration,
) -> SessionEndAction
where
    Q: FnOnce() -> QuitOutcome + Send + 'static,
    F: FnOnce() -> bool,
{
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("ice-box-session-end-quit".into())
        .spawn(move || {
            let outcome = graceful_quit();
            let _ = tx.send(outcome);
        });

    let graceful_ok = match spawned {
        Ok(_) => match rx.recv_timeout(quit_timeout) {
            Ok(QuitOutcome::Stopped) => true,
            Ok(outcome) => {
                tracing::warn!(
                    ?outcome,
                    "graceful quit did not complete cleanly at Linux session end; falling back to proxy restore"
                );
                false
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                tracing::warn!(
                    timeout_secs = quit_timeout.as_secs(),
                    "graceful quit timed out at Linux session end; falling back to proxy restore"
                );
                false
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                tracing::warn!(
                    "graceful quit worker terminated unexpectedly at Linux session end; falling back to proxy restore"
                );
                false
            }
        },
        Err(err) => {
            tracing::warn!(
                error = %err,
                "failed to spawn graceful quit worker at Linux session end; falling back to proxy restore"
            );
            false
        }
    };

    if graceful_ok {
        SessionEndAction::GracefulStopped
    } else {
        SessionEndAction::FallbackRestored(fallback_restore())
    }
}

fn watch_pipe(read_fd: RawFd, app_handle: AppHandle, backup_path: PathBuf) {
    let mut buf = [0u8; 1];
    loop {
        // SAFETY: `read_fd` is owned exclusively by this thread.
        let n = unsafe { libc::read(read_fd, buf.as_mut_ptr().cast::<libc::c_void>(), 1) };
        if n > 0 {
            let sig = i32::from(buf[0]);
            // Immediately restore default signal dispositions so a second
            // SIGTERM/SIGHUP terminates the process right away.
            SIGNAL_PIPE_WR.store(-1, Ordering::SeqCst);
            reset_signal_handlers_to_default();

            tracing::info!(
                signal = sig,
                "received termination signal; starting bounded graceful shutdown"
            );

            // Arm a hard-deadline watchdog so logout/shutdown can never stall
            // until systemd's 90s SIGKILL even if D-Bus, helper IPC, or exit
            // handlers hang.
            let watchdog_code = hard_exit_code(sig);
            let _ = std::thread::Builder::new()
                .name("ice-box-session-end-watchdog".into())
                .spawn(move || {
                    std::thread::sleep(HARD_EXIT_DEADLINE);
                    // SAFETY: `_exit` terminates immediately without running
                    // destructors or blocking on held locks.
                    unsafe { libc::_exit(watchdog_code) };
                });

            let quit_handle = app_handle.clone();
            let _ = run_session_end_cleanup(
                move || {
                    if quit_handle.try_state::<crate::AppState>().is_none() {
                        // AppState not initialized yet; fall back to restoring
                        // any prior `proxy-backup.json`.
                        return QuitOutcome::StopFailed;
                    }
                    crate::shutdown::request_tray_quit(&quit_handle)
                },
                || restore_before_session_end(&backup_path),
                GRACEFUL_QUIT_TIMEOUT,
            );

            unsafe {
                libc::close(read_fd);
            }
            std::process::exit(0);
        }
        if n == 0 {
            break;
        }
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::Interrupted {
            break;
        }
    }
    // Never leave SIGTERM/SIGHUP swallowed if the watcher exits unexpectedly.
    SIGNAL_PIPE_WR.store(-1, Ordering::SeqCst);
    reset_signal_handlers_to_default();
    unsafe {
        libc::close(read_fd);
    }
}

fn restore_before_session_end(backup_path: &Path) -> bool {
    restore_backup(ice_proxy_sys::create_system_proxy().as_ref(), backup_path)
}

fn restore_backup(proxy: &dyn SystemProxy, path: &Path) -> bool {
    match ice_proxy_sys::restore_and_clear_flag(path, proxy) {
        Ok(true) => {
            tracing::info!("restored system proxy before Linux session end");
            true
        }
        Ok(false) => false,
        Err(err) => {
            tracing::error!(
                error = %err,
                "system proxy restore failed at Linux session end"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ice_proxy_sys::{ProxyBackup, ProxyBackupFile, ProxyEndpoints, ProxySysError};
    use std::cell::Cell;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct CountingProxy {
        restore_calls: Cell<usize>,
    }

    impl SystemProxy for CountingProxy {
        fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
            Ok(ProxyBackup::default())
        }

        fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
            Ok(())
        }

        fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
            self.restore_calls.set(self.restore_calls.get() + 1);
            Ok(())
        }
    }

    #[test]
    fn session_end_unix_restores_once_and_clears_the_flag() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-session-end-unix-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("proxy-backup.json");
        ProxyBackupFile {
            applied: true,
            pending_apply: false,
            applied_at: None,
            endpoints: ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: Some("127.0.0.1".into()),
                socks_port: Some(17890),
            },
            backup: ProxyBackup::default(),
        }
        .save(&path)
        .unwrap();
        let proxy = CountingProxy {
            restore_calls: Cell::new(0),
        };

        assert!(restore_backup(&proxy, &path));
        assert_eq!(proxy.restore_calls.get(), 1);
        assert!(!ProxyBackupFile::load(&path).unwrap().applied);

        assert!(!restore_backup(&proxy, &path));
        assert_eq!(proxy.restore_calls.get(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn self_pipe_roundtrips_signal_byte() {
        let (r, w) = create_self_pipe().expect("pipe");
        write_signal_byte_preserving_errno(w, libc::SIGTERM);
        let mut out = 0u8;
        let read = unsafe { libc::read(r, (&mut out as *mut u8).cast::<libc::c_void>(), 1) };
        assert_eq!(read, 1);
        assert_eq!(out, libc::SIGTERM as u8);
        unsafe {
            libc::close(r);
            libc::close(w);
        }
    }

    #[test]
    fn signal_handler_write_preserves_thread_errno_on_error() {
        let (r, w) = create_self_pipe().expect("pipe");
        // Fill the non-blocking pipe so the next write hits EAGAIN.
        let chunk = [0u8; 4096];
        loop {
            let n = unsafe { libc::write(w, chunk.as_ptr().cast::<libc::c_void>(), chunk.len()) };
            if n < 0 {
                break;
            }
        }

        let sentinel_errno = libc::ENOENT;
        unsafe {
            *libc::__errno_location() = sentinel_errno;
        }
        write_signal_byte_preserving_errno(w, libc::SIGHUP);
        let after_errno = unsafe { *libc::__errno_location() };
        assert_eq!(
            after_errno, sentinel_errno,
            "EAGAIN from self-pipe write must not clobber thread errno"
        );

        unsafe {
            libc::close(r);
            libc::close(w);
        }
    }

    #[test]
    fn session_end_cleanup_skips_fallback_when_graceful_quit_stops() {
        let fallback_called = Cell::new(false);
        let action = run_session_end_cleanup(
            || QuitOutcome::Stopped,
            || {
                fallback_called.set(true);
                true
            },
            Duration::from_secs(1),
        );
        assert_eq!(action, SessionEndAction::GracefulStopped);
        assert!(!fallback_called.get());
    }

    #[test]
    fn session_end_cleanup_falls_back_on_timeout_and_quit_errors() {
        let fallback_called = Cell::new(false);
        let action = run_session_end_cleanup(
            || {
                std::thread::sleep(Duration::from_millis(250));
                QuitOutcome::Stopped
            },
            || {
                fallback_called.set(true);
                true
            },
            Duration::from_millis(30),
        );
        assert_eq!(action, SessionEndAction::FallbackRestored(true));
        assert!(fallback_called.get());

        let fallback_on_err = Cell::new(false);
        let action = run_session_end_cleanup(
            || QuitOutcome::ProxyRestoreFailed,
            || {
                fallback_on_err.set(true);
                true
            },
            Duration::from_secs(1),
        );
        assert_eq!(action, SessionEndAction::FallbackRestored(true));
        assert!(fallback_on_err.get());
    }

    #[test]
    fn hard_exit_code_uses_128_plus_signal() {
        assert_eq!(hard_exit_code(libc::SIGTERM), 128 + libc::SIGTERM);
        assert_eq!(hard_exit_code(libc::SIGHUP), 128 + libc::SIGHUP);
        assert_eq!(hard_exit_code(0), 0);
    }
}
