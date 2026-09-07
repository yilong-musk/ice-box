// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[tauri::command]
pub async fn get_status(app: AppHandle) -> Result<StatusResponse, AppError> {
    run_blocking("get_status", move || {
        let state = app.state::<AppState>();
        collect_status(&state)
    })
    .await
}

#[tauri::command]
pub async fn get_traffic_snapshot(app: AppHandle) -> Result<TrafficSnapshot, AppError> {
    run_blocking("get_traffic_snapshot", move || {
        let state = app.state::<AppState>();
        require_running_core(&state)?;
        Ok(state.traffic.snapshot())
    })
    .await
}

#[tauri::command]
pub async fn get_traffic_since(
    app: AppHandle,
    cursor: Option<u64>,
) -> Result<TrafficDelta, AppError> {
    run_blocking("get_traffic_since", move || {
        let state = app.state::<AppState>();
        require_running_core(&state)?;
        Ok(state.traffic.snapshot_since(cursor))
    })
    .await
}
