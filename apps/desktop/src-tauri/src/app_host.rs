// SPDX-License-Identifier: GPL-3.0-or-later

//! Tauri adapter for `ice-runtime` application ports.

use crate::AppState;
use ice_runtime::application::{request_runtime_probe_refresh, AppHost, AppResources};
use ice_runtime::mcp::McpContext;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Runtime};

#[derive(Clone)]
pub(crate) struct TauriHost<R: Runtime = tauri::Wry>(pub AppHandle<R>);

pub(crate) fn tauri_host<R: Runtime>(app: &AppHandle<R>) -> TauriHost<R> {
    TauriHost(app.clone())
}

pub(crate) fn mcp_context(app: &AppHandle) -> Option<McpContext> {
    let state = app.try_state::<AppState>()?;
    Some(McpContext::new(
        Arc::clone(state.inner()),
        Arc::new(TauriHost(app.clone())),
    ))
}

impl<R: Runtime> AppResources for TauriHost<R> {
    fn resource_dir(&self) -> Option<PathBuf> {
        self.0.path().resource_dir().ok()
    }
}

impl AppHost for TauriHost {
    fn state_changed(&self) {
        if let Some(state) = self.0.try_state::<AppState>() {
            request_runtime_probe_refresh(state.inner());
        }
        let _ = self.0.emit(crate::core_snapshot::APP_STATE_CHANGED, ());
        crate::tray::sync_menu(&self.0);
    }
}
