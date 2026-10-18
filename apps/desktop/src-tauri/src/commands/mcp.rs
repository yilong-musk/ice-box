// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::mcp::{McpService, McpStatus};

#[tauri::command]
pub async fn get_mcp_status(app: AppHandle) -> Result<McpStatus, AppError> {
    run_blocking("get_mcp_status", move || {
        let mcp = app.state::<McpService>();
        Ok(mcp.status())
    })
    .await
}

#[tauri::command]
pub async fn set_mcp_enabled(app: AppHandle, enabled: bool) -> Result<McpStatus, AppError> {
    run_blocking("set_mcp_enabled", move || {
        let mcp = app.state::<McpService>();
        mcp.set_enabled(&app, enabled)
    })
    .await
}

#[tauri::command]
pub async fn restart_mcp(app: AppHandle) -> Result<McpStatus, AppError> {
    run_blocking("restart_mcp", move || {
        let mcp = app.state::<McpService>();
        mcp.restart(&app)
    })
    .await
}
