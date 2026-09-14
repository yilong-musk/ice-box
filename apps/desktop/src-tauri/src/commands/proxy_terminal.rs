// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::proxy_terminal::{self, shell_proxy_host, validate_shell_proxy_host};

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
    // `mixed_listen` is unvalidated while `allow_lan` is on, so the host is
    // checked before it reaches any generated shell command.
    //
    // The settings fallback is deliberate: the port resolves while the proxy
    // service is stopped, so both the Home card and the tray keep offering the
    // session helpers — the command can be prepared before the service starts.
    let host = shell_proxy_host(raw_host);
    validate_shell_proxy_host(&host)?;
    Ok((host, port))
}

pub(crate) fn copy_proxy_terminal_command(state: &AppState) -> Result<(), AppError> {
    let (host, port) = mixed_proxy_endpoint(state)?;
    proxy_terminal::copy_shell_proxy_command(&host, port)
}

pub(crate) fn open_proxy_terminal_from_state(state: &AppState) -> Result<(), AppError> {
    let (host, port) = mixed_proxy_endpoint(state)?;
    proxy_terminal::open_proxy_terminal(&host, port)
}

/// Copy the session-only Mixed command (Home card and tray use this one path).
///
/// Blocking: the clipboard helper is an external process (pbcopy / clip /
/// wl-copy). Owns the command text so the UI never builds it itself.
#[tauri::command]
pub async fn copy_proxy_command(app: AppHandle) -> Result<(), AppError> {
    run_blocking("copy_proxy_command", move || {
        let state = app.state::<AppState>();
        copy_proxy_terminal_command(state.inner())
    })
    .await
}

/// Open the platform default terminal with Mixed proxy env for this session only.
///
/// Blocking: macOS waits on the one-time Automation consent prompt and every
/// platform spawns a process. A sync command would freeze the UI event loop.
#[tauri::command]
pub async fn open_proxy_terminal(app: AppHandle) -> Result<(), AppError> {
    run_blocking("open_proxy_terminal", move || {
        let state = app.state::<AppState>();
        open_proxy_terminal_from_state(state.inner())
    })
    .await
}
