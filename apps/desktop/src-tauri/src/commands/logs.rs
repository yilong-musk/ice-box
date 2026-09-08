// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[derive(Deserialize)]
pub struct LogViewRequest {
    pub n: usize,
}

#[tauri::command]
pub async fn get_log_view(app: AppHandle, req: LogViewRequest) -> Result<Vec<String>, AppError> {
    run_blocking("get_log_view", move || {
        let state = app.state::<AppState>();
        // While TUN capture runs through the privileged helper (production
        // macOS path), the elevated core's output goes to the helper's fixed
        // log instead of the app-data core log; the dev sudo runner keeps the
        // app-data path, so it must not read the helper log. The capture
        // controller latches helper usage for the app session, so the merge
        // also persists after a TUN session ends.
        let helper_log = (state.capture.helper_core_used()
            && !ice_tun_sys::dev_sudo_runner_enabled())
        .then(|| std::path::Path::new(ice_tun_sys::install_paths::CORE_LOG_DEST));
        // Change detection: the view is polled every 2s; skip the read + parse
        // when no source file changed and the requested depth is unchanged.
        let sigs = vec![
            file_sig(&state.paths.app_log()),
            file_sig(&state.paths.core_log()),
            helper_log.and_then(file_sig),
        ];
        if let Ok(cache) = state.log_view_cache.lock() {
            if let Some(entry) = cache.as_ref() {
                if entry.n == req.n && entry.sigs == sigs {
                    return Ok(entry.lines.clone());
                }
            }
        }
        let lines = crate::log_view::read_log_view(
            &state.paths.app_log(),
            &state.paths.core_log(),
            helper_log,
            req.n,
        )?;
        if let Ok(mut cache) = state.log_view_cache.lock() {
            *cache = Some(LogViewCache {
                sigs,
                n: req.n,
                lines: lines.clone(),
            });
        }
        Ok(lines)
    })
    .await
}

#[tauri::command]
pub async fn clear_logs(app: AppHandle) -> Result<(), AppError> {
    run_blocking("clear_logs", move || {
        let state = app.state::<AppState>();
        ice_core::truncate_log_file(&state.paths.app_log(), ice_core::APP_LOG_KEEP).map_err(
            |e| AppError::new(ErrorCode::ConfigInvalid, format!("truncate app log: {e}")),
        )?;
        ice_core::truncate_log_file(&state.paths.core_log(), ice_core::CORE_LOG_KEEP).map_err(
            |e| AppError::new(ErrorCode::ConfigInvalid, format!("truncate core log: {e}")),
        )?;
        if state.capture.helper_core_used() && !ice_tun_sys::dev_sudo_runner_enabled() {
            let helper_log = std::path::Path::new(ice_tun_sys::install_paths::CORE_LOG_DEST);
            if let Err(err) = ice_core::truncate_log_file(helper_log, ice_core::CORE_LOG_KEEP) {
                tracing::warn!(error = %err, "could not truncate helper core log");
            }
        }
        if let Ok(mut cache) = state.log_view_cache.lock() {
            *cache = None;
        }
        if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
            slot.retain(|m| m.key != ErrorCode::LogsOversized.message_key());
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn get_runtime_config(app: AppHandle) -> Result<String, AppError> {
    run_blocking("get_runtime_config", move || {
        let state = app.state::<AppState>();
        let path = state.paths.config();
        if !path.exists() {
            return Ok(String::new());
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("read config: {e}")))?;
        redact_config_str(&raw).map_err(|e| {
            AppError::new(
                ErrorCode::ConfigInvalid,
                format!("redact runtime config: {e}"),
            )
        })
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
