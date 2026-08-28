//! Graceful shutdown shared by IPC `stop` and tray Quit.

use crate::orchestrate::orchestrate_stop;
use crate::AppState;
use ice_config::{AppError, ErrorCode};
use ice_core::CoreStatus;
use tauri::{AppHandle, Manager, Runtime};

fn lock_poisoned(context: &str) -> AppError {
    AppError::new(
        ErrorCode::ConfigInvalid,
        format!("internal lock poisoned: {context}"),
    )
}

fn core_is_live(status: CoreStatus) -> bool {
    matches!(status, CoreStatus::Running | CoreStatus::Starting)
}

fn detach_traffic_if_core_not_live(state: &AppState, status: CoreStatus) {
    if !core_is_live(status) {
        state.traffic.set_endpoints(None);
    }
}

/// Stop core + restore system proxy under the orchestrate lock (same serialization as `start`).
pub fn graceful_stop(state: &AppState) -> Result<(), AppError> {
    // Abort any in-flight auto-start healthcheck before waiting on the orchestrate lock.
    state
        .shutdown_requested
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let _orch = state
        .orchestrate
        .lock()
        .map_err(|_| lock_poisoned("orchestrate"))?;
    let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
    let proxy = state.proxy.lock().map_err(|_| lock_poisoned("proxy"))?;
    match orchestrate_stop(&state.paths, &mut **core, proxy.as_ref()) {
        Ok(()) => {
            drop(proxy);
            drop(core);
            state.traffic.set_endpoints(None);
            if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
                *slot = None;
            }
            Ok(())
        }
        Err(err) if err.code == ErrorCode::ProxyRestoreFailed.as_str() => {
            drop(proxy);
            drop(core);
            state.traffic.set_endpoints(None);
            // Stay open: allow another quit attempt / UI retry.
            state
                .shutdown_requested
                .store(false, std::sync::atomic::Ordering::SeqCst);
            if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
                *slot = Some(err.message.clone());
            }
            Err(err)
        }
        Err(err) => {
            drop(proxy);
            let status = core.state().status;
            drop(core);
            // Stop did not finish; drop the collector if Clash API is gone so
            // the supervisor cannot keep retrying a dead port.
            detach_traffic_if_core_not_live(state, status);
            state
                .shutdown_requested
                .store(false, std::sync::atomic::Ordering::SeqCst);
            Err(err)
        }
    }
}

/// Outcome of a tray Quit request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitOutcome {
    /// Core stopped (and proxy restored when applicable); safe to exit the app.
    Stopped,
    /// Core stopped but system proxy restore failed — keep running and surface the warning.
    ProxyRestoreFailed,
    /// Stop failed for another reason — keep running so the user can retry from the UI.
    StopFailed,
    /// Mutex poisoned — cannot safely continue.
    LockPoisoned,
}

/// Tray Quit: serialize with orchestrate, stop core, exit only when proxy state is consistent.
pub fn request_tray_quit<R: Runtime>(app: &AppHandle<R>) -> QuitOutcome {
    let Some(state) = app.try_state::<AppState>() else {
        return QuitOutcome::Stopped;
    };

    match graceful_stop(state.inner()) {
        Ok(()) => QuitOutcome::Stopped,
        Err(err) if err.code == ErrorCode::ProxyRestoreFailed.as_str() => {
            tracing::error!(error = %err, "tray quit: proxy restore failed; staying open");
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }
            QuitOutcome::ProxyRestoreFailed
        }
        Err(err) if err.message.contains("lock poisoned") => {
            tracing::error!(error = %err, "tray quit: lock poisoned");
            QuitOutcome::LockPoisoned
        }
        Err(err) => {
            tracing::error!(error = %err, "tray quit: stop failed; staying open");
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }
            QuitOutcome::StopFailed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ice_config::AppPaths;
    use ice_core::{
        CoreController, CoreError, CoreHandle, CorePaths, CoreState, CoreStatus, HealthEndpoints,
        ImmediateHealthProbe, MockReloader, MockSpawner, ReloadOutcome,
    };
    use ice_proxy_sys::{ProxyBackup, ProxyEndpoints, ProxySysError, SystemProxy};
    use std::cell::Cell;
    use std::path::Path;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct OkProxy;

    impl SystemProxy for OkProxy {
        fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
            Ok(ProxyBackup::default())
        }

        fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
            Ok(())
        }

        fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct FailRestoreProxy {
        restore_calls: Cell<usize>,
    }

    impl SystemProxy for FailRestoreProxy {
        fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
            Ok(ProxyBackup::default())
        }

        fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
            Ok(())
        }

        fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
            self.restore_calls.set(self.restore_calls.get() + 1);
            Err(ProxySysError::RestoreFailed("mock restore fail".into()))
        }
    }

    fn temp_state(label: &str, proxy: Box<dyn SystemProxy>) -> AppState {
        temp_state_with_core(
            label,
            proxy,
            Box::new(CoreController::with_deps(
                MockSpawner::default(),
                ImmediateHealthProbe,
                Box::new(MockReloader::default()),
                Duration::from_millis(20),
                Duration::from_millis(20),
            )) as Box<dyn CoreHandle>,
        )
    }

    fn temp_state_with_core(
        label: &str,
        proxy: Box<dyn SystemProxy>,
        core: Box<dyn CoreHandle>,
    ) -> AppState {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-shutdown-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = AppPaths::new(&dir);
        paths.ensure_dirs().unwrap();
        let system_proxy_available = proxy.is_available();
        AppState {
            paths: paths.clone(),
            core: Mutex::new(core),
            proxy: Mutex::new(proxy),
            orchestrate: Mutex::new(()),
            proxy_recovery_warning: Mutex::new(None),
            proxy_applied_cache: Mutex::new(None),
            system_proxy_available,
            shutdown_requested: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            _instance_lock: crate::test_instance_lock(&paths),
            traffic: ice_core::TrafficMonitor::new(),
        }
    }

    struct FailStopCore {
        status: CoreStatus,
    }

    impl CoreHandle for FailStopCore {
        fn state(&self) -> CoreState {
            CoreState {
                status: self.status,
                message: Some("mock".into()),
                inbound_host: None,
                inbound_port: None,
            }
        }

        fn start(&mut self, _: &CorePaths) -> Result<(), CoreError> {
            Err(CoreError::invalid_state("mock"))
        }

        fn stop(&mut self, _: &Path) -> Result<(), CoreError> {
            Err(CoreError::invalid_state("mock stop failed"))
        }

        fn reload(&mut self, _: &CorePaths) -> Result<ReloadOutcome, CoreError> {
            Err(CoreError::invalid_state("mock"))
        }

        fn needs_proxy_restore(&self) -> bool {
            false
        }

        fn clear_needs_proxy_restore(&mut self) {}

        fn reap_exited_child(&mut self, _: &Path) -> bool {
            false
        }
    }

    use std::sync::Mutex;

    #[test]
    fn graceful_stop_blocks_until_orchestrate_lock_released() {
        let state = Arc::new(temp_state("block", Box::new(OkProxy)));
        let state_bg = state.clone();
        let guard = state.orchestrate.lock().unwrap();

        let handle = thread::spawn(move || {
            let t0 = Instant::now();
            graceful_stop(&state_bg).expect("stop");
            t0.elapsed()
        });

        thread::sleep(Duration::from_millis(40));
        assert!(
            !handle.is_finished(),
            "graceful_stop must wait for orchestrate lock"
        );
        drop(guard);
        let elapsed = handle.join().expect("join");
        assert!(
            elapsed >= Duration::from_millis(30),
            "expected blocking wait, got {elapsed:?}"
        );

        let _ = std::fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn graceful_stop_proxy_restore_failed_sets_warning() {
        let state = temp_state("restore-fail", Box::new(FailRestoreProxy::default()));
        let backup = ice_proxy_sys::ProxyBackupFile {
            applied: true,
            pending_apply: false,
            applied_at: None,
            endpoints: ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            },
            backup: ProxyBackup::default(),
        };
        backup
            .save(&state.paths.proxy_backup())
            .expect("seed backup");

        let err = graceful_stop(&state).expect_err("restore fail");
        assert_eq!(err.code, "proxy.restore_failed");

        let warning = state.proxy_recovery_warning.lock().unwrap().clone();
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("系统代理恢复失败"));

        let _ = std::fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn graceful_stop_leaves_core_stopped() {
        let state = temp_state("stopped", Box::new(OkProxy));
        state.traffic.set_endpoints(Some(HealthEndpoints {
            host: "127.0.0.1".into(),
            port: 9,
        }));
        graceful_stop(&state).expect("stop");
        let core = state.core.lock().unwrap();
        assert_eq!(core.state().status, CoreStatus::Stopped);
        assert!(!state.traffic.has_target());

        let _ = std::fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn graceful_stop_generic_err_detaches_traffic_when_core_not_live() {
        let state = temp_state_with_core(
            "stop-fail-dead",
            Box::new(OkProxy),
            Box::new(FailStopCore {
                status: CoreStatus::Error,
            }),
        );
        state.traffic.set_endpoints(Some(HealthEndpoints {
            host: "127.0.0.1".into(),
            port: 9,
        }));
        assert!(state.traffic.has_target());

        let err = graceful_stop(&state).expect_err("stop fail");
        assert_eq!(err.code, "core.invalid_state");
        assert!(!state.traffic.has_target());

        let _ = std::fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn graceful_stop_generic_err_keeps_traffic_when_core_still_live() {
        let state = temp_state_with_core(
            "stop-fail-live",
            Box::new(OkProxy),
            Box::new(FailStopCore {
                status: CoreStatus::Running,
            }),
        );
        state.traffic.set_endpoints(Some(HealthEndpoints {
            host: "127.0.0.1".into(),
            port: 9,
        }));

        let err = graceful_stop(&state).expect_err("stop fail");
        assert_eq!(err.code, "core.invalid_state");
        assert!(
            state.traffic.has_target(),
            "live core should keep the traffic collector"
        );

        let _ = std::fs::remove_dir_all(state.paths.root());
    }
}
