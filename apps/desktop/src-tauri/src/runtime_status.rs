// SPDX-License-Identifier: GPL-3.0-or-later

//! Committed runtime reads and independently sampled, explicitly aged probes.

use crate::application::StatusResponse;
use ice_config::UiMessage;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) const PROBE_INTERVAL: Duration = Duration::from_secs(2);

pub(crate) fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone, Debug, Serialize)]
pub struct ProbeFreshness {
    pub checked_at_ms: Option<u64>,
    pub age_ms: Option<u64>,
    pub stale: bool,
    pub error: Option<UiMessage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProbeValues {
    pub core_generation: u64,
    pub settings_signature: Option<(SystemTime, u64)>,
    pub system_proxy_applied: Option<bool>,
    pub helper_installed: bool,
    pub helper_stale: bool,
    pub tun_elevation_ready: bool,
}

impl Default for ProbeValues {
    fn default() -> Self {
        Self {
            core_generation: 0,
            settings_signature: None,
            system_proxy_applied: None,
            helper_installed: false,
            helper_stale: false,
            tun_elevation_ready: !cfg!(target_os = "windows"),
        }
    }
}

#[derive(Default)]
struct ProbeSample {
    values: ProbeValues,
    checked_at: Option<Instant>,
    checked_at_ms: Option<u64>,
    epoch: u64,
    error: Option<UiMessage>,
}

#[derive(Default)]
pub struct RuntimeReadModel {
    /// Serialize short read assemblies without mistaking another reader for
    /// a mutation. Slow probes never acquire this gate.
    pub(crate) read_gate: Mutex<()>,
    latest: Mutex<Option<StatusResponse>>,
    revision: AtomicU64,
    probe_epoch: AtomicU64,
    probes: Mutex<ProbeSample>,
    /// Single-flight slow work; never held by status reads.
    pub(crate) probe_refresh: Mutex<()>,
}

impl RuntimeReadModel {
    pub(crate) fn publish(&self, mut status: StatusResponse) -> StatusResponse {
        let mut slot = self.latest.lock().unwrap_or_else(|e| e.into_inner());
        status.revision = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
        *slot = Some(status.clone());
        status
    }

    pub(crate) fn latest(&self) -> Option<StatusResponse> {
        self.latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub(crate) fn invalidate_probes(&self) {
        self.probe_epoch.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) fn probe_epoch(&self) -> u64 {
        self.probe_epoch.load(Ordering::Acquire)
    }

    pub(crate) fn probes_at(&self, now: Instant) -> (ProbeValues, ProbeFreshness) {
        let sample = self.probes.lock().unwrap_or_else(|e| e.into_inner());
        let age = sample
            .checked_at
            .map(|at| now.saturating_duration_since(at));
        let freshness = ProbeFreshness {
            checked_at_ms: sample.checked_at_ms,
            age_ms: age.map(|age| age.as_millis() as u64),
            stale: sample.epoch != self.probe_epoch()
                || age.is_none_or(|age| age >= PROBE_INTERVAL)
                || sample.error.is_some(),
            error: sample.error.clone(),
        };
        (sample.values.clone(), freshness)
    }

    /// Discard slow work that crossed a mutation; it cannot describe the new state.
    pub(crate) fn complete_probe(
        &self,
        epoch: u64,
        values: Result<ProbeValues, UiMessage>,
        now: Instant,
    ) -> bool {
        let mut sample = self.probes.lock().unwrap_or_else(|e| e.into_inner());
        if epoch != self.probe_epoch() {
            return false;
        }
        match values {
            Ok(values) => {
                let changed = sample.checked_at.is_none()
                    || sample.values != values
                    || sample.error.is_some();
                *sample = ProbeSample {
                    values,
                    checked_at: Some(now),
                    checked_at_ms: Some(timestamp_ms()),
                    epoch,
                    error: None,
                };
                changed
            }
            Err(error) => {
                sample.error = Some(error);
                false
            }
        }
    }
}

/// Independent slow lane: never delay core health reconciliation with probes.
pub(crate) fn spawn_probe_watchdog(app: tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let workers = app.state::<crate::AppState>().workers.clone();
    workers.spawn("runtime-probes", move |cancel| loop {
        if !cancel.wait(Duration::ZERO) {
            break;
        }
        let Some(state) = app.try_state::<crate::AppState>() else {
            break;
        };
        let changed = crate::application::refresh_runtime_probes(state.inner());
        let _ = crate::application::collect_status(state.inner());
        if changed {
            let _ = app.emit(crate::core_snapshot::APP_STATE_CHANGED, ());
        }
        if !cancel.wait(PROBE_INTERVAL) {
            break;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_freshness_uses_an_injected_monotonic_instant() {
        let model = RuntimeReadModel::default();
        let now = Instant::now();
        assert!(model.probes_at(now).1.stale);
        model.complete_probe(0, Ok(ProbeValues::default()), now);
        assert!(!model.probes_at(now).1.stale);
        assert!(model.probes_at(now + PROBE_INTERVAL).1.stale);
        assert_eq!(model.probes_at(now + PROBE_INTERVAL).1.age_ms, Some(2000));
    }

    #[test]
    fn invalidation_rejects_an_inflight_probe() {
        let model = RuntimeReadModel::default();
        let epoch = model.probe_epoch();
        model.invalidate_probes();
        let values = ProbeValues {
            helper_installed: true,
            ..ProbeValues::default()
        };
        assert!(!model.complete_probe(epoch, Ok(values), Instant::now()));
        let (values, freshness) = model.probes_at(Instant::now());
        assert!(!values.helper_installed);
        assert!(freshness.checked_at_ms.is_none());
        assert!(freshness.stale);
    }

    #[test]
    fn probe_failure_retains_the_last_value_but_marks_it_stale() {
        let model = RuntimeReadModel::default();
        let now = Instant::now();
        let values = ProbeValues {
            helper_installed: true,
            ..ProbeValues::default()
        };
        model.complete_probe(0, Ok(values), now);
        model.complete_probe(0, Err(UiMessage::raw("probe unavailable")), now);
        let (values, freshness) = model.probes_at(now);
        assert!(values.helper_installed);
        assert!(freshness.stale);
        assert!(freshness.error.is_some());
    }
}
