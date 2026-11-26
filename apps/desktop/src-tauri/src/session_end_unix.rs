// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux-only: restore the system proxy when the desktop session terminates
//! via `SIGTERM` or `SIGHUP`.
//!
//! GTK/Tauri may exit before `RunEvent::ExitRequested` completes when the
//! display manager or systemd user session sends `SIGTERM`/`SIGHUP` on logout
//! or shutdown. Leaving GNOME `gsettings` or KDE `kioslaverc` pointing at the
//! dead local mixed port would break networking on the next login.
//!
//! A classic async-signal-safe self-pipe notifies a dedicated watcher thread
//! which restores `proxy-backup.json` and then terminates the process.

use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::OnceLock;

use ice_proxy_sys::SystemProxy;

/// Backup file the normal apply path writes; read from the signal thread.
static BACKUP_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Installed once per process; a second call is a no-op.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Write end of the self-pipe, written from the async-signal-safe handler.
static SIGNAL_PIPE_WR: AtomicI32 = AtomicI32::new(-1);

extern "C" fn handle_termination_signal(sig: libc::c_int) {
    let fd = SIGNAL_PIPE_WR.load(Ordering::Relaxed);
    if fd >= 0 {
        let byte = (sig & 0xff) as u8;
        // SAFETY: `write` of 1 byte to a non-blocking pipe fd is async-signal-safe.
        unsafe {
            let _ = libc::write(fd, (&byte as *const u8).cast::<libc::c_void>(), 1);
        }
    }
}

/// Install the `SIGTERM`/`SIGHUP` session-end hook. Must be called once on startup.
pub fn install(backup_path: PathBuf) {
    if BACKUP_PATH.set(backup_path).is_err() || INSTALLED.swap(true, Ordering::SeqCst) {
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
        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
        return;
    }

    let spawned = std::thread::Builder::new()
        .name("ice-box-session-end".into())
        .spawn(move || watch_pipe(read_fd));
    if let Err(err) = spawned {
        tracing::warn!(error = %err, "session-end proxy restore thread failed to start");
        SIGNAL_PIPE_WR.store(-1, Ordering::SeqCst);
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

fn watch_pipe(read_fd: RawFd) {
    let mut buf = [0u8; 1];
    loop {
        // SAFETY: `read_fd` is owned exclusively by this thread.
        let n = unsafe { libc::read(read_fd, buf.as_mut_ptr().cast::<libc::c_void>(), 1) };
        if n > 0 {
            let sig = i32::from(buf[0]);
            tracing::info!(
                signal = sig,
                "received termination signal; restoring system proxy"
            );
            restore_before_session_end();
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
    unsafe {
        libc::close(read_fd);
    }
}

fn restore_before_session_end() -> bool {
    let Some(path) = BACKUP_PATH.get() else {
        return false;
    };
    restore_backup(ice_proxy_sys::create_system_proxy().as_ref(), path)
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
        let sig = libc::SIGTERM as u8;
        let written = unsafe { libc::write(w, (&sig as *const u8).cast::<libc::c_void>(), 1) };
        assert_eq!(written, 1);
        let mut out = 0u8;
        let read = unsafe { libc::read(r, (&mut out as *mut u8).cast::<libc::c_void>(), 1) };
        assert_eq!(read, 1);
        assert_eq!(out, libc::SIGTERM as u8);
        unsafe {
            libc::close(r);
            libc::close(w);
        }
    }
}
