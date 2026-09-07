// SPDX-License-Identifier: GPL-3.0-or-later

//! Read-side core status (ORCH-1).
//!
//! Transitions still serialize on `AppState.core`. Status polling and the
//! UI event read an immutable snapshot and never take that mutex.

use ice_core::{CoreError, CoreHandle, CorePaths, CoreState, ReloadOutcome};
use serde::Serialize;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, Emitter};

pub const CORE_STATUS_CHANGED: &str = "core://status-changed";
pub const WINDOW_HIDDEN: &str = "window://hidden";
pub const WINDOW_SHOWN: &str = "window://shown";
pub const TRAFFIC_SAMPLE: &str = "traffic://sample";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CoreSnapshot {
    pub state: CoreState,
    pub generation: u64,
}

impl Default for CoreSnapshot {
    fn default() -> Self {
        Self {
            state: CoreState::default(),
            generation: 0,
        }
    }
}

pub struct CoreSnapshotHub {
    inner: RwLock<Arc<CoreSnapshot>>,
    emitter: Mutex<Option<AppHandle>>,
}

impl CoreSnapshotHub {
    pub fn from_state(state: CoreState) -> Arc<Self> {
        Arc::new(Self {
            inner: RwLock::new(Arc::new(CoreSnapshot {
                state,
                generation: 0,
            })),
            emitter: Mutex::new(None),
        })
    }

    pub fn bind_emitter(&self, app: AppHandle) {
        if let Ok(mut slot) = self.emitter.lock() {
            *slot = Some(app);
        }
    }

    pub fn load(&self) -> Arc<CoreSnapshot> {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn publish(&self, state: CoreState) {
        let snapshot = {
            let mut slot = self.inner.write().unwrap_or_else(|e| e.into_inner());
            let generation = slot.generation.saturating_add(1);
            let next = Arc::new(CoreSnapshot { state, generation });
            *slot = next.clone();
            next
        };
        if let Ok(guard) = self.emitter.lock() {
            if let Some(app) = guard.as_ref() {
                let _ = app.emit(CORE_STATUS_CHANGED, snapshot.as_ref());
            }
        }
    }
}

/// Forwards `CoreHandle` mutations and publishes a snapshot after each one.
pub struct PublishingCore {
    inner: Box<dyn CoreHandle>,
    hub: Arc<CoreSnapshotHub>,
}

impl PublishingCore {
    pub fn new(inner: Box<dyn CoreHandle>, hub: Arc<CoreSnapshotHub>) -> Self {
        Self { inner, hub }
    }

    fn publish(&self) {
        self.hub.publish(self.inner.state());
    }
}

impl CoreHandle for PublishingCore {
    fn state(&self) -> CoreState {
        self.inner.state()
    }

    fn start(&mut self, paths: &CorePaths) -> Result<(), CoreError> {
        let result = self.inner.start(paths);
        self.publish();
        result
    }

    fn stop(&mut self, pid_file: &Path) -> Result<(), CoreError> {
        let result = self.inner.stop(pid_file);
        self.publish();
        result
    }

    fn reload(&mut self, paths: &CorePaths) -> Result<ReloadOutcome, CoreError> {
        let result = self.inner.reload(paths);
        self.publish();
        result
    }

    fn needs_proxy_restore(&self) -> bool {
        self.inner.needs_proxy_restore()
    }

    fn clear_needs_proxy_restore(&mut self) {
        self.inner.clear_needs_proxy_restore();
        self.publish();
    }

    fn reap_exited_child(&mut self, pid_file: &Path) -> bool {
        let changed = self.inner.reap_exited_child(pid_file);
        if changed {
            self.publish();
        }
        changed
    }

    fn adopt_external(&mut self, pid: u32, paths: &CorePaths) -> Result<(), CoreError> {
        let result = self.inner.adopt_external(pid, paths);
        self.publish();
        result
    }

    fn reclaim_orphan_pid(&mut self, pid_file: &Path) -> Result<(), CoreError> {
        let result = self.inner.reclaim_orphan_pid(pid_file);
        self.publish();
        result
    }
}

/// Wrap a controller so every mutation publishes into `hub`.
pub fn wrap_core(
    inner: Box<dyn CoreHandle>,
) -> (std::sync::Mutex<Box<dyn CoreHandle>>, Arc<CoreSnapshotHub>) {
    let hub = CoreSnapshotHub::from_state(inner.state());
    let wrapped: Box<dyn CoreHandle> = Box::new(PublishingCore::new(inner, hub.clone()));
    (std::sync::Mutex::new(wrapped), hub)
}
