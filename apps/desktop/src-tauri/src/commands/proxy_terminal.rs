// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::proxy_terminal::{self, shell_proxy_host};

pub(crate) fn mixed_proxy_endpoint(state: &AppState) -> Result<(String, u16), AppError> {
    let settings = current_settings(&state.paths)?;
    let core = state.core_snapshot.load();
    let port = core
        .state
        .inbound_port
        .filter(|p| *p > 0)
        .unwrap_or(settings.mixed_port);
    if port == 0 {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            "mixed port is not configured",
        ));
    }
    let raw_host = core
        .state
        .inbound_host
        .as_deref()
        .filter(|h| !h.is_empty())
        .unwrap_or(settings.mixed_listen.as_str());
    Ok((shell_proxy_host(raw_host), port))
}

pub(crate) fn copy_proxy_terminal_command(state: &AppState) -> Result<(), AppError> {
    let (host, port) = mixed_proxy_endpoint(state)?;
    proxy_terminal::copy_shell_proxy_command(&host, port)
}

pub(crate) fn open_proxy_terminal_from_state(state: &AppState) -> Result<(), AppError> {
    let (host, port) = mixed_proxy_endpoint(state)?;
    proxy_terminal::open_proxy_terminal(&host, port)
}

/// Open the platform default terminal with Mixed proxy env for this session only.
#[tauri::command]
pub fn open_proxy_terminal(state: State<'_, AppState>) -> Result<(), AppError> {
    open_proxy_terminal_from_state(state.inner())
}
