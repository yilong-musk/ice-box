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
        let debug = current_settings(&state.paths)
            .map(|s| s.log_debug)
            .unwrap_or(false);
        // Change detection: the view is polled every 2s; skip the read + parse
        // when no source file changed and the requested depth / debug flag
        // are unchanged. Settings is in the sigs so flipping debug invalidates.
        let sigs = vec![
            file_sig(&state.paths.app_log()),
            file_sig(&state.paths.core_log()),
            helper_log.and_then(file_sig),
            file_sig(&state.paths.settings()),
        ];
        if let Ok(cache) = state.log_view_cache.lock() {
            if let Some(entry) = cache.as_ref() {
                if entry.n == req.n && entry.debug == debug && entry.sigs == sigs {
                    return Ok(entry.lines.clone());
                }
            }
        }
        let lines = crate::log_view::read_log_view(
            &state.paths.app_log(),
            &state.paths.core_log(),
            helper_log,
            req.n,
            debug,
        )?;
        if let Ok(mut cache) = state.log_view_cache.lock() {
            *cache = Some(LogViewCache {
                sigs,
                n: req.n,
                debug,
                lines: lines.clone(),
            });
        }
        Ok(lines)
    })
    .await
}

/// Cap core/helper logs at 20 MiB by dropping the oldest 5 MiB of the same
/// inode. Used by the watchdog so a long-running sing-box stays inside the
/// cap. The desktop process owns the app-log writer and caps it itself.
pub(crate) fn cap_oversized_logs(state: &AppState) {
    let mut failed = false;
    if ice_core::cap_log_file(
        &state.paths.core_log(),
        ice_core::CORE_LOG_MAX_BYTES,
        ice_core::CORE_LOG_KEEP,
    )
    .is_err()
    {
        failed = true;
    }
    if state.capture.helper_core_used() && !ice_tun_sys::dev_sudo_runner_enabled() {
        let helper_log = std::path::Path::new(ice_tun_sys::install_paths::CORE_LOG_DEST);
        match ice_core::cap_log_file(
            helper_log,
            ice_core::CORE_LOG_MAX_BYTES,
            ice_core::CORE_LOG_KEEP,
        ) {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                // Last resort: the helper's TruncateCoreLog empties the file.
                if ice_core::log_file_oversized(helper_log, ice_core::CORE_LOG_MAX_BYTES)
                    && truncate_helper_core_log(state).is_err()
                {
                    failed = true;
                }
            }
            Err(_) => failed = true,
        }
    }
    if failed {
        push_oversized_warning(state);
    } else {
        clear_oversized_warning(state);
    }
}

fn push_oversized_warning(state: &AppState) {
    let Ok(mut slot) = state.proxy_recovery_warning.lock() else {
        return;
    };
    if slot
        .iter()
        .any(|m| m.key == ErrorCode::LogsOversized.message_key())
    {
        return;
    }
    slot.push(ErrorCode::LogsOversized.ui_message());
}

fn clear_oversized_warning(state: &AppState) {
    if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
        slot.retain(|m| m.key != ErrorCode::LogsOversized.message_key());
    }
}

fn map_truncate_err(label: &str, err: std::io::Error) -> AppError {
    AppError::new(ErrorCode::LogsOversized, format!("truncate {label}: {err}"))
}

/// `/var/log/ice-box-core.log` is created by the privileged helper. Trim as
/// the user first (install/start now chown it to the authorized uid); if that
/// is denied, ask the helper to empty the same inode.
#[cfg_attr(not(unix), allow(unused_variables))]
fn truncate_helper_core_log(state: &AppState) -> Result<(), AppError> {
    let helper_log = std::path::Path::new(ice_tun_sys::install_paths::CORE_LOG_DEST);
    match ice_core::truncate_log_file(helper_log, ice_core::CORE_LOG_KEEP) {
        Ok(()) => return Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {}
        Err(err) => return Err(map_truncate_err("helper core log", err)),
    }
    #[cfg(unix)]
    {
        let helper = ice_tun_sys::helper::HelperCoreCoordinator::from_data_dir(state.paths.root())
            .map_err(AppError::from)?;
        helper
            .truncate_core_log()
            .map_err(|e| AppError::new(ErrorCode::LogsOversized, e.to_string()))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        Err(map_truncate_err(
            "helper core log",
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        ))
    }
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
