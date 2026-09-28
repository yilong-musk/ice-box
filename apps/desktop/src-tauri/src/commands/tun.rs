// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// Home「重试恢复」: on-demand retry of TUN recovery (`docs/tun.md`). Runs
/// the journal recovery driver under the orchestration lock; never enables
/// capture. Returns a warning message when cleanup is still uncertain.
#[tauri::command]
pub async fn recover_tun(app: AppHandle) -> Result<Vec<ice_config::UiMessage>, AppError> {
    run_blocking("recover_tun", move || {
        let state = app.state::<AppState>();
        let _orch = lock_orchestrate(&state)?;
        state.capture.refresh_backend()?;
        let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
        let warning = state.capture.recover(&mut **core)?;
        replace_recovery_warnings(&state, warning.clone());
        Ok(warning)
    })
    .await
}

#[tauri::command]
pub async fn stop_system_proxy(app: AppHandle) -> Result<(), AppError> {
    run_blocking("stop_system_proxy", move || {
        let state = app.state::<AppState>();
        disable_active_backend_inner(&app, &state)
    })
    .await
}

/// Install and authorize the trusted helper component (TUN elevation path):
/// prompts with the system authorization dialog and runs the bundled
/// `ice-helper install` as root. macOS only; cancelling modifies nothing.
/// On success the backend is refreshed so the fresh helper is usable at once.
#[tauri::command]
pub async fn install_helper(app: AppHandle) -> Result<(), AppError> {
    run_blocking("install_helper", move || {
        let result = crate::helper_install::install_helper_inner(&app);
        let state = app.state::<AppState>();
        reset_helper_probe_cache(&state);
        result
    })
    .await
}

/// Uninstall the trusted helper component: prompts with the system
/// authorization dialog, runs `ice-helper uninstall` as root, and refreshes
/// the backend (back to fail-closed).
#[tauri::command]
pub async fn uninstall_helper(app: AppHandle) -> Result<(), AppError> {
    run_blocking("uninstall_helper", move || {
        let result = crate::helper_install::uninstall_helper_inner(&app);
        let state = app.state::<AppState>();
        reset_helper_probe_cache(&state);
        result
    })
    .await
}

/// One-time Windows TUN elevation setup (plan B): installs the scheduled task
/// that runs the TUN core elevated. The only elevation the user ever sees —
/// a single UAC prompt to create the task; afterwards TUN transitions
/// (`schtasks /Run` / `/End`) work from an unelevated app without prompts.
/// No-op when the task already exists; always `Ok` on non-Windows hosts.
#[tauri::command]
pub async fn ensure_tun_elevation(app: AppHandle) -> Result<(), AppError> {
    run_blocking("ensure_tun_elevation", move || {
        let state = app.state::<AppState>();
        ensure_tun_elevation_inner(&app, &state)
    })
    .await
}

/// Remove the Windows TUN scheduled task (one UAC prompt). No-op when the
/// task does not exist; always `Ok` on non-Windows hosts.
#[tauri::command]
pub async fn remove_tun_elevation(app: AppHandle) -> Result<(), AppError> {
    run_blocking("remove_tun_elevation", move || {
        let state = app.state::<AppState>();
        remove_tun_elevation_inner(&app, &state)
    })
    .await
}
