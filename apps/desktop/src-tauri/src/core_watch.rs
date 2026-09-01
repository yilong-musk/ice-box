//! Background core health checks: reap exited sing-box and restore capture.

use crate::capture::TrafficCapture;
use crate::orchestrate::{current_settings, restore_proxy_after_unexpected_core_exit};
use crate::AppState;
use ice_core::CoreStatus;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

const WATCH_INTERVAL: Duration = Duration::from_secs(2);

/// Reap an unexpectedly exited sing-box child and restore the active capture
/// backend (system proxy via `proxy-backup.json`, or the TUN journal).
///
/// Uses `try_lock` on the orchestrate mutex so start/stop/apply are never blocked by
/// `networksetup` restore or TUN cleanup. Skips when a mutation is in flight (watchdog retries).
pub fn reconcile_unexpected_core_exit(state: &AppState) {
    let reaped = {
        let Ok(_orch) = state.orchestrate.try_lock() else {
            return;
        };
        let Ok(mut core) = state.core.lock() else {
            return;
        };
        core.reap_exited_child(&state.paths.pid())
    };

    if !reaped {
        return;
    }

    state.traffic.set_endpoints(None);

    let needs_restore = state
        .core
        .lock()
        .ok()
        .is_some_and(|core| core.state().status == CoreStatus::Error);

    if !needs_restore {
        return;
    }

    // TUN capture was claimed: run the controller's idempotent release
    // (journal + Diagnostic config) instead of the proxy-backup path.
    if state.capture.active_backend() == TrafficCapture::Tun {
        let Ok(_orch) = state.orchestrate.try_lock() else {
            return;
        };
        let settings = current_settings(&state.paths).unwrap_or_default();
        let warning = {
            let Ok(mut core) = state.core.lock() else {
                return;
            };
            state
                .capture
                .handle_unexpected_core_exit(&mut **core, &settings)
        };
        if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
            *slot = warning;
        }
        return;
    }

    let Ok(proxy) = state.proxy.lock() else {
        return;
    };
    let warning = restore_proxy_after_unexpected_core_exit(&state.paths, proxy.as_ref());
    if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
        *slot = warning;
    }
}

/// While TUN capture is active, self-heal a drifted system DNS after wake /
/// network change: the interface and routes are intact but name resolution is
/// broken (the classic "TUN is on but nothing resolves" after sleep). Uses
/// `try_lock` so a mutation in flight is skipped and retried on the next tick.
fn heal_tun_dns(state: &AppState) {
    if state.capture.active_backend() != TrafficCapture::Tun {
        return;
    }
    let Ok(_orch) = state.orchestrate.try_lock() else {
        return;
    };
    let warning = state.capture.heal_tun_dns();
    if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
        *slot = warning;
    }
}

/// Poll core health for the app lifetime (independent of frontend tab visibility).
pub fn spawn_core_watchdog<R: Runtime>(app: AppHandle<R>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(WATCH_INTERVAL);
        let Some(state) = app.try_state::<AppState>() else {
            break;
        };
        reconcile_unexpected_core_exit(state.inner());
        heal_tun_dns(state.inner());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::CaptureController;
    use ice_config::{write_json_atomic, AppPaths};
    use ice_core::{CoreError, CoreHandle, CorePaths, CoreState, CoreStatus, ReloadOutcome};
    use ice_proxy_sys::{ProxyBackup, ProxyBackupFile, ProxyEndpoints, ProxySysError, SystemProxy};
    use std::cell::Cell;
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    struct TrackProxy {
        restore_calls: Arc<AtomicUsize>,
    }

    impl SystemProxy for TrackProxy {
        fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
            Ok(ProxyBackup::default())
        }

        fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
            Ok(())
        }

        fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
            self.restore_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct MockExitedCore {
        state: CoreState,
        reaped: Cell<bool>,
    }

    impl MockExitedCore {
        fn running() -> Self {
            Self {
                state: CoreState {
                    status: CoreStatus::Running,
                    message: None,
                    inbound_host: Some("127.0.0.1".into()),
                    inbound_port: Some(17890),
                },
                reaped: Cell::new(false),
            }
        }
    }

    impl CoreHandle for MockExitedCore {
        fn state(&self) -> CoreState {
            self.state.clone()
        }

        fn start(&mut self, _: &CorePaths) -> Result<(), CoreError> {
            Err(CoreError::invalid_state("mock"))
        }

        fn stop(&mut self, _: &Path) -> Result<(), CoreError> {
            Ok(())
        }

        fn reload(&mut self, _: &CorePaths) -> Result<ReloadOutcome, CoreError> {
            Err(CoreError::invalid_state("mock"))
        }

        fn needs_proxy_restore(&self) -> bool {
            false
        }

        fn clear_needs_proxy_restore(&mut self) {}

        fn reap_exited_child(&mut self, _: &Path) -> bool {
            if self.reaped.get() {
                return false;
            }
            self.reaped.set(true);
            self.state.status = CoreStatus::Error;
            self.state.message = Some("sing-box exited unexpectedly (code 1)".into());
            self.state.inbound_host = None;
            self.state.inbound_port = None;
            true
        }

        fn adopt_external(&mut self, _pid: u32, _paths: &CorePaths) -> Result<(), CoreError> {
            Err(CoreError::invalid_state("mock adopt unsupported"))
        }

        fn reclaim_orphan_pid(&mut self, _: &Path) -> Result<(), CoreError> {
            Ok(())
        }
    }

    fn temp_state(label: &str, restore_calls: Arc<AtomicUsize>) -> Arc<AppState> {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-core-watch-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = AppPaths::new(&dir);
        paths.ensure_dirs().unwrap();
        Arc::new(AppState {
            paths: paths.clone(),
            core: Mutex::new(Box::new(MockExitedCore::running()) as Box<dyn CoreHandle>),
            proxy: Mutex::new(Box::new(TrackProxy {
                restore_calls: restore_calls.clone(),
            })),
            orchestrate: Mutex::new(()),
            proxy_recovery_warning: Mutex::new(None),
            proxy_applied_cache: Mutex::new(None),
            system_proxy_available: true,
            shutdown_requested: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            _instance_lock: crate::test_instance_lock(&paths),
            traffic: ice_core::TrafficMonitor::new(),
            capture: CaptureController::new(paths.clone(), None),
            profile_cache: Mutex::new(None),
            log_view_cache: Mutex::new(None),
            helper_probe_cache: Mutex::new(None),
            clash_live_mode_cache: Mutex::new(true),
        })
    }

    fn seed_applied_proxy(paths: &AppPaths) {
        let record = ProxyBackupFile {
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
        write_json_atomic(&paths.proxy_backup(), &record).unwrap();
    }

    #[test]
    fn reconcile_restores_proxy_after_unexpected_exit() {
        let restore_calls = Arc::new(AtomicUsize::new(0));
        let state = temp_state("restore", restore_calls.clone());
        seed_applied_proxy(&state.paths);

        reconcile_unexpected_core_exit(state.as_ref());

        assert_eq!(state.core.lock().unwrap().state().status, CoreStatus::Error);
        assert_eq!(restore_calls.load(Ordering::SeqCst), 1);
        let backup = ProxyBackupFile::load(&state.paths.proxy_backup()).unwrap();
        assert!(!backup.applied);
        assert!(state.proxy_recovery_warning.lock().unwrap().is_none());

        let _ = fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn reconcile_skips_when_orchestrate_lock_held() {
        let restore_calls = Arc::new(AtomicUsize::new(0));
        let state = temp_state("skip", restore_calls.clone());
        seed_applied_proxy(&state.paths);

        let state_bg = state.clone();
        let guard = state.orchestrate.lock().unwrap();
        let handle = thread::spawn(move || {
            let t0 = Instant::now();
            reconcile_unexpected_core_exit(state_bg.as_ref());
            t0.elapsed()
        });

        let elapsed = handle.join().expect("join");
        assert!(
            elapsed < Duration::from_millis(50),
            "must not block on orchestrate lock, took {elapsed:?}"
        );
        drop(guard);

        assert_eq!(
            state.core.lock().unwrap().state().status,
            CoreStatus::Running
        );
        assert_eq!(restore_calls.load(Ordering::SeqCst), 0);

        reconcile_unexpected_core_exit(state.as_ref());
        assert_eq!(state.core.lock().unwrap().state().status, CoreStatus::Error);
        assert_eq!(restore_calls.load(Ordering::SeqCst), 1);

        let _ = fs::remove_dir_all(state.paths.root());
    }
}
