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
        let ctx = mcp_context(&app)
            .ok_or_else(|| AppError::new(ErrorCode::ConfigInvalid, "app state unavailable"))?;
        let status = mcp.set_enabled(&ctx, enabled)?;
        crate::tray::sync_view(&app);
        Ok(status)
    })
    .await
}

#[tauri::command]
pub async fn restart_mcp(app: AppHandle) -> Result<McpStatus, AppError> {
    run_blocking("restart_mcp", move || {
        let mcp = app.state::<McpService>();
        let ctx = mcp_context(&app)
            .ok_or_else(|| AppError::new(ErrorCode::ConfigInvalid, "app state unavailable"))?;
        mcp.restart(&ctx)
    })
    .await
}
