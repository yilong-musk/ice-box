// SPDX-License-Identifier: GPL-3.0-or-later

//! Windows-only: restore the system proxy when the session ends.
//!
//! Tao does not process `WM_QUERYENDSESSION`/`WM_ENDSESSION`, and Windows may
//! terminate the app while the main thread is busy or hung. The proxy registry
//! entry then survives the reboot and points at a core that is no longer
//! running: every proxy-aware app reports "no internet" until ice-box is
//! started again. A dedicated thread owns a hidden top-level window; Windows
//! broadcasts the session-end message to it, so the backup is restored even
//! when the main thread cannot run the normal quit path.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use ice_proxy_sys::SystemProxy;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, TRUE, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostQuitMessage,
    RegisterClassW, TranslateMessage, MSG, WM_DESTROY, WM_ENDSESSION, WM_QUERYENDSESSION,
    WNDCLASSW, WS_OVERLAPPED,
};

/// Backup file the normal apply path writes; read from the session-end thread.
static BACKUP_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Installed once per process; a second call is a no-op.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Install the session-end hook. Must be called once on startup.
pub fn install(backup_path: PathBuf) {
    if BACKUP_PATH.set(backup_path).is_err() || INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("ice-box-session-end".into())
        .spawn(run);
    if let Err(err) = spawned {
        tracing::warn!(error = %err, "session-end proxy restore thread failed to start");
    }
}

fn run() {
    // SAFETY: the arguments follow the Win32 contract: a null module name asks
    // for the executable's module, the class is registered on this thread, and
    // the message loop only reads messages Windows dispatches to this thread.
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: windows_sys::w!("IceBoxSessionEnd"),
        };
        if RegisterClassW(&class) == 0 {
            tracing::warn!(
                error = %std::io::Error::last_os_error(),
                "session-end window class registration failed"
            );
            return;
        }
        // A top-level (never shown) window is required: message-only windows do
        // not receive the broadcast session-end notifications.
        let hwnd = CreateWindowExW(
            0,
            windows_sys::w!("IceBoxSessionEnd"),
            windows_sys::w!("ice-box session end"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            tracing::warn!(
                error = %std::io::Error::last_os_error(),
                "session-end window creation failed"
            );
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // `wparam != 0` means the session is really ending. Restoring on the
        // query message instead would leave the proxy off if a shutdown is
        // cancelled after the app already agreed to it.
        WM_ENDSESSION => {
            if wparam != 0 {
                restore_before_session_end();
            }
            0
        }
        WM_QUERYENDSESSION => TRUE as LRESULT,
        WM_DESTROY => {
            // SAFETY: the message loop runs on this thread.
            unsafe { PostQuitMessage(0) };
            0
        }
        // SAFETY: forwarding unknown messages to the default handler is the
        // documented WNDPROC contract.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
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
            tracing::info!("restored system proxy before Windows session end");
            true
        }
        Ok(false) => false,
        Err(err) => {
            tracing::error!(
                error = %err,
                "system proxy restore failed at Windows session end"
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
    fn session_end_restores_once_and_clears_the_flag() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-session-end-{}",
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
                http_port: 1,
                socks_host: None,
                socks_port: None,
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
        assert_eq!(
            proxy.restore_calls.get(),
            1,
            "an already cleared backup must not restore again"
        );
        let _ = fs::remove_dir_all(dir);
    }
}
