mod acceptance;
mod capture;
mod commands;
mod core_watch;
mod helper_install;
mod instance;
mod log_tail;
mod log_view;
mod orchestrate;
mod shutdown;
mod subscription_watch;
mod tray;
mod windows_elevation;

use crate::capture::CaptureController;
use crate::orchestrate::current_settings;
use crate::shutdown::{request_tray_quit, QuitOutcome};
use ice_config::{init_logging, purge_invalid_pid_file, AppPaths};
use ice_core::{CoreController, CoreHandle, TrafficMonitor};
use ice_proxy_sys::{
    create_system_proxy, is_proxy_applied_on_disk, recover_if_applied, ProxyEndpoints, SystemProxy,
};
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
    /// Persistent Clash `/traffic` stream; survives home-page unmounts.
    pub traffic: TrafficMonitor,
    /// TUN capture runtime controller (plan §4.3): owns the active backend,
    /// the capture state machine, and the recovery journal.
    pub capture: CaptureController,
    /// mtime-keyed cache of the parsed active profile (+ rule fingerprints);
    /// read paths poll every 2-5s and must not re-parse a multi-MB profile
    /// each time. Invalidated implicitly: the key changes when the active
    /// subscription, its profile, or `auto_default_rules` changes on disk.
    pub profile_cache: Mutex<Option<commands::ProfileCacheEntry>>,
    /// Change-detected merged log view: re-read only when a source file's
    /// size/mtime (or the requested line count) changes.
    pub log_view_cache: Mutex<Option<commands::LogViewCache>>,
    /// Memoized helper-daemon reachability probe (TTL'd, invalidated by
    /// install/uninstall); avoids a socket roundtrip on every status poll.
    pub helper_probe_cache: Mutex<Option<(Instant, bool)>>,
    /// Whether the running core supports live mode switches via the Clash API
    /// (`PATCH /configs`). The pinned sing-box never honors it; the probe is
    /// attempted once and the failure is remembered so later mode switches
    /// skip the two wasted HTTP roundtrips (forward-compatible: a core that
    /// honors the PATCH keeps the fast path).
    pub clash_live_mode_cache: Mutex<bool>,
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
    // The pid file can be missing while a previous session's core is still
    // running (the app was killed mid-teardown after the pid record was
    // cleared); scan the process table for sing-box processes running this
    // installation's config and reclaim the user-owned ones, so the auto-start
    // never hits `bind: address already in use` with no way to recover.
    let reclaimed = ice_core::reclaim_orphan_cores_with_config(&paths.config());
    if reclaimed > 0 {
        tracing::warn!(
            reclaimed,
            "reclaimed orphan sing-box cores without a pid file"
        );
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
            Some(format!("system proxy recovery failed: {err}"))
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
            let capture = CaptureController::new(paths.clone(), app.path().resource_dir().ok());
            // Fail-closed exclusivity: when startup proxy recovery failed, the
            // OS proxy is still applied and the app still owns it. Keep the
            // capture controller consistent with disk so TUN activation stays
            // rejected until the proxy is restored.
            if is_proxy_applied_on_disk(&paths.proxy_backup()) {
                tracing::warn!(
                    "system proxy backup still records applied after startup recovery; capture controller treats system proxy as the active backend"
                );
                let _ = capture.set_system_proxy_active();
            }
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
                traffic: TrafficMonitor::new(),
                capture,
                profile_cache: Mutex::new(None),
                log_view_cache: Mutex::new(None),
                helper_probe_cache: Mutex::new(None),
                clash_live_mode_cache: Mutex::new(true),
            });
            // Startup TUN recovery: inside the orchestration lock, after the
            // orphan-core reclamation in bootstrap. Never enables capture. A
            // recovery error (e.g. an unreadable journal) is fail-closed: it
            // is surfaced as a warning and TUN activation stays rejected
            // until an explicit retry succeeds.
            {
                let state = app.state::<AppState>();
                let _orch = state.orchestrate.lock().ok();
                let mut core = state.core.lock().ok();
                if let Some(core) = core.as_deref_mut() {
                    let append_warning = |warning: String| {
                        if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
                            let existing = slot.take().unwrap_or_default();
                            *slot = Some(if existing.is_empty() {
                                warning
                            } else {
                                format!("{existing}；{warning}")
                            });
                        }
                    };
                    // A leftover root-owned core from a previous session (the
                    // unprivileged bootstrap could not signal it) still holds
                    // the ports; reclaim it through the elevated coordinator
                    // before journal recovery so a later start / re-enable
                    // never hits `bind: address already in use`.
                    if let Err(err) = state
                        .capture
                        .reclaim_orphan_elevated_core(&mut **core)
                    {
                        tracing::warn!(error = %err, "failed to reclaim orphaned elevated core");
                        append_warning(format!("残留内核清理未确认 ({err})"));
                    }
                    let recovery = state.capture.recover(&mut **core);
                    match recovery {
                        Ok(Some(warning)) => append_warning(warning),
                        Ok(None) => {}
                        Err(err) => {
                            tracing::error!(error = %err, "startup tun recovery failed");
                            append_warning(format!("TUN state recovery unconfirmed ({err})"));
                        }
                    }
                }
            }
            instance::spawn_focus_watchdog(app.handle().clone(), paths_for_focus);
            let tray_language = {
                let state = app.state::<AppState>();
                current_settings(&state.paths)
                    .map(|settings| tray::TrayLanguage::from(settings.language))
                    .unwrap_or(tray::TrayLanguage::En)
            };
            tray::setup_tray(app.handle(), tray_language)?;
            core_watch::spawn_core_watchdog(app.handle().clone());
            subscription_watch::spawn_subscription_watchdog(app.handle().clone());
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
                        *slot = Some(format!("core auto-start failed ({err})"));
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
            commands::recover_tun,
            commands::install_helper,
            commands::uninstall_helper,
            commands::relaunch_elevated_for_tun,
            commands::get_log_view,
            commands::get_runtime_config,
            commands::reveal_data_dir,
            commands::get_settings,
            commands::save_settings,
            commands::set_tray_language,
            commands::set_proxy_mode,
            commands::add_subscription,
            commands::remove_subscription,
            commands::update_subscription,
            commands::update_all_subscriptions,
            commands::set_active_subscription,
            commands::set_auto_update_subscription,
            commands::list_nodes,
            commands::set_selected_node,
            commands::set_group_selection,
            commands::test_node_delay,
            commands::get_rule_overview,
            commands::list_rules,
            commands::set_rule_disabled,
            commands::add_custom_rule,
            commands::remove_custom_rule,
            commands::get_traffic_snapshot,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            // Cmd+Q / OS logout / app.exit() path: run the same cleanup as tray
            // Quit (stop core + restore system proxy) before the process dies,
            // instead of orphaning sing-box and leaving the proxy applied until
            // the next launch. The stop runs off the main thread — it can take
            // seconds with TUN active (teardown waits + core stop +
            // `networksetup` restore) — and the process exits from the worker
            // once the state is consistent.
            api.prevent_exit();
            let handle = app_handle.clone();
            tauri::async_runtime::spawn_blocking(move || {
                match request_tray_quit(&handle) {
                    QuitOutcome::Stopped => std::process::exit(0),
                    QuitOutcome::LockPoisoned => std::process::exit(1),
                    QuitOutcome::ProxyRestoreFailed | QuitOutcome::StopFailed => {
                        // Stay running so the user can retry from the UI; the
                        // warning is surfaced on the next get_status poll.
                    }
                }
            });
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

/// A free loopback port for tests that bind real listeners.
///
/// Ports come from a suite-wide counter in a fixed range below the OS
/// ephemeral range (Linux default 32768+, macOS / Windows 49152+), so a
/// concurrent test's `bind("127.0.0.1:0")` (e.g. `MockClashApi`) can never
/// land on one. The counter is serialized by a global mutex, so parallel
/// tests never pick the same port. Each candidate is probed and skipped when
/// an external process already holds it (e.g. a dev instance of the app on
/// the fixed defaults 17890 / 19150), so the suite never depends on host
/// state. The gap between picking a port and actually binding it remains
/// racy against *external* holders only — nothing inside the suite can take
/// a counter port.
#[cfg(test)]
pub(crate) fn free_loopback_port() -> u16 {
    const RANGE_START: u32 = 20_000;
    const RANGE_END: u32 = 23_000;
    static NEXT: std::sync::Mutex<u32> = std::sync::Mutex::new(RANGE_START);
    let mut next = NEXT.lock().expect("port counter lock");
    for _ in RANGE_START..RANGE_END {
        let port = *next as u16;
        *next = if *next + 1 >= RANGE_END {
            RANGE_START
        } else {
            *next + 1
        };
        if !ice_core::tcp_port_is_in_use("127.0.0.1", port) {
            return port;
        }
    }
    panic!("no free loopback port in {RANGE_START}..{RANGE_END}");
}

/// Default settings with random free loopback ports for the mixed and clash
/// API inbounds. Tests that start a (mock) core run the real port probe, so
/// the fixed defaults would fail whenever another program holds them.
#[cfg(test)]
pub(crate) fn test_settings() -> ice_config::AppSettings {
    ice_config::AppSettings {
        mixed_port: free_loopback_port(),
        clash_api_port: free_loopback_port(),
        ..ice_config::AppSettings::default()
    }
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
