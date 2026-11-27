// SPDX-License-Identifier: GPL-3.0-or-later

mod app_host;
mod app_update;
mod commands;
#[cfg(target_os = "linux")]
mod session_end_unix;
mod shutdown;
mod tray;
#[cfg(target_os = "windows")]
mod tray_compat;
/// Tray delay test: runs inside the macOS native menu (`tray_delay_view`) and
/// the Windows custom tray panel (`tray_panel`). Its platform-neutral model
/// compiles on every host so panel serialization and host unit tests stay
/// covered.
mod tray_delay;
/// The delay test button's own view (macOS-only): the row handles its mouse
/// events itself, so a click on it leaves the menu open for the running test.
#[cfg(target_os = "macos")]
mod tray_delay_view;
mod tray_panel;
#[cfg(target_os = "macos")]
mod tray_speed;

#[cfg(target_os = "windows")]
pub use ice_runtime::session_end;
pub use ice_runtime::{
    application, autostart, capture, core_snapshot, core_watch, helper_install, instance, log_tail,
    log_view, mcp, orchestrate, proc_memory, proxy_terminal, rule_set_watch, runtime,
    runtime_status, subscription_watch, windows_elevation, workers,
};

use crate::app_host::{mcp_context, tauri_host, TauriHost};
use crate::orchestrate::current_settings;
use crate::shutdown::{request_tray_quit, QuitOutcome};
use ice_config::AppPaths;
use ice_runtime::{install_panic_hook, set_panic_log_path, HostCapabilities};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{Emitter, Manager, RunEvent, WindowEvent};

/// Desktop Tauri-managed runtime state handle.
pub type AppState = Arc<ice_runtime::AppState>;

/// Run the lightweight MCP stdio bridge used by Agent clients.
///
/// The bridge never owns application state; it connects to the MCP listener in
/// the running desktop process so the GUI and Agent share one data directory,
/// one core, and one OS capture session.
pub fn run_mcp_stdio_bridge(args: impl IntoIterator<Item = std::ffi::OsString>) -> i32 {
    ice_runtime::run_mcp_stdio_bridge(args)
}

fn acquire_instance_lock(paths: &AppPaths, request_focus: bool) -> Result<std::fs::File, String> {
    match instance::acquire_or_request_focus(paths, request_focus)? {
        instance::InstanceLock::Primary(file) => Ok(file),
        instance::InstanceLock::Secondary => std::process::exit(0),
    }
}

/// Show the main window on a user launch.
///
/// `tauri.conf.json` creates the window hidden so a login-item start
/// ([`autostart::AUTOSTART_FLAG`]) cannot flash a window at the desktop before
/// the shell decides to stay in the tray.
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        round_window_corners(&window);
    }
}

/// Draw the bundled artwork in the Dock with no system plate behind it.
#[cfg(target_os = "macos")]
fn apply_plain_dock_icon() {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    const ICON: &[u8] = include_bytes!("../icons/icon.icns");

    let Some(mtm) = MainThreadMarker::new() else {
        tracing::warn!("dock icon: skipped, not on the main thread");
        return;
    };
    let data = NSData::with_bytes(ICON);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        tracing::warn!("dock icon: skipped, the bundled icns did not decode");
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: always passes `Some`, never `None` (the setter might not allow
    // `None`), and `mtm` proves the main-thread affinity AppKit requires.
    unsafe { app.setApplicationIconImage(Some(&image)) };
}

/// Give the window the system's rounded corners (Apple-style on every desktop).
pub(crate) fn round_window_corners<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    #[cfg(windows)]
    {
        let scheduled = window.clone();
        let _ = window.run_on_main_thread(move || apply_round_window_corners(&scheduled));
    }
    #[cfg(not(windows))]
    {
        let _ = window;
    }
}

/// Ask DWM for rounded corners. The caller is on the UI thread.
#[cfg(windows)]
fn apply_round_window_corners<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    };

    let Ok(handle) = window.hwnd() else {
        return;
    };
    let hwnd = handle.0;
    if hwnd.is_null() {
        return;
    }
    let preference = DWMWCP_ROUND;
    // SAFETY: `hwnd` is this webview's live top-level window.
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &preference as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&preference) as u32,
        )
    };
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_panic_hook();
    let login_item_launch = autostart::is_autostart_launch();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(move |app| {
            let root = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("resolve app_data_dir: {e}"))?;
            let paths = AppPaths::new(root);
            set_panic_log_path(paths.app_log());
            let instance_lock = acquire_instance_lock(&paths, !login_item_launch)?;
            #[cfg(target_os = "windows")]
            session_end::install(paths.proxy_backup());
            #[cfg(target_os = "linux")]
            session_end_unix::install(app.handle().clone(), paths.proxy_backup());
            if !login_item_launch {
                show_main_window(app.handle());
            }
            let paths_for_focus = paths.clone();
            let resource_dir = app.path().resource_dir().ok();
            let capabilities = HostCapabilities::desktop(app_update::in_app_update_supported());
            let runtime_state = ice_runtime::AppState::bootstrap(
                paths,
                instance_lock,
                resource_dir.clone(),
                capabilities,
            )?;
            app.manage(Arc::clone(&runtime_state));
            {
                let handle = app.handle().clone();
                runtime_state.core_snapshot.bind_emitter({
                    let handle = handle.clone();
                    move |snapshot| {
                        let _ = handle.emit(core_snapshot::CORE_STATUS_CHANGED, snapshot);
                    }
                });
                // Establish a committed read before startup takes the write lock.
                let _ = application::collect_status(&runtime_state);
                runtime_state.traffic.set_on_sample({
                    let handle = handle.clone();
                    move |sample| {
                        let _ = handle.emit(core_snapshot::TRAFFIC_SAMPLE, sample);
                    }
                });
            }
            let mcp_paths = runtime_state.paths.root().to_path_buf();
            app.manage(mcp::McpService::new(mcp_paths));
            {
                let warmup_paths = runtime_state.paths.clone();
                let warmup_resource = resource_dir.clone();
                std::thread::spawn(move || {
                    let _ = orchestrate::ensure_geoip_rule_sets(
                        &warmup_paths,
                        warmup_resource.as_deref(),
                    );
                    let _ = helper_install::helper_core_stale(warmup_resource.as_deref());
                });
            }
            let startup_host = tauri_host(app.handle());
            let startup_state = Arc::clone(&runtime_state);
            let (orch_tx, orch_rx) = std::sync::mpsc::sync_channel(1);
            std::thread::spawn(move || {
                if let Err(err) =
                    application::auto_start_on_launch(&startup_host, &startup_state, Some(orch_tx))
                {
                    if startup_state.shutdown_requested.load(Ordering::SeqCst) {
                        tracing::info!(error = %err, "auto-start aborted by quit");
                        return;
                    }
                    tracing::error!(error = %err, "auto-start failed");
                    application::append_recovery_warning(
                        &startup_state,
                        ice_config::UiMessage::new("recover.autoStartFailed")
                            .with("detail", err.to_string()),
                    );
                }
            });
            if orch_rx.recv().is_err() {
                tracing::warn!("startup worker exited before acquiring orchestrate lock");
            }
            if let Some(ctx) = mcp_context(app.handle()) {
                app.state::<mcp::McpService>().start_if_enabled(&ctx);
            }
            let login_item = current_settings(&runtime_state.paths)
                .map(|settings| settings.launch_at_login)
                .unwrap_or(false);
            std::thread::spawn(move || {
                if let Err(err) = autostart::set_enabled(login_item) {
                    if login_item {
                        tracing::warn!(error = %err, "failed to refresh the login item");
                    } else {
                        tracing::debug!(error = %err, "no login item to clear");
                    }
                }
            });
            {
                let focus_handle = app.handle().clone();
                instance::spawn_focus_watchdog(
                    runtime_state.workers.clone(),
                    paths_for_focus,
                    move || {
                        let app_for_ui = focus_handle.clone();
                        let _ = focus_handle.run_on_main_thread(move || {
                            if let Some(win) = app_for_ui.get_webview_window("main") {
                                let _ = win.unminimize();
                                let _ = win.show();
                                let _ = win.set_focus();
                                round_window_corners(&win);
                            }
                        });
                    },
                );
            }
            let tray_language = current_settings(&runtime_state.paths)
                .map(|settings| tray::TrayLanguage::from(settings.language))
                .unwrap_or(tray::TrayLanguage::En);
            tray::setup_tray(app.handle(), tray_language)?;
            #[cfg(target_os = "macos")]
            tray_speed::spawn_watchdog(app.handle().clone());
            tray::spawn_state_watchdog(app.handle().clone());
            core_watch::spawn_core_watchdog(Arc::clone(&runtime_state));
            {
                let probe_handle = app.handle().clone();
                runtime_status::spawn_probe_watchdog(
                    Arc::clone(&runtime_state),
                    Arc::new(move || {
                        let _ = probe_handle.emit(core_snapshot::APP_STATE_CHANGED, ());
                    }),
                );
            }
            let shared_host = Arc::new(TauriHost(app.handle().clone()));
            subscription_watch::spawn_subscription_watchdog(
                Arc::clone(&runtime_state),
                Arc::clone(&shared_host),
            );
            rule_set_watch::spawn_rule_set_watchdog(Arc::clone(&runtime_state), shared_host);
            Ok(())
        })
        .on_window_event(|window, event| {
            // The custom tray panel handles its own hide-on-blur / close
            // lifecycle and must not emit main-window visibility events.
            if window.label() == tray_panel::PANEL_LABEL {
                return;
            }
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let _ = window.hide();
                    let _ = window.emit(core_snapshot::WINDOW_HIDDEN, ());
                }
                WindowEvent::Focused(focused) => {
                    if *focused {
                        if let Some(state) = window.app_handle().try_state::<AppState>() {
                            application::request_runtime_probe_refresh(state.inner());
                        }
                        let _ = window.emit(core_snapshot::WINDOW_SHOWN, ());
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::set_home_active,
            commands::list_subscriptions,
            commands::start,
            commands::restore_launch_proxy,
            commands::stop_system_proxy,
            commands::stop,
            commands::recover_tun,
            commands::install_helper,
            commands::uninstall_helper,
            commands::ensure_tun_elevation,
            commands::remove_tun_elevation,
            commands::list_connections,
            commands::get_connection_stats,
            commands::close_connection,
            commands::close_all_connections,
            commands::get_runtime_config,
            commands::get_mcp_status,
            commands::set_mcp_enabled,
            commands::restart_mcp,
            commands::reveal_data_dir,
            commands::copy_proxy_command,
            commands::open_proxy_terminal,
            commands::get_settings,
            commands::save_settings,
            commands::set_tray_language,
            commands::set_tray_update_available,
            commands::set_proxy_mode,
            commands::add_subscription,
            commands::import_subscription_file,
            commands::remove_subscription,
            commands::update_subscription,
            commands::set_active_subscription,
            commands::set_auto_update_subscription,
            commands::subscription_share,
            commands::copy_subscription_share,
            commands::export_subscription_singbox,
            commands::list_nodes,
            commands::set_selected_node,
            commands::set_node_favorite,
            commands::set_group_selection,
            commands::test_node_delay,
            commands::get_rule_overview,
            commands::get_rule_set_status,
            commands::refresh_rule_sets,
            commands::release_rule_keyword_cache,
            commands::list_rules,
            commands::set_rule_disabled,
            commands::add_custom_rule,
            commands::update_custom_rule,
            commands::remove_custom_rule,
            commands::get_traffic_snapshot,
            commands::get_traffic_since,
            app_update::check_app_update,
            app_update::record_update_prompt,
            app_update::record_app_update_check,
            app_update::skip_app_update,
            app_update::install_app_update,
            app_update::open_app_download,
            tray_panel::tray_panel_state,
            tray_panel::tray_panel_placement,
            tray_panel::tray_panel_layout,
            tray_panel::tray_panel_hide,
            tray_panel::tray_panel_action,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        #[cfg(target_os = "macos")]
        if matches!(event, RunEvent::Ready) {
            apply_plain_dock_icon();
        }
        if let RunEvent::ExitRequested { api, .. } = event {
            tray_panel::hide_panel(app_handle);
            if crate::app_update::update_installing() {
                if let Some(mcp) = app_handle.try_state::<mcp::McpService>() {
                    mcp.shutdown();
                }
                if let Some(state) = app_handle.try_state::<AppState>() {
                    state
                        .workers
                        .shutdown(std::time::Duration::from_millis(500));
                    state.release_instance_lock();
                }
                return;
            }
            api.prevent_exit();
            let handle = app_handle.clone();
            tauri::async_runtime::spawn_blocking(move || match request_tray_quit(&handle) {
                QuitOutcome::Stopped => std::process::exit(0),
                QuitOutcome::LockPoisoned => std::process::exit(1),
                QuitOutcome::ProxyRestoreFailed | QuitOutcome::StopFailed => {}
            });
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn error_codes_ts_matches_rust() {
        let ts = include_str!("../../src/api/errorCodes.ts");
        let rust: std::collections::HashSet<&str> = ice_config::ErrorCode::ALL
            .iter()
            .map(|code| code.as_str())
            .collect();
        let mut from_ts = std::collections::HashSet::new();
        for line in ts.lines() {
            let trimmed = line.trim().trim_end_matches(',');
            if let Some(code) = trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                if code.contains('.') {
                    from_ts.insert(code);
                }
            }
        }
        assert_eq!(
            rust, from_ts,
            "ErrorCode::ALL and apps/desktop/src/api/errorCodes.ts ERROR_CODES must be the same set"
        );
    }
}
