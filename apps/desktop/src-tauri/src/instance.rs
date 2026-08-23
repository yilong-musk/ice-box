//! Single-instance lock: second launch requests focus on the running window.

use ice_config::AppPaths;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

const FOCUS_FILE: &str = "instance.focus";

pub enum InstanceLock {
    Primary(std::fs::File),
    /// Another instance holds the lock; focus was requested on it.
    Secondary,
}

/// Acquire the data-dir lock, or signal the primary instance to show its window.
pub fn acquire_or_request_focus(paths: &AppPaths) -> Result<InstanceLock, String> {
    use fs2::FileExt;

    paths
        .ensure_dirs()
        .map_err(|e| format!("ensure data dirs: {e}"))?;
    let lock_path = paths.root().join("instance.lock");
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&lock_path)
        .map_err(|e| format!("open instance lock: {e}"))?;

    if file.try_lock_exclusive().is_err() {
        request_focus(paths.root());
        return Ok(InstanceLock::Secondary);
    }

    file.set_len(0)
        .map_err(|e| format!("truncate instance lock: {e}"))?;
    let mut file = file;
    let _ = writeln!(file, "{}", std::process::id());
    Ok(InstanceLock::Primary(file))
}

fn request_focus(data_root: &Path) {
    let _ = fs::write(data_root.join(FOCUS_FILE), b"1");
}

/// Poll for secondary-instance focus requests for the app lifetime.
pub fn spawn_focus_watchdog<R: Runtime>(app: AppHandle<R>, paths: AppPaths) {
    let focus_path = paths.root().join(FOCUS_FILE);
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(400));
        if !focus_path.exists() {
            continue;
        }
        let _ = fs::remove_file(&focus_path);
        let app_for_ui = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(win) = app_for_ui.get_webview_window("main") {
                let _ = win.unminimize();
                let _ = win.show();
                let _ = win.set_focus();
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn request_focus_writes_marker_file() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-focus-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        request_focus(&dir);
        assert!(dir.join(FOCUS_FILE).is_file());
        let _ = fs::remove_dir_all(&dir);
    }
}
