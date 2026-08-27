mod acceptance;
mod commands;
mod core_watch;
mod instance;
mod log_tail;
mod log_view;
mod orchestrate;
mod shutdown;
mod tray;

use crate::shutdown::{request_tray_quit, QuitOutcome};
use ice_config::{init_logging, purge_invalid_pid_file, AppPaths};
use ice_core::{CoreController, CoreHandle};
use ice_proxy_sys::{create_system_proxy, recover_if_applied, ProxyEndpoints, SystemProxy};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use tauri::{Manager, RunEvent, WindowEvent};

/// Panic log path, resolved after `AppPaths` is available in `setup`. Panics are
/// written here so a crash on Windows (where the release binary has no console and
/// stderr is discarded) still leaves a trace in `ice-box.log`.
static PANIC_LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Route panics to stderr (if any) and append them to the app log file.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic payload".to_string()
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let line = format!("{payload} (at {location})");
        let _ = writeln!(std::io::stderr(), "ice-box PANIC: {line}");
        if let Some(path) = PANIC_LOG_PATH.get() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(file, "{} PANIC {line}", chrono::Utc::now().to_rfc3339());
            }
        }
    }));
}

pub struct AppState {
    pub paths: AppPaths,
    pub core: Mutex<Box<dyn CoreHandle>>,
    pub proxy: Mutex<Box<dyn SystemProxy>>,
    /// Serializes config mutations (subscriptions, settings, start/stop, node select).
    pub orchestrate: Mutex<()>,
    /// Shown in UI when startup proxy crash recovery failed.
    pub proxy_recovery_warning: Mutex<Option<String>>,
    /// Memoized `is_proxy_live_applied` result (endpoints, checked-at, value); avoids a
    /// `networksetup` subprocess storm from 2s status polling. Invalidated by the
    /// `start` command and on endpoints change (cache key).
    pub proxy_applied_cache: Mutex<Option<(ProxyEndpoints, Instant, bool)>>,
    /// Platform backend capability; set once at process start (`Noop` is false).
    pub system_proxy_available: bool,
    /// Set by quit so an in-flight auto-start healthcheck aborts instead of blocking ~5s.
    pub shutdown_requested: Arc<AtomicBool>,
    /// Held for app lifetime; releasing the file unlocks the data directory.
    _instance_lock: std::fs::File,
}

fn acquire_instance_lock(paths: &AppPaths) -> Result<std::fs::File, String> {
    match instance::acquire_or_request_focus(paths)? {
        instance::InstanceLock::Primary(file) => Ok(file),
        instance::InstanceLock::Secondary => std::process::exit(0),
    }
}

fn bootstrap_data_dir(
    paths: &AppPaths,
    shutdown_requested: Arc<AtomicBool>,
) -> Result<(Box<dyn CoreHandle>, Option<String>), String> {
    paths
        .ensure_dirs()
        .map_err(|e| format!("ensure data dirs: {e}"))?;

    if let Err(err) = init_logging(Some(&paths.app_log())) {
        eprintln!("ice-box: logging init: {err}");
    }

    if let Err(err) = purge_invalid_pid_file(&paths.pid()) {
        tracing::warn!(error = %err, "failed to purge invalid pid file");
    }

    let mut core = CoreController::new();
    core.set_health_cancel(shutdown_requested);
    if let Err(err) = core.reclaim_orphan_pid(&paths.pid()) {
        tracing::warn!(error = %err, "failed to reclaim orphan sing-box pid");
    }

    let proxy = create_system_proxy();
    let proxy_recovery_warning = match recover_if_applied(&paths.proxy_backup(), proxy.as_ref()) {
        Ok(true) => {
            tracing::info!("restored system proxy from previous session");
            None
        }
        Ok(false) => {
            tracing::debug!("no applied system proxy backup to restore");
            None
        }
        Err(err) => {
            tracing::error!(error = %err, "system proxy crash recovery failed");
            Some(format!("系统代理恢复失败: {err}"))
        }
    };

    Ok((Box::new(core), proxy_recovery_warning))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_panic_hook();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let root = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("resolve app_data_dir: {e}"))?;
            let paths = AppPaths::new(root);
            let _ = PANIC_LOG_PATH.set(paths.app_log());
            let instance_lock = acquire_instance_lock(&paths)?;
            let paths_for_focus = paths.clone();
            let shutdown_requested = Arc::new(AtomicBool::new(false));
            let (core, proxy_recovery_warning) =
                bootstrap_data_dir(&paths, shutdown_requested.clone())?;
            let proxy = create_system_proxy();
            let system_proxy_available = proxy.is_available();
            app.manage(AppState {
                paths,
                core: Mutex::new(core),
                proxy: Mutex::new(proxy),
                orchestrate: Mutex::new(()),
                proxy_recovery_warning: Mutex::new(proxy_recovery_warning),
                proxy_applied_cache: Mutex::new(None),
                system_proxy_available,
                shutdown_requested,
                _instance_lock: instance_lock,
            });
            instance::spawn_focus_watchdog(app.handle().clone(), paths_for_focus);
            tray::setup_tray(app.handle())?;
            core_watch::spawn_core_watchdog(app.handle().clone());
            // Product: opening the app starts the core only; system proxy is
            // toggled from the home page. Quit still restores an applied proxy.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let state = handle.state::<AppState>();
                if state.shutdown_requested.load(Ordering::SeqCst) {
                    return;
                }
                if let Err(err) = commands::start_core(&handle, &state) {
                    if state.shutdown_requested.load(Ordering::SeqCst) {
                        tracing::info!(error = %err, "auto-start aborted by quit");
                        return;
                    }
                    tracing::error!(error = %err, "auto-start core failed");
                    if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
                        *slot = Some(format!("内核自动启动失败（{err}）"));
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::list_subscriptions,
            commands::start,
            commands::stop_system_proxy,
            commands::stop,
            commands::get_log_view,
            commands::get_runtime_config,
            commands::reveal_data_dir,
            commands::get_settings,
            commands::save_settings,
            commands::set_proxy_mode,
            commands::add_subscription,
            commands::remove_subscription,
            commands::update_subscription,
            commands::update_all_subscriptions,
            commands::set_active_subscription,
            commands::apply_subscriptions,
            commands::list_nodes,
            commands::set_selected_node,
            commands::set_group_selection,
            commands::test_node_delay,
            commands::get_rule_overview,
            commands::list_rules,
            commands::set_rule_disabled,
            commands::add_custom_rule,
            commands::remove_custom_rule,
            commands::get_connection_stats,
            commands::get_traffic_sample,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            // Cmd+Q / OS logout / app.exit() path: run the same cleanup as tray Quit
            // (stop core + restore system proxy) before the process dies, instead of
            // orphaning sing-box and leaving the proxy applied until the next launch.
            api.prevent_exit();
            match request_tray_quit(app_handle) {
                QuitOutcome::Stopped => std::process::exit(0),
                QuitOutcome::LockPoisoned => std::process::exit(1),
                QuitOutcome::ProxyRestoreFailed | QuitOutcome::StopFailed => {
                    // Stay running so the user can retry from the UI; the warning is
                    // surfaced on the next get_status poll.
                }
            }
        }
    });
}

#[cfg(test)]
pub(crate) fn test_instance_lock(paths: &AppPaths) -> std::fs::File {
    use fs2::FileExt;
    use std::fs::OpenOptions;
    paths.ensure_dirs().expect("dirs");
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(paths.root().join("instance.lock"))
        .expect("lock file");
    file.try_lock_exclusive().expect("lock");
    file
}

#[cfg(test)]
mod tests {
    use super::*;
    use ice_config::write_json_atomic;
    use ice_proxy_sys::{ProxyBackup, ProxyBackupFile, ProxyEndpoints, ProxySysError, SystemProxy};
    use std::cell::Cell;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct CountingProxy {
        restore_calls: Cell<usize>,
        apply_calls: Cell<usize>,
    }

    impl SystemProxy for CountingProxy {
        fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
            Ok(ProxyBackup::default())
        }

        fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
            self.apply_calls.set(self.apply_calls.get() + 1);
            Ok(())
        }

        fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
            self.restore_calls.set(self.restore_calls.get() + 1);
            Ok(())
        }
    }

    #[test]
    fn bootstrap_purges_invalid_pid_and_recovers_applied_proxy() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-bootstrap-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let paths = AppPaths::new(&dir);
        paths.ensure_dirs().expect("dirs");
        fs::write(paths.pid(), b"not-a-pid").expect("pid");

        let record = ProxyBackupFile {
            applied: true,
            pending_apply: false,
            applied_at: None,
            endpoints: ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            },
            backup: ProxyBackup::default(),
        };
        write_json_atomic(&paths.proxy_backup(), &record).expect("backup");

        purge_invalid_pid_file(&paths.pid()).expect("purge");
        assert!(!paths.pid().exists());

        let proxy = CountingProxy::default();
        let did = recover_if_applied(&paths.proxy_backup(), &proxy).expect("recover");
        assert!(did);
        assert_eq!(proxy.restore_calls.get(), 1);
        assert_eq!(proxy.apply_calls.get(), 0);

        let after: ProxyBackupFile =
            serde_json::from_str(&fs::read_to_string(paths.proxy_backup()).unwrap()).unwrap();
        assert!(!after.applied);
        assert!(paths.proxy_backup().exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
