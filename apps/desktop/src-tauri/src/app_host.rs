// SPDX-License-Identifier: GPL-3.0-or-later

//! Tauri adapter for application ports. Business use cases never import it.

use crate::application::{invalidate_runtime_probes, AppHost, AppResources};
use crate::AppState;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, Runtime};

impl<R: Runtime> AppResources for AppHandle<R> {
    fn resource_dir(&self) -> Option<PathBuf> {
        self.path().resource_dir().ok()
    }
}

impl AppHost for AppHandle {
    fn state_changed(&self) {
        if let Some(state) = self.try_state::<AppState>() {
            invalidate_runtime_probes(state.inner());
        }
        let _ = self.emit(crate::core_snapshot::APP_STATE_CHANGED, ());
        crate::tray::sync_menu(self);
    }
}
