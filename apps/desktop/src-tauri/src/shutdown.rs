// SPDX-License-Identifier: GPL-3.0-or-later

//! Desktop tray / window quit adapter over `ice_runtime::shutdown`.

pub use ice_runtime::shutdown::{
    clear_shutdown_request, graceful_stop, graceful_stop_for_update, request_headless_quit,
    QuitOutcome,
};

use crate::app_host::tauri_host;
use crate::AppState;
use tauri::{AppHandle, Manager, Runtime};

/// Tray Quit: serialize with orchestrate, stop core, exit only when proxy
/// state is consistent. Re-shows the window on failure so the user can retry.
pub fn request_tray_quit<R: Runtime>(app: &AppHandle<R>) -> QuitOutcome {
    let Some(state) = app.try_state::<AppState>() else {
        if let Some(mcp) = app.try_state::<crate::mcp::McpService>() {
            mcp.shutdown();
        }
        return QuitOutcome::Stopped;
    };
    let mcp = app.try_state::<crate::mcp::McpService>();
    let host = tauri_host(app);
    let outcome = request_headless_quit(state.inner(), &host, mcp.as_deref());
    if matches!(
        outcome,
        QuitOutcome::ProxyRestoreFailed | QuitOutcome::StopFailed
    ) {
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.show();
            let _ = win.set_focus();
            crate::round_window_corners(&win);
        }
    }
    outcome
}
