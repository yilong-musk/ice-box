// SPDX-License-Identifier: GPL-3.0-or-later

//! Single-instance lock: second launch requests focus on the running window.

use ice_config::AppPaths;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

const FOCUS_FILE: &str = "instance.focus";
/// Retry budget for transient lock handoff (e.g. an exiting instance during an
/// in-app update restart). Kept short so a genuine second launch still focuses
/// the running window almost immediately.
const LOCK_RETRY_ATTEMPTS: usize = 5;
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(50);

pub enum InstanceLock {
    Primary(std::fs::File),
    /// Another instance holds the lock; focus was requested on it (unless the
    /// caller asked for a silent exit).
    Secondary,
}

/// Acquire the data-dir lock, or signal the primary instance to show its
/// window. `request_focus` is false for a login-item launch, which must leave
/// an already-running session alone instead of raising its window.
pub fn acquire_or_request_focus(
    paths: &AppPaths,
    request_focus: bool,
) -> Result<InstanceLock, String> {
    acquire_or_request_focus_with_retries(
        paths,
        request_focus,
        LOCK_RETRY_ATTEMPTS,
        LOCK_RETRY_DELAY,
    )
}

fn acquire_or_request_focus_with_retries(
    paths: &AppPaths,
    request_focus: bool,
    attempts: usize,
    delay: Duration,
) -> Result<InstanceLock, String> {
    use fs2::FileExt;

    paths
        .ensure_dirs()
        .map_err(|e| format!("ensure data dirs: {e}"))?;
    let lock_path = paths.root().join("instance.lock");
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| format!("open instance lock: {e}"))?;

    let attempts = attempts.max(1);
    let mut locked = false;
    for attempt in 0..attempts {
        if file.try_lock_exclusive().is_ok() {
            locked = true;
            break;
        }
        if attempt + 1 < attempts {
            std::thread::sleep(delay);
        }
    }
    if !locked {
        if request_focus {
            write_focus_request(paths.root());
        }
        return Ok(InstanceLock::Secondary);
    }

    file.set_len(0)
        .map_err(|e| format!("truncate instance lock: {e}"))?;
    let mut file = file;
    let _ = writeln!(file, "{}", std::process::id());
    Ok(InstanceLock::Primary(file))
}

fn write_focus_request(data_root: &Path) {
    let _ = fs::write(data_root.join(FOCUS_FILE), b"1");
}

/// Poll for secondary-instance focus requests for the app lifetime.
pub fn spawn_focus_watchdog<R: Runtime>(app: AppHandle<R>, paths: AppPaths) {
    let focus_path = paths.root().join(FOCUS_FILE);
    let workers = app.state::<crate::AppState>().workers.clone();
    workers.spawn("instance-focus", move |cancel| loop {
        if !cancel.wait(Duration::from_millis(400)) {
            break;
        }
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
                crate::round_window_corners(&win);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn write_focus_request_writes_marker_file() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-focus-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        write_focus_request(&dir);
        assert!(dir.join(FOCUS_FILE).is_file());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn secondary_instance_writes_focus_only_when_requested() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-lock-secondary-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = AppPaths::new(&dir);
        let primary =
            match acquire_or_request_focus_with_retries(&paths, true, 1, Duration::from_millis(1))
                .unwrap()
            {
                InstanceLock::Primary(file) => file,
                InstanceLock::Secondary => panic!("first instance must acquire lock"),
            };

        assert!(matches!(
            acquire_or_request_focus_with_retries(&paths, false, 1, Duration::from_millis(1))
                .unwrap(),
            InstanceLock::Secondary
        ));
        assert!(!dir.join(FOCUS_FILE).exists());

        assert!(matches!(
            acquire_or_request_focus_with_retries(&paths, true, 1, Duration::from_millis(1))
                .unwrap(),
            InstanceLock::Secondary
        ));
        assert!(dir.join(FOCUS_FILE).is_file());

        drop(primary);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn acquires_lock_when_exiting_primary_releases_during_retry_window() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-lock-handoff-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = AppPaths::new(&dir);
        let primary =
            match acquire_or_request_focus_with_retries(&paths, true, 1, Duration::from_millis(1))
                .unwrap()
            {
                InstanceLock::Primary(file) => file,
                InstanceLock::Secondary => panic!("first instance must acquire lock"),
            };

        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            drop(primary);
        });

        let outcome =
            acquire_or_request_focus_with_retries(&paths, true, 10, Duration::from_millis(15))
                .unwrap();
        releaser.join().unwrap();
        assert!(
            matches!(outcome, InstanceLock::Primary(_)),
            "restarting instance must acquire the lock once the exiting instance releases it"
        );
        assert!(!dir.join(FOCUS_FILE).exists());

        drop(outcome);
        let _ = fs::remove_dir_all(dir);
    }
}
