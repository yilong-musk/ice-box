// SPDX-License-Identifier: GPL-3.0-or-later

//! Ownership and cooperative lifecycle for independent background workers.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Phase {
    stopping: bool,
    pauses: usize,
}

#[derive(Default)]
struct Control {
    phase: Mutex<Phase>,
    wake: Condvar,
}

/// A coalescing "run now" request for a worker that otherwise sleeps on a
/// timer. Any number of `wake` calls before the worker next looks collapse into
/// one early return from [`WorkerToken::wait_or_wake`]; a call made before the
/// worker ever waited is remembered, not lost.
#[derive(Default)]
pub(crate) struct WakeSignal {
    pending: AtomicBool,
    /// The waiter's control block, bound on its first wait so the signaller
    /// needs no handle to the supervisor.
    control: Mutex<Option<Arc<Control>>>,
}

impl WakeSignal {
    pub(crate) fn wake(&self) {
        self.pending.store(true, Ordering::Release);
        let control = self
            .control
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(control) = control {
            // A waiter checks `pending` while holding the phase lock and only
            // releases it by parking. Taking the lock here orders this
            // notification after that park, so the wake cannot slip between
            // the check and the wait.
            drop(control.phase.lock().unwrap_or_else(|e| e.into_inner()));
            control.wake.notify_all();
        }
    }

    #[cfg(test)]
    pub(crate) fn is_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }

    fn bind(&self, control: &Arc<Control>) {
        let mut slot = self.control.lock().unwrap_or_else(|e| e.into_inner());
        if !slot
            .as_ref()
            .is_some_and(|bound| Arc::ptr_eq(bound, control))
        {
            *slot = Some(control.clone());
        }
    }
}

#[derive(Clone)]
pub(crate) struct WorkerToken(Arc<Control>);

impl WorkerToken {
    /// Wait without busy polling. Pause retains the deadline; cancellation
    /// interrupts even an hourly timer immediately. Uses a monotonic clock.
    pub(crate) fn wait(&self, delay: Duration) -> bool {
        self.wait_inner(delay, None)
    }

    /// Like [`Self::wait`], but also returns `true` early when `signal` was
    /// woken. Cancellation still wins, and a paused worker keeps sleeping (the
    /// wake stays pending until the pause ends).
    pub(crate) fn wait_or_wake(&self, delay: Duration, signal: &WakeSignal) -> bool {
        signal.bind(&self.0);
        self.wait_inner(delay, Some(signal))
    }

    fn wait_inner(&self, delay: Duration, signal: Option<&WakeSignal>) -> bool {
        let deadline = Instant::now() + delay;
        let mut phase = self.0.phase.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if phase.stopping {
                return false;
            }
            if phase.pauses > 0 {
                phase = self.0.wake.wait(phase).unwrap_or_else(|e| e.into_inner());
                continue;
            }
            if signal.is_some_and(|signal| signal.pending.swap(false, Ordering::AcqRel)) {
                return true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return true;
            }
            (phase, _) = self
                .0
                .wake
                .wait_timeout(phase, remaining)
                .unwrap_or_else(|e| e.into_inner());
        }
    }
}

struct Worker {
    name: &'static str,
    handle: JoinHandle<()>,
    restarts: Arc<AtomicU64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorkerStatus {
    pub name: &'static str,
    pub alive: bool,
    pub restarts: u64,
}

#[derive(Clone, Default)]
pub struct WorkerSupervisor {
    control: Arc<Control>,
    workers: Arc<Mutex<Vec<Worker>>>,
}

pub(crate) struct PauseGuard(Arc<Control>);

impl Drop for PauseGuard {
    fn drop(&mut self) {
        let mut phase = self.0.phase.lock().unwrap_or_else(|e| e.into_inner());
        phase.pauses = phase.pauses.saturating_sub(1);
        self.0.wake.notify_all();
    }
}

impl WorkerSupervisor {
    /// A panic restarts only this worker, with a bounded retry delay. The jobs
    /// remain independent so subscription I/O cannot delay core health checks.
    pub(crate) fn spawn(
        &self,
        name: &'static str,
        mut job: impl FnMut(WorkerToken) + Send + 'static,
    ) {
        let mut workers = self.workers.lock().unwrap_or_else(|e| e.into_inner());
        if self
            .control
            .phase
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stopping
        {
            return;
        }
        let token = WorkerToken(self.control.clone());
        let restarts = Arc::new(AtomicU64::new(0));
        let counter = restarts.clone();
        let handle = thread::Builder::new().name(name.into()).spawn(move || {
            while token.wait(Duration::ZERO) {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(token.clone())));
                token.0.wake.notify_all();
                if result.is_ok() {
                    break;
                }
                counter.fetch_add(1, Ordering::Relaxed);
                tracing::error!(worker = name, "background worker panicked; restarting");
                if !token.wait(Duration::from_secs(1)) {
                    break;
                }
            }
            token.0.wake.notify_all();
        });
        match handle {
            Ok(handle) => workers.push(Worker {
                name,
                handle,
                restarts,
            }),
            Err(error) => {
                tracing::error!(worker = name, %error, "background worker could not start")
            }
        }
    }

    pub(crate) fn pause(&self) -> PauseGuard {
        let mut phase = self.control.phase.lock().unwrap_or_else(|e| e.into_inner());
        phase.pauses += 1;
        self.control.wake.notify_all();
        PauseGuard(self.control.clone())
    }

    pub(crate) fn is_paused_or_stopping(&self) -> bool {
        let phase = self.control.phase.lock().unwrap_or_else(|e| e.into_inner());
        phase.stopping || phase.pauses > 0
    }

    pub(crate) fn statuses(&self) -> Vec<WorkerStatus> {
        self.workers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|worker| WorkerStatus {
                name: worker.name,
                alive: !worker.handle.is_finished(),
                restarts: worker.restarts.load(Ordering::Relaxed),
            })
            .collect()
    }

    /// Stop accepting work, wake every timer, and join within one shared budget.
    /// Unfinished bounded I/O keeps its handle for a later shutdown attempt.
    pub(crate) fn shutdown(&self, budget: Duration) -> Vec<&'static str> {
        {
            let mut phase = self.control.phase.lock().unwrap_or_else(|e| e.into_inner());
            phase.stopping = true;
            self.control.wake.notify_all();
        }
        let deadline = Instant::now() + budget;
        loop {
            let mut workers = self.workers.lock().unwrap_or_else(|e| e.into_inner());
            let mut i = 0;
            while i < workers.len() {
                if workers[i].handle.is_finished() {
                    let worker = workers.swap_remove(i);
                    let _ = worker.handle.join();
                } else {
                    i += 1;
                }
            }
            if workers.is_empty() {
                return Vec::new();
            }
            if Instant::now() >= deadline {
                let pending: Vec<_> = workers.iter().map(|worker| worker.name).collect();
                tracing::warn!(
                    ?pending,
                    "workers still completing bounded work at shutdown"
                );
                return pending;
            }
            drop(workers);
            let phase = self.control.phase.lock().unwrap_or_else(|e| e.into_inner());
            let _wait = self
                .control
                .wake
                .wait_timeout(phase, Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn shutdown_wakes_an_hourly_wait_and_rejects_new_workers() {
        let supervisor = WorkerSupervisor::default();
        let (ready, started) = mpsc::channel();
        supervisor.spawn("hourly", move |token| {
            ready.send(()).unwrap();
            assert!(!token.wait(Duration::from_secs(3600)));
        });
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(supervisor.shutdown(Duration::from_secs(2)).is_empty());
        supervisor.spawn("late", |_| panic!("must not start after shutdown"));
        assert!(supervisor.statuses().is_empty());
    }

    #[test]
    fn dropping_a_failed_quit_pause_resumes_work() {
        let supervisor = WorkerSupervisor::default();
        let pause = supervisor.pause();
        let (sent, received) = mpsc::channel();
        supervisor.spawn("paused", move |_| {
            sent.send(()).unwrap();
        });
        assert!(received.recv_timeout(Duration::from_millis(30)).is_err());
        drop(pause);
        received.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(supervisor.shutdown(Duration::from_secs(2)).is_empty());
    }

    #[test]
    fn shutdown_cancels_a_paused_worker_without_resuming_it() {
        let supervisor = WorkerSupervisor::default();
        let _pause = supervisor.pause();
        let (sent, received) = mpsc::channel();
        supervisor.spawn("paused", move |_| {
            sent.send(()).unwrap();
        });
        assert!(supervisor.shutdown(Duration::from_secs(2)).is_empty());
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn a_wake_interrupts_a_long_wait_and_cancellation_still_wins() {
        let supervisor = WorkerSupervisor::default();
        let signal = Arc::new(WakeSignal::default());
        let (ready, started) = mpsc::channel();
        let (woken, wake_seen) = mpsc::channel();
        let worker_signal = signal.clone();
        supervisor.spawn("sleeper", move |token| {
            ready.send(()).unwrap();
            // The hourly timer must be cut short by the wake ...
            let resumed = token.wait_or_wake(Duration::from_secs(3600), &worker_signal);
            woken.send(resumed).unwrap();
            // ... and shutdown must still interrupt the next wait.
            assert!(!token.wait_or_wake(Duration::from_secs(3600), &worker_signal));
        });
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        signal.wake();
        assert!(wake_seen.recv_timeout(Duration::from_secs(2)).unwrap());
        assert!(supervisor.shutdown(Duration::from_secs(2)).is_empty());
    }

    #[test]
    fn a_wake_sent_before_the_first_wait_is_not_lost_and_coalesces() {
        let supervisor = WorkerSupervisor::default();
        let signal = Arc::new(WakeSignal::default());
        signal.wake();
        signal.wake();
        let (seen, results) = mpsc::channel();
        let worker_signal = signal.clone();
        supervisor.spawn("early", move |token| {
            let first = Instant::now();
            assert!(token.wait_or_wake(Duration::from_secs(3600), &worker_signal));
            seen.send(first.elapsed()).unwrap();
            // Both wakes collapsed into one: the next wait runs to its deadline.
            let second = Instant::now();
            assert!(token.wait_or_wake(Duration::from_millis(60), &worker_signal));
            seen.send(second.elapsed()).unwrap();
        });
        assert!(results.recv_timeout(Duration::from_secs(2)).unwrap() < Duration::from_secs(1));
        assert!(results.recv_timeout(Duration::from_secs(2)).unwrap() >= Duration::from_millis(60));
        assert!(supervisor.shutdown(Duration::from_secs(2)).is_empty());
    }

    #[test]
    fn a_paused_worker_keeps_a_wake_pending_until_resumed() {
        let supervisor = WorkerSupervisor::default();
        let signal = Arc::new(WakeSignal::default());
        let (ready, started) = mpsc::channel();
        let (resumed, resumed_seen) = mpsc::channel();
        let worker_signal = signal.clone();
        supervisor.spawn("paused-sleeper", move |token| {
            ready.send(()).unwrap();
            resumed
                .send(token.wait_or_wake(Duration::from_secs(3600), &worker_signal))
                .unwrap();
        });
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        let pause = supervisor.pause();
        signal.wake();
        assert!(resumed_seen
            .recv_timeout(Duration::from_millis(60))
            .is_err());
        drop(pause);
        assert!(resumed_seen.recv_timeout(Duration::from_secs(2)).unwrap());
        assert!(supervisor.shutdown(Duration::from_secs(2)).is_empty());
    }

    #[test]
    fn one_panicking_worker_restarts_without_stopping_its_siblings() {
        let supervisor = WorkerSupervisor::default();
        let mut attempts = 0;
        let (sent, received) = mpsc::channel();
        supervisor.spawn("retry", move |_| {
            attempts += 1;
            if attempts == 1 {
                panic!("injected failure");
            }
            sent.send(()).unwrap();
        });
        received.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(supervisor.statuses()[0].restarts, 1);
        assert!(supervisor.shutdown(Duration::from_secs(2)).is_empty());
    }
}
