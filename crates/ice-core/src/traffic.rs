// SPDX-License-Identifier: GPL-3.0-or-later

//! Persistent Clash `/traffic` stream, a rolling 60-second history, and a
//! peak recomputed from that rolling window.
//!
//! Opening a new `/traffic` connection per UI poll waits ~1s for the first tick
//! and then races the 1s interval, so the chart effectively samples at ~0.5Hz
//! and forgets its series whenever the home page unmounts. One long-lived
//! stream plus a ring buffer keeps true ~1Hz samples and survives tab switches
//! without extra IPC or HTTP work on the UI thread.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::clash_api::{traffic_foreach, TrafficSample, TrafficStreamEnd};
use crate::error::CoreError;
use crate::health::HealthEndpoints;

/// Visible window on the home-page chart.
pub const TRAFFIC_WINDOW_MS: u64 = 60_000;
/// Safety cap if the stream ever emits faster than ~1Hz.
pub const TRAFFIC_HISTORY_MAX: usize = 120;

const RECONNECT_BACKOFF: Duration = Duration::from_millis(500);
const STREAM_READ_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimedTrafficSample {
    pub up: u64,
    pub down: u64,
    /// Unix time in milliseconds.
    pub t: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrafficSnapshot {
    pub points: Vec<TimedTrafficSample>,
    pub latest: Option<TrafficSample>,
    /// Highest up/down rate within the current 60-second rolling window.
    pub peak: Option<TrafficSample>,
}

/// Incremental traffic window for the chart (PERF-3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrafficDelta {
    /// Incremented when history is cleared (retarget / stop).
    pub generation: u64,
    /// Timestamp of the newest retained sample; `None` when the window is empty.
    pub cursor: Option<u64>,
    pub points: Vec<TimedTrafficSample>,
    pub latest: Option<TrafficSample>,
    pub peak: Option<TrafficSample>,
}

struct Inner {
    desired: Option<HealthEndpoints>,
    points: VecDeque<TimedTrafficSample>,
    latest: Option<TrafficSample>,
    generation: u64,
}

type SampleCallback = Arc<dyn Fn(TimedTrafficSample) + Send + Sync>;

struct Shared {
    inner: Mutex<Inner>,
    changed: Condvar,
    shutdown: AtomicBool,
    on_sample: Mutex<Option<SampleCallback>>,
    /// Test-only: panic the next `/traffic` stream on this monitor, not a
    /// process-global flag that races parallel `ice-core` tests.
    #[cfg(test)]
    panic_next_stream: AtomicBool,
}

/// App-lifetime collector: idle until endpoints are set, then one `/traffic` stream.
pub struct TrafficMonitor {
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl TrafficMonitor {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    desired: None,
                    points: VecDeque::new(),
                    latest: None,
                    generation: 0,
                }),
                changed: Condvar::new(),
                shutdown: AtomicBool::new(false),
                on_sample: Mutex::new(None),
                #[cfg(test)]
                panic_next_stream: AtomicBool::new(false),
            }),
            thread: Mutex::new(None),
        }
    }

    /// Start (or retarget) the stream. Any change of target (including `Some` →
    /// a different `Some`) drops history so the chart cannot mix two Clash
    /// APIs. `None` stops collection.
    pub fn set_endpoints(&self, endpoints: Option<HealthEndpoints>) {
        {
            let mut inner = lock_inner(&self.shared);
            if inner.desired == endpoints {
                return;
            }
            inner.desired = endpoints.clone();
            inner.points.clear();
            inner.latest = None;
            inner.generation = inner.generation.saturating_add(1);
        }
        if endpoints.is_some() {
            self.ensure_thread();
        }
        self.shared.changed.notify_all();
    }

    /// Whether a Clash API target is currently configured.
    pub fn has_target(&self) -> bool {
        lock_inner(&self.shared).desired.is_some()
    }

    pub fn snapshot(&self) -> TrafficSnapshot {
        let inner = lock_inner(&self.shared);
        TrafficSnapshot {
            points: inner.points.iter().copied().collect(),
            latest: inner.latest,
            peak: window_peak(&inner.points),
        }
    }

    /// Samples newer than `cursor` (exclusive). `cursor == None` returns the
    /// full window. `generation` changes when history is dropped.
    pub fn snapshot_since(&self, cursor: Option<u64>) -> TrafficDelta {
        let inner = lock_inner(&self.shared);
        let points: Vec<TimedTrafficSample> = match cursor {
            Some(c) => inner.points.iter().copied().filter(|p| p.t > c).collect(),
            None => inner.points.iter().copied().collect(),
        };
        TrafficDelta {
            generation: inner.generation,
            cursor: inner.points.back().map(|p| p.t),
            points,
            latest: inner.latest,
            peak: window_peak(&inner.points),
        }
    }

    /// Called from the supervisor thread after each accepted sample.
    pub fn set_on_sample(&self, cb: impl Fn(TimedTrafficSample) + Send + Sync + 'static) {
        let mut slot = self
            .shared
            .on_sample
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *slot = Some(Arc::new(cb));
    }

    #[cfg(test)]
    fn seed_history_for_test(&self, sample: TrafficSample) {
        let mut inner = lock_inner(&self.shared);
        push_sample(&mut inner, sample, now_ms());
    }

    #[cfg(test)]
    fn inject_next_stream_panic(&self) {
        self.shared.panic_next_stream.store(true, Ordering::SeqCst);
    }

    fn ensure_thread(&self) {
        let mut slot = self.thread.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(handle) = slot.as_ref() {
            if !handle.is_finished() {
                return;
            }
            if let Some(handle) = slot.take() {
                let _ = handle.join();
            }
        }
        let shared = self.shared.clone();
        *slot = Some(
            thread::Builder::new()
                .name("ice-box-traffic".into())
                .spawn(move || supervisor_loop(shared))
                .expect("spawn traffic supervisor"),
        );
    }

    fn shutdown(&self) {
        self.shared.shutdown.store(true, Ordering::SeqCst);
        self.shared.changed.notify_all();
        if let Ok(mut slot) = self.thread.lock() {
            if let Some(handle) = slot.take() {
                let _ = handle.join();
            }
        }
    }
}

impl Default for TrafficMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TrafficMonitor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn lock_inner(shared: &Shared) -> std::sync::MutexGuard<'_, Inner> {
    shared.inner.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn retain_window(points: &mut VecDeque<TimedTrafficSample>, now_ms: u64) {
    let cutoff = now_ms.saturating_sub(TRAFFIC_WINDOW_MS);
    while points.front().is_some_and(|p| p.t < cutoff) {
        points.pop_front();
    }
    while points.len() > TRAFFIC_HISTORY_MAX {
        points.pop_front();
    }
}

fn push_sample(inner: &mut Inner, sample: TrafficSample, now_ms: u64) {
    inner.latest = Some(sample);
    inner.points.push_back(TimedTrafficSample {
        up: sample.up,
        down: sample.down,
        t: now_ms,
    });
    retain_window(&mut inner.points, now_ms);
}

fn notify_sample(shared: &Shared, sample: TimedTrafficSample) {
    let cb = shared
        .on_sample
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if let Some(cb) = cb {
        cb(sample);
    }
}

/// Highest up/down rate among the samples currently inside the rolling
/// window; `None` when the window is empty.
fn window_peak(points: &VecDeque<TimedTrafficSample>) -> Option<TrafficSample> {
    let mut peak: Option<TrafficSample> = None;
    for point in points {
        peak = Some(match peak {
            Some(peak) => TrafficSample {
                up: peak.up.max(point.up),
                down: peak.down.max(point.down),
            },
            None => TrafficSample {
                up: point.up,
                down: point.down,
            },
        });
    }
    peak
}

fn wait_for_desired(shared: &Shared) -> Option<HealthEndpoints> {
    let mut inner = lock_inner(shared);
    loop {
        if shared.shutdown.load(Ordering::SeqCst) {
            return None;
        }
        if inner.desired.is_some() {
            return inner.desired.clone();
        }
        inner = match shared.changed.wait(inner) {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
    }
}

fn run_stream(endpoints: &HealthEndpoints, shared: &Shared) -> Result<TrafficStreamEnd, CoreError> {
    #[cfg(test)]
    if shared.panic_next_stream.swap(false, Ordering::SeqCst) {
        panic!("injected traffic stream panic");
    }
    traffic_foreach(endpoints, STREAM_READ_TIMEOUT, |sample| {
        if shared.shutdown.load(Ordering::SeqCst) {
            return false;
        }
        #[cfg(test)]
        if shared.panic_next_stream.swap(false, Ordering::SeqCst) {
            panic!("injected traffic stream panic");
        }
        let mut inner = lock_inner(shared);
        if inner.desired.as_ref() != Some(endpoints) {
            return false;
        }
        push_sample(&mut inner, sample, now_ms());
        let emitted = inner.points.back().copied();
        drop(inner);
        if let Some(emitted) = emitted {
            notify_sample(shared, emitted);
        }
        true
    })
}

fn supervisor_loop(shared: Arc<Shared>) {
    let mut backoff = RECONNECT_BACKOFF;
    while !shared.shutdown.load(Ordering::SeqCst) {
        let Some(endpoints) = wait_for_desired(&shared) else {
            break;
        };
        let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_stream(&endpoints, &shared)
        }));
        match run {
            Ok(Ok(TrafficStreamEnd::Stopped)) => {
                backoff = RECONNECT_BACKOFF;
            }
            Ok(Ok(TrafficStreamEnd::Eof)) => {
                tracing::debug!("traffic stream ended; backing off before reconnect");
                if shared.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
            Ok(Err(err)) => {
                tracing::debug!(error = %err, "traffic stream interrupted");
                if shared.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
            Err(_) => {
                tracing::error!("traffic supervisor panicked; restarting stream");
                if shared.shutdown.load(Ordering::SeqCst) {
                    break;
                }
                thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clash_api::{MockClashApi, TrafficSample};
    use crate::health::HealthEndpoints;
    use std::time::Instant;

    #[test]
    fn retain_window_drops_points_older_than_60s() {
        let mut points = VecDeque::new();
        points.push_back(TimedTrafficSample {
            up: 1,
            down: 1,
            t: 1_000,
        });
        points.push_back(TimedTrafficSample {
            up: 2,
            down: 2,
            t: 50_000,
        });
        points.push_back(TimedTrafficSample {
            up: 3,
            down: 3,
            t: 61_000,
        });
        retain_window(&mut points, 62_000);
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].up, 2);
        assert_eq!(points[1].up, 3);
    }

    #[test]
    fn retain_window_caps_length() {
        let mut points = VecDeque::new();
        let now = TRAFFIC_WINDOW_MS;
        for i in 0..(TRAFFIC_HISTORY_MAX as u64 + 25) {
            points.push_back(TimedTrafficSample {
                up: i,
                down: i,
                t: now,
            });
        }
        retain_window(&mut points, now);
        assert_eq!(points.len(), TRAFFIC_HISTORY_MAX);
        assert_eq!(points[0].up, 25);
    }

    #[test]
    fn window_peak_is_recomputed_from_the_rolling_window() {
        let mut points = VecDeque::new();
        points.push_back(TimedTrafficSample {
            up: 100,
            down: 200,
            t: 1_000,
        });
        points.push_back(TimedTrafficSample {
            up: 300,
            down: 400,
            t: 50_000,
        });
        points.push_back(TimedTrafficSample {
            up: 50,
            down: 60,
            t: 61_000,
        });
        assert_eq!(
            window_peak(&points),
            Some(TrafficSample { up: 300, down: 400 })
        );
        assert_eq!(window_peak(&VecDeque::new()), None);
    }

    #[test]
    fn rolling_trim_drops_the_former_peak_with_its_sample() {
        let mut inner = Inner {
            desired: None,
            points: VecDeque::new(),
            latest: None,
            generation: 0,
        };
        let high = TrafficSample {
            up: 2_000,
            down: 1_000,
        };
        let low = TrafficSample { up: 20, down: 10 };

        push_sample(&mut inner, high, 1_000);
        push_sample(&mut inner, low, TRAFFIC_WINDOW_MS + 2_000);

        assert_eq!(inner.points.len(), 1);
        assert_eq!(inner.latest, Some(low));
        assert_eq!(
            window_peak(&inner.points),
            Some(low),
            "the window peak follows the retained window, not the run total"
        );
    }

    #[test]
    fn monitor_collects_stream_and_clears_on_stop() {
        let server = MockClashApi::start(200, "Rule");
        let monitor = TrafficMonitor::new();
        monitor.set_endpoints(Some(server.endpoints()));

        let started = Instant::now();
        let snap = loop {
            let snap = monitor.snapshot();
            if snap.points.len() >= 3 {
                break snap;
            }
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "timed out waiting for traffic samples"
            );
            thread::sleep(Duration::from_millis(20));
        };
        assert!(snap.latest.is_some());
        assert!(snap.peak.is_some());
        assert!(snap.points.windows(2).all(|w| w[1].t >= w[0].t));

        monitor.set_endpoints(None);
        let empty = monitor.snapshot();
        assert!(empty.points.is_empty());
        assert!(empty.latest.is_none());
        assert!(empty.peak.is_none());
        assert!(!monitor.has_target());
    }

    #[test]
    fn retargeting_endpoints_clears_history() {
        let a = HealthEndpoints::new("127.0.0.1", 1);
        let b = HealthEndpoints::new("127.0.0.1", 2);
        let monitor = TrafficMonitor::new();
        monitor.set_endpoints(Some(a.clone()));
        monitor.seed_history_for_test(TrafficSample { up: 9, down: 8 });
        assert_eq!(monitor.snapshot().points.len(), 1);
        assert_eq!(
            monitor.snapshot().peak,
            Some(TrafficSample { up: 9, down: 8 })
        );

        monitor.set_endpoints(Some(a.clone()));
        assert_eq!(
            monitor.snapshot().points.len(),
            1,
            "same endpoints must keep history"
        );

        monitor.set_endpoints(Some(b));
        let snap = monitor.snapshot();
        assert!(snap.points.is_empty());
        assert!(snap.latest.is_none());
        assert!(snap.peak.is_none());
        assert!(monitor.has_target());
    }

    #[test]
    fn snapshot_since_returns_only_newer_samples() {
        let monitor = TrafficMonitor::new();
        {
            let mut inner = lock_inner(&monitor.shared);
            inner.points.push_back(TimedTrafficSample {
                up: 1,
                down: 1,
                t: 1_000,
            });
            inner.points.push_back(TimedTrafficSample {
                up: 2,
                down: 2,
                t: 2_000,
            });
            inner.latest = Some(TrafficSample { up: 2, down: 2 });
            inner.generation = 3;
        }
        let full = monitor.snapshot_since(None);
        assert_eq!(full.points.len(), 2);
        assert_eq!(full.generation, 3);
        let delta = monitor.snapshot_since(Some(1_000));
        assert_eq!(delta.points.len(), 1);
        assert_eq!(delta.points[0].t, 2_000);
        assert_eq!(delta.cursor, Some(2_000));
    }

    #[test]
    fn supervisor_recovers_after_injected_panic() {
        let server = MockClashApi::start(200, "Rule");
        let monitor = TrafficMonitor::new();
        monitor.set_endpoints(Some(server.endpoints()));

        let started = Instant::now();
        loop {
            if !monitor.snapshot().points.is_empty() {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "timed out waiting for first sample"
            );
            thread::sleep(Duration::from_millis(20));
        }

        monitor.inject_next_stream_panic();
        let after_panic = Instant::now();
        let before = monitor.snapshot().points.len();
        loop {
            if monitor.snapshot().points.len() > before {
                break;
            }
            assert!(
                after_panic.elapsed() < Duration::from_secs(5),
                "monitor did not recover after injected panic"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }
}
