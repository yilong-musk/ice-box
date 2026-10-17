// SPDX-License-Identifier: GPL-3.0-or-later

//! Read-side core status (ORCH-1).
//!
//! Transitions still serialize on `AppState.core`. Status polling and the
//! UI event read an immutable snapshot and never take that mutex.

use crate::runtime_status::timestamp_ms;
use ice_core::{CoreError, CoreHandle, CorePaths, CoreState, CoreStatus, ReloadOutcome};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, Emitter};

pub const CORE_STATUS_CHANGED: &str = "core://status-changed";
pub const WINDOW_HIDDEN: &str = "window://hidden";
pub const WINDOW_SHOWN: &str = "window://shown";
pub const TRAFFIC_SAMPLE: &str = "traffic://sample";
/// A mutation the window did not initiate (today: tray actions, launch-time
/// restore). Pages re-read status and settings instead of waiting for their
/// fallback poll.
pub const APP_STATE_CHANGED: &str = "app://state-changed";

#[cfg(test)]
#[test]
fn runtime_probe_invalidations_are_coalesced_and_rearmed_on_mutations() {
    let hub = CoreSnapshotHub::from_state(CoreState::default());
    assert!(hub.take_probe_refresh());
    assert!(!hub.take_probe_refresh());
    hub.request_probe_refresh();
    hub.request_probe_refresh();
    assert!(hub.take_probe_refresh());
    assert!(!hub.take_probe_refresh());
    hub.publish(CoreState::default());
    assert!(hub.take_probe_refresh());
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct CoreSnapshot {
    pub state: CoreState,
    pub generation: u64,
    /// Wall-clock ms when the current run first reached `Running`; `None` otherwise.
    pub running_since_ms: Option<u64>,
}

pub struct CoreSnapshotHub {
    inner: RwLock<Arc<CoreSnapshot>>,
    emitter: Mutex<Option<AppHandle>>,
    probes_dirty: AtomicBool,
}

impl CoreSnapshotHub {
    pub fn from_state(state: CoreState) -> Arc<Self> {
        Arc::new(Self {
            inner: RwLock::new(Arc::new(CoreSnapshot {
                running_since_ms: running_since(&state, None),
                state,
                generation: 0,
            })),
            emitter: Mutex::new(None),
            probes_dirty: AtomicBool::new(true),
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
        self.publish_with_run_start(state, false);
    }

    fn publish_restarted(&self, state: CoreState) {
        self.publish_with_run_start(state, true);
    }

    fn publish_with_run_start(&self, state: CoreState, restart_run: bool) {
        self.request_probe_refresh();
        let snapshot = {
            let mut slot = self.inner.write().unwrap_or_else(|e| e.into_inner());
            let generation = slot.generation.saturating_add(1);
            let previous = (!restart_run).then_some(slot.running_since_ms).flatten();
            let running_since_ms = running_since(&state, previous);
            let next = Arc::new(CoreSnapshot {
                state,
                generation,
                running_since_ms,
            });
            *slot = next.clone();
            next
        };
        if let Ok(guard) = self.emitter.lock() {
            if let Some(app) = guard.as_ref() {
                let _ = app.emit(CORE_STATUS_CHANGED, snapshot.as_ref());
            }
        }
    }

    pub fn request_probe_refresh(&self) {
        self.probes_dirty.store(true, Ordering::Release);
    }

    pub fn take_probe_refresh(&self) -> bool {
        self.probes_dirty.swap(false, Ordering::AcqRel)
    }
}

/// Keeps the start of an ongoing run; a run starts when `Running` is first seen.
fn running_since(state: &CoreState, previous: Option<u64>) -> Option<u64> {
    match state.status {
        CoreStatus::Running => previous.or_else(|| Some(timestamp_ms())),
        _ => None,
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
        let mut preview = self.inner.state();
        preview.status = CoreStatus::Starting;
        self.hub.publish(preview);
        let result = self.inner.start(paths);
        self.publish();
        result
    }

    fn stop(&mut self, pid_file: &Path) -> Result<(), CoreError> {
        let mut preview = self.inner.state();
        preview.status = CoreStatus::Stopping;
        self.hub.publish(preview);
        let result = self.inner.stop(pid_file);
        self.publish();
        result
    }

    fn reload(&mut self, paths: &CorePaths) -> Result<ReloadOutcome, CoreError> {
        let result = self.inner.reload(paths);
        if matches!(&result, Ok(ReloadOutcome::Restarted)) {
            self.hub.publish_restarted(self.inner.state());
        } else {
            self.publish();
        }
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
        let mut preview = self.inner.state();
        preview.status = CoreStatus::Starting;
        self.hub.publish(preview);
        let result = self.inner.adopt_external(pid, paths);
        self.publish();
        result
    }

    fn reclaim_orphan_pid(&mut self, pid_file: &Path, cores: &[&Path]) -> Result<(), CoreError> {
        let result = self.inner.reclaim_orphan_pid(pid_file, cores);
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

#[cfg(test)]
#[test]
fn running_since_survives_repeated_running_publishes_and_clears_on_stop() {
    let hub = CoreSnapshotHub::from_state(CoreState::default());
    assert_eq!(hub.load().running_since_ms, None);
    let running = CoreState {
        status: CoreStatus::Running,
        ..CoreState::default()
    };
    hub.publish(running.clone());
    let started = hub
        .load()
        .running_since_ms
        .expect("running run has a start time");
    hub.publish(running);
    assert_eq!(hub.load().running_since_ms, Some(started));
    hub.publish(CoreState::default());
    assert_eq!(hub.load().running_since_ms, None);
}

#[cfg(test)]
#[test]
fn a_restarted_reload_starts_a_new_run_clock() {
    let running = CoreState {
        status: CoreStatus::Running,
        ..CoreState::default()
    };
    let hub = CoreSnapshotHub::from_state(running.clone());
    let started = hub
        .load()
        .running_since_ms
        .expect("running run has a start time");

    std::thread::sleep(std::time::Duration::from_millis(10));
    let inner = RestartingReloadCore {
        state: running,
        outcome: ReloadOutcome::Restarted,
    };
    let mut core = PublishingCore::new(Box::new(inner), hub.clone());
    let paths = CorePaths {
        binary: std::path::PathBuf::new(),
        extra_binaries: Vec::new(),
        config: std::path::PathBuf::new(),
        log_file: std::path::PathBuf::new(),
        pid_file: std::path::PathBuf::new(),
        inbound_host: "127.0.0.1".into(),
        inbound_port: 17890,
        clash_api_host: "127.0.0.1".into(),
        clash_api_port: 19090,
        clash_api_secret: String::new(),
        allow_lan: false,
    };
    assert_eq!(
        core.reload(&paths).expect("mock reload"),
        ReloadOutcome::Restarted
    );

    let restarted = hub
        .load()
        .running_since_ms
        .expect("restarted run has a start time");
    assert!(restarted > started);
}

#[cfg(test)]
struct RestartingReloadCore {
    state: CoreState,
    outcome: ReloadOutcome,
}

#[cfg(test)]
impl CoreHandle for RestartingReloadCore {
    fn state(&self) -> CoreState {
        self.state.clone()
    }

    fn start(&mut self, _paths: &CorePaths) -> Result<(), CoreError> {
        Ok(())
    }

    fn stop(&mut self, _pid_file: &Path) -> Result<(), CoreError> {
        Ok(())
    }

    fn reload(&mut self, _paths: &CorePaths) -> Result<ReloadOutcome, CoreError> {
        Ok(self.outcome)
    }

    fn needs_proxy_restore(&self) -> bool {
        false
    }

    fn clear_needs_proxy_restore(&mut self) {}

    fn reap_exited_child(&mut self, _pid_file: &Path) -> bool {
        false
    }

    fn adopt_external(&mut self, _pid: u32, _paths: &CorePaths) -> Result<(), CoreError> {
        Ok(())
    }

    fn reclaim_orphan_pid(&mut self, _pid_file: &Path, _cores: &[&Path]) -> Result<(), CoreError> {
        Ok(())
    }
}
