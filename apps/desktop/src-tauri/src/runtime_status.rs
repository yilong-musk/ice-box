// SPDX-License-Identifier: GPL-3.0-or-later

//! Committed runtime reads and independently sampled, explicitly aged probes.

use crate::application::StatusResponse;
use crate::workers::{WakeSignal, WorkerToken};
use ice_config::UiMessage;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How often the probe worker re-samples once a sample is this old.
pub(crate) const PROBE_INTERVAL: Duration = Duration::from_secs(2);

/// How long a sample may be served to readers. Deliberately longer than
/// `PROBE_INTERVAL`: the worker starts refreshing at one interval, and the
/// probe itself takes time (a helper socket call, `networksetup`, the
/// registry). If a sample expired at the moment its refresh began, every
/// refresh would open a window in which readers get `None`, and the UI would
/// flicker between "active" and "recorded".
pub(crate) const PROBE_MAX_AGE: Duration = Duration::from_secs(PROBE_INTERVAL.as_secs() * 2);

pub(crate) fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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
    /// A refresh was asked for without discarding the current sample.
    refresh_requested: AtomicBool,
    /// Cuts the probe worker's sleep short when a refresh is wanted now.
    probe_wake: WakeSignal,
    /// Single-flight slow work; never held by status reads.
    pub(crate) probe_refresh: Mutex<()>,
}

/// Clock and probe-age fields move on every read. They are not a new
/// committed view, and neither is a memory figure that was already published.
fn same_committed_view(previous: &StatusResponse, next: &StatusResponse) -> bool {
    let mut previous = previous.clone();
    let mut next = next.clone();
    previous.revision = 0;
    next.revision = 0;
    previous.sampled_at_ms = 0;
    next.sampled_at_ms = 0;
    previous.diagnostics.checked_at_ms = None;
    next.diagnostics.checked_at_ms = None;
    previous.diagnostics.age_ms = None;
    next.diagnostics.age_ms = None;
    previous == next
}

impl RuntimeReadModel {
    pub(crate) fn publish(&self, mut status: StatusResponse) -> StatusResponse {
        let mut slot = self.latest.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = slot.as_ref() {
            if same_committed_view(previous, &status) {
                return previous.clone();
            }
        }
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

    /// A mutation crossed the sampled state: the current sample no longer
    /// describes it and is withheld from readers until a new probe lands.
    /// Wakes the probe worker so that gap is one probe long, not one interval.
    pub(crate) fn invalidate_probes(&self) {
        self.probe_epoch.fetch_add(1, Ordering::AcqRel);
        self.probe_wake.wake();
    }

    /// Re-sample soon without withholding the current sample (window focus,
    /// state announcements). Changes that matter to readers are still caught
    /// by the core generation and settings signature checks, and by
    /// `PROBE_MAX_AGE`; readers keep the last value meanwhile instead of
    /// flickering to "unknown" for the length of one probe.
    pub(crate) fn request_refresh(&self) {
        self.refresh_requested.store(true, Ordering::Release);
        self.probe_wake.wake();
    }

    pub(crate) fn probe_epoch(&self) -> u64 {
        self.probe_epoch.load(Ordering::Acquire)
    }

    /// Claim a refresh if one is due: the sample was invalidated, failed, is
    /// a full `PROBE_INTERVAL` old, or a refresh was requested. Returns the
    /// epoch the probe must complete against.
    pub(crate) fn begin_refresh(&self, now: Instant) -> Option<u64> {
        let epoch = self.probe_epoch();
        let sample = self.probes.lock().unwrap_or_else(|e| e.into_inner());
        let due = sample.epoch != epoch
            || sample.error.is_some()
            || sample
                .checked_at
                .is_none_or(|at| now.saturating_duration_since(at) >= PROBE_INTERVAL);
        drop(sample);
        // Consume the request only once it is being served; one that arrives
        // during the probe stays pending and re-arms the worker.
        let requested = self.refresh_requested.swap(false, Ordering::AcqRel);
        (due || requested).then_some(epoch)
    }

    /// Sleep until the next refresh is due, or until one is asked for.
    /// Returns `false` when the worker was cancelled.
    pub(crate) fn wait_for_next_probe(&self, cancel: &WorkerToken) -> bool {
        cancel.wait_or_wake(PROBE_INTERVAL, &self.probe_wake)
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
                || age.is_none_or(|age| age >= PROBE_MAX_AGE)
                || sample.error.is_some(),
            error: sample.error.clone(),
        };
        (sample.values.clone(), freshness)
    }

    /// Discard slow work that crossed a mutation; it cannot describe the new state.
    ///
    /// Returns whether readers may have observed something different: the
    /// values changed, or the previous sample was being withheld (invalidated,
    /// failed, or older than `PROBE_MAX_AGE`), in which case a reader that saw
    /// `None` needs to be told it can look again.
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
                let was_served = sample.epoch == epoch
                    && sample.error.is_none()
                    && sample
                        .checked_at
                        .is_some_and(|at| now.saturating_duration_since(at) < PROBE_MAX_AGE);
                let changed = !was_served || sample.values != values;
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
        // Invalidations and refresh requests cut this sleep short.
        if !state.runtime_status.wait_for_next_probe(&cancel) {
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
        assert!(model.probes_at(now + PROBE_MAX_AGE).1.stale);
        assert_eq!(model.probes_at(now + PROBE_MAX_AGE).1.age_ms, Some(4000));
    }

    #[test]
    fn a_sample_stays_served_while_its_refresh_is_running() {
        let model = RuntimeReadModel::default();
        let now = Instant::now();
        model.complete_probe(0, Ok(ProbeValues::default()), now);
        assert_eq!(model.begin_refresh(now), None);
        // The refresh is due one interval in, yet readers still get the
        // sample for a second interval so the probe never opens a `None` gap.
        let due = now + PROBE_INTERVAL;
        assert_eq!(model.begin_refresh(due), Some(0));
        assert!(!model.probes_at(due).1.stale);
        assert!(
            !model
                .probes_at(due + PROBE_INTERVAL - Duration::from_millis(1))
                .1
                .stale
        );
        assert!(model.probes_at(due + PROBE_INTERVAL).1.stale);
    }

    #[test]
    fn invalidation_makes_a_refresh_due_and_wakes_the_worker() {
        let model = RuntimeReadModel::default();
        let now = Instant::now();
        model.complete_probe(0, Ok(ProbeValues::default()), now);
        assert_eq!(model.begin_refresh(now), None);
        model.invalidate_probes();
        assert!(model.probes_at(now).1.stale);
        assert_eq!(model.begin_refresh(now), Some(1));
        assert!(model.probe_wake.is_pending());
    }

    #[test]
    fn a_refresh_request_keeps_the_sample_served() {
        let model = RuntimeReadModel::default();
        let now = Instant::now();
        model.complete_probe(0, Ok(ProbeValues::default()), now);
        model.request_refresh();
        assert!(!model.probes_at(now).1.stale);
        assert_eq!(model.begin_refresh(now), Some(0));
        // Claimed once; a request that arrives later is a new claim.
        assert_eq!(model.begin_refresh(now), None);
        model.request_refresh();
        assert_eq!(model.begin_refresh(now), Some(0));
    }

    #[test]
    fn completion_reports_a_change_when_readers_may_have_seen_no_sample() {
        let model = RuntimeReadModel::default();
        let now = Instant::now();
        let values = ProbeValues::default();
        assert!(model.complete_probe(0, Ok(values.clone()), now));
        // Same values, sample still served: nothing to announce.
        assert!(!model.complete_probe(0, Ok(values.clone()), now + PROBE_INTERVAL));
        // Same values, but the sample had aged out: readers saw `None`.
        assert!(model.complete_probe(0, Ok(values.clone()), now + PROBE_INTERVAL + PROBE_MAX_AGE));
        // Same values, but an invalidation withheld the sample meanwhile.
        model.invalidate_probes();
        assert!(model.complete_probe(1, Ok(values), now + PROBE_INTERVAL + PROBE_MAX_AGE));
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
