// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::proxy_terminal::{self, shell_proxy_host};

/// Open the platform default terminal with Mixed proxy env for this session only.
#[tauri::command]
pub fn open_proxy_terminal(state: State<'_, AppState>) -> Result<(), AppError> {
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
            "open proxy terminal: mixed port is not configured",
        ));
    }
    let raw_host = core
        .state
        .inbound_host
        .as_deref()
        .filter(|h| !h.is_empty())
        .unwrap_or(settings.mixed_listen.as_str());
    let host = shell_proxy_host(raw_host);
    proxy_terminal::open_proxy_terminal(&host, port)
}
