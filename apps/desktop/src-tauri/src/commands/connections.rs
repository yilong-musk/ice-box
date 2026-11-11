// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[tauri::command]
pub async fn list_connections(app: AppHandle) -> Result<ConnectionsResponse, AppError> {
    run_blocking("list_connections", move || {
        let state = app.state::<AppState>();
        list_connections_use_case(state.inner())
    })
    .await
}

#[tauri::command]
pub async fn get_connection_stats(app: AppHandle) -> Result<ConnectionStats, AppError> {
    run_blocking("get_connection_stats", move || {
        let state = app.state::<AppState>();
        connection_stats_use_case(state.inner())
    })
    .await
}

#[tauri::command]
pub async fn close_connection(app: AppHandle, req: CloseConnectionRequest) -> Result<(), AppError> {
    run_blocking("close_connection", move || {
        let state = app.state::<AppState>();
        close_connection_use_case(state.inner(), req)
    })
    .await
}

#[tauri::command]
pub async fn close_all_connections(app: AppHandle) -> Result<(), AppError> {
    run_blocking("close_all_connections", move || {
        let state = app.state::<AppState>();
        close_all_connections_use_case(state.inner())
    })
    .await
}
