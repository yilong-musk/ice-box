// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

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
