// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[tauri::command]
pub async fn get_log_view(app: AppHandle, req: LogViewRequest) -> Result<Vec<String>, AppError> {
    run_blocking("get_log_view", move || {
        let state = app.state::<AppState>();
        get_log_view_use_case(state.inner(), req)
    })
    .await
}

#[tauri::command]
pub async fn get_runtime_config(app: AppHandle) -> Result<String, AppError> {
    run_blocking("get_runtime_config", move || {
        let state = app.state::<AppState>();
        get_runtime_config_use_case(state.inner())
    })
    .await
}

#[tauri::command]
pub fn reveal_data_dir(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(state.paths.root().to_string_lossy(), None::<&str>)
        .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("reveal data dir: {e}")))?;
    Ok(())
}
