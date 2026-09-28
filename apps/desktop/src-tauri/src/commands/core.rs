// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// Home「启动代理服务」: ensure core is running, then take over the OS system proxy.
#[tauri::command]
pub async fn start(app: AppHandle) -> Result<(), AppError> {
    run_blocking("start", move || {
        let state = app.state::<AppState>();
        start_service(&app, &state)
    })
    .await
}

/// First-frame restore of the last proxy-service state. No-op when the last
/// session left capture off, or when capture is already active.
#[tauri::command]
pub async fn restore_launch_proxy(app: AppHandle) -> Result<(), AppError> {
    run_blocking("restore_launch_proxy", move || {
        let state = app.state::<AppState>();
        restore_proxy_service_on_launch(&app, &state)
    })
    .await
}

#[tauri::command]
pub async fn stop(app: AppHandle) -> Result<(), AppError> {
    run_blocking("stop", move || {
        let state = app.state::<AppState>();
        let binary = binary_for(&app)?;
        graceful_stop(&state, binary)
    })
    .await
}
