// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Result<AppSettings, AppError> {
    run_blocking("get_settings", move || {
        let state = app.state::<AppState>();
        current_settings(&state.paths)
    })
    .await
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, patch: SettingsPatch) -> Result<(), AppError> {
    run_blocking("save_settings", move || {
        let state = app.state::<AppState>();
        save_settings_inner(&app, &state, patch)
    })
    .await
}

/// Tray language: every label is rewritten, and the update prompt follows
/// immediately instead of at the watchdog's next tick. Both mutate the native
/// menu, so the work runs off the main thread like the other rebuild paths.
#[tauri::command]
pub async fn set_tray_language(app: AppHandle, language: TrayLanguage) -> Result<(), AppError> {
    run_blocking("set_tray_language", move || {
        tray::set_language(&app, language)?;
        tray::sync_update_prompt(&app);
        Ok(())
    })
    .await
}

/// Tray update prompt: the window mirrors the version the sidebar arrow offers,
/// or `None` to drop the item. Off the main thread — the menu mutation hops to
/// the main thread, which a sync command would be blocking.
#[tauri::command]
pub async fn set_tray_update_available(
    app: AppHandle,
    version: Option<String>,
) -> Result<(), AppError> {
    run_blocking("set_tray_update_available", move || {
        tray::set_update_available(&app, version)
    })
    .await
}

/// Switch routing mode. With the pinned sing-box 1.13.19 the runtime Clash `mode-list` is
/// only `[<default_mode>]`, so a `PATCH /configs` to another mode is silently ignored and
/// the switch always takes the rebuild + reload/restart path (the PATCH attempt is a
/// forward-compatible capability gate). Settings are always persisted so the next apply
/// builds the new `default_mode`.
#[tauri::command]
pub async fn set_proxy_mode(app: AppHandle, req: SetProxyModeRequest) -> Result<(), AppError> {
    run_blocking("set_proxy_mode", move || {
        let state = app.state::<AppState>();
        set_proxy_mode_inner(&app, &state, req)
    })
    .await
}
