// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::capture::CaptureController;
use crate::orchestrate::generate_config;
use ice_config::{set_proxy_service_enabled, AppPaths, NormalizedOutbound};
use ice_engine::{
    load_index, read_profile, write_subscription_success, SubscriptionFormat, SubscriptionMeta,
    SubscriptionPaths,
};
use ice_proxy_sys::is_proxy_applied_on_disk;
use std::fs;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn temp_state_with_node(label: &str) -> AppState {
    let dir = std::env::temp_dir().join(format!(
        "ice-box-cmd-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let paths = AppPaths::new(&dir);
    paths.ensure_dirs().unwrap();
    let sub = SubscriptionPaths::from_app(&paths);
    let id = Uuid::new_v4();
    let meta = SubscriptionMeta {
        id,
        name: "t".into(),
        url: "https://example.com/s".into(),
        active: true,
        format: SubscriptionFormat::SingBox,
        node_count: 1,
        group_count: 0,
        rule_count: 0,
        has_dns: false,
        parse_warnings: vec![],
        last_updated: None,
        last_error: None,
        etag: None,
        last_modified: None,
        userinfo: None,
        provider_info: vec![],
        auto_update: false,
        auto_update_interval: None,
    };
    let nodes = vec![NormalizedOutbound {
        tag: "n1".into(),
        outbound: std::sync::Arc::new(
            serde_json::json!({"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}),
        ),
    }];
    write_subscription_success(
        &sub,
        &meta,
        "{}",
        &ice_config::NormalizedProfile::from_nodes_only(nodes),
    )
    .unwrap();

    let (core, core_snapshot) = crate::core_snapshot::wrap_core(Box::new(
        ice_core::CoreController::default(),
    )
        as Box<dyn ice_core::CoreHandle>);
    AppState {
        paths: paths.clone(),
        core,
        core_snapshot,
        runtime_status: crate::runtime_status::RuntimeReadModel::default(),
        workers: crate::workers::WorkerSupervisor::default(),
        proxy: Mutex::new(Box::new(ice_proxy_sys::NoopSystemProxy)),
        orchestrate: Mutex::new(()),
        proxy_recovery_warning: Mutex::new(Vec::new()),
        proxy_applied_cache: Mutex::new(None),
        system_proxy_available: false,
        shutdown_requested: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        _instance_lock: crate::test_instance_lock(&paths),
        traffic: ice_core::TrafficMonitor::new(),
        capture: CaptureController::new(paths.clone(), None),
        profile_cache: Mutex::new(None),
        profile_parse_cache: std::sync::Arc::new(ice_engine::ProfileCache::new()),
        subscription_watchdog_alive: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
        log_view_cache: Mutex::new(LogViewSlot::default()),
        helper_probe_cache: Mutex::new(None),
        tun_task_cache: Mutex::new(None),
        clash_live_mode_cache: Mutex::new(true),
        launch_proxy_restore_attempted: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            false,
        )),
        live_cache: crate::application::LiveCache::default(),
    }
}

fn temp_state_with_rules(label: &str, rules: Vec<serde_json::Value>) -> AppState {
    let dir = std::env::temp_dir().join(format!(
        "ice-box-cmd-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let paths = AppPaths::new(&dir);
    paths.ensure_dirs().unwrap();
    let sub = SubscriptionPaths::from_app(&paths);
    let id = Uuid::new_v4();
    let meta = SubscriptionMeta {
        id,
        name: "t".into(),
        url: "https://example.com/s".into(),
        active: true,
        format: SubscriptionFormat::SingBox,
        node_count: 1,
        group_count: 0,
        rule_count: rules.len(),
        has_dns: false,
        parse_warnings: vec![],
        last_updated: None,
        last_error: None,
        etag: None,
        last_modified: None,
        userinfo: None,
        provider_info: vec![],
        auto_update: false,
        auto_update_interval: None,
    };
    let mut profile = ice_config::NormalizedProfile::from_nodes_only(vec![NormalizedOutbound {
        tag: "n1".into(),
        outbound: std::sync::Arc::new(
            serde_json::json!({"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}),
        ),
    }]);
    profile.route.rules = rules;
    write_subscription_success(&sub, &meta, "{}", &profile).unwrap();

    let (core, core_snapshot) = crate::core_snapshot::wrap_core(Box::new(
        ice_core::CoreController::default(),
    )
        as Box<dyn ice_core::CoreHandle>);
    AppState {
        paths: paths.clone(),
        core,
        core_snapshot,
        runtime_status: crate::runtime_status::RuntimeReadModel::default(),
        workers: crate::workers::WorkerSupervisor::default(),
        proxy: Mutex::new(Box::new(ice_proxy_sys::NoopSystemProxy)),
        orchestrate: Mutex::new(()),
        proxy_recovery_warning: Mutex::new(Vec::new()),
        proxy_applied_cache: Mutex::new(None),
        system_proxy_available: false,
        shutdown_requested: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        _instance_lock: crate::test_instance_lock(&paths),
        traffic: ice_core::TrafficMonitor::new(),
        capture: CaptureController::new(paths.clone(), None),
        profile_cache: Mutex::new(None),
        profile_parse_cache: std::sync::Arc::new(ice_engine::ProfileCache::new()),
        subscription_watchdog_alive: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
        log_view_cache: Mutex::new(LogViewSlot::default()),
        helper_probe_cache: Mutex::new(None),
        tun_task_cache: Mutex::new(None),
        clash_live_mode_cache: Mutex::new(true),
        launch_proxy_restore_attempted: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            false,
        )),
        live_cache: crate::application::LiveCache::default(),
    }
}

#[test]
fn collect_status_does_not_block_on_core_lock() {
    let state = std::sync::Arc::new(temp_state_with_node("orch1-status"));
    *state.helper_probe_cache.lock().unwrap() = Some((Instant::now(), false));
    let _ = crate::helper_install::helper_core_stale(state.capture.resource_dir());
    let _ = cached_tun_task_ready(&state);
    collect_status(state.as_ref()).expect("warmup");
    let (locked_tx, locked_rx) = std::sync::mpsc::channel();
    let bg = state.clone();
    let handle = std::thread::spawn(move || {
        let _held = bg.core.lock().unwrap();
        locked_tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(3));
    });
    locked_rx.recv().expect("core lock held");
    let t0 = Instant::now();
    collect_status(state.as_ref()).expect("status");
    let elapsed = t0.elapsed();
    assert!(
        state.core.try_lock().is_err(),
        "core lock must still be held; collect_status must not have waited for it"
    );
    // Holder sleeps 3s. 50ms was a false fail under parallel `cargo test`:
    // `load_index` recovered leftover dirs under a process-wide commit lock.
    // Status now reads the index without that lock; 500ms matches the
    // sibling proxy-lock poll budget and is still well under the hold.
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "collect_status waited {elapsed:?}; it must not take the core lock"
    );
    handle.join().expect("holder");
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn launch_restores_proxy_service_only_when_flag_is_on() {
    let state = temp_state_with_node("launch-flag");
    assert!(
        !take_launch_proxy_restore(&state).unwrap(),
        "fresh data dir must not restore capture"
    );
    assert!(!state.launch_proxy_restore_attempted.load(Ordering::SeqCst));

    set_proxy_service_enabled(&state.paths.settings(), true).unwrap();
    assert!(take_launch_proxy_restore(&state).unwrap());
    assert!(
        !take_launch_proxy_restore(&state).unwrap(),
        "StrictMode remount must not restore twice"
    );
    let _ = fs::remove_dir_all(state.paths.root());

    let skipped = temp_state_with_node("launch-flag-off");
    set_proxy_service_enabled(&skipped.paths.settings(), false).unwrap();
    assert!(!take_launch_proxy_restore(&skipped).unwrap());
    assert!(!skipped
        .launch_proxy_restore_attempted
        .load(Ordering::SeqCst));
    let _ = fs::remove_dir_all(skipped.paths.root());
}

#[test]
fn launch_proxy_restore_skips_when_shutting_down_or_already_capturing() {
    let shutting_down = temp_state_with_node("launch-flag-quit");
    set_proxy_service_enabled(&shutting_down.paths.settings(), true).unwrap();
    shutting_down
        .shutdown_requested
        .store(true, Ordering::SeqCst);
    assert!(!take_launch_proxy_restore(&shutting_down).unwrap());
    assert!(!shutting_down
        .launch_proxy_restore_attempted
        .load(Ordering::SeqCst));
    let _ = fs::remove_dir_all(shutting_down.paths.root());

    let already_on = temp_state_with_node("launch-flag-active");
    set_proxy_service_enabled(&already_on.paths.settings(), true).unwrap();
    already_on.capture.set_system_proxy_active().unwrap();
    assert!(!take_launch_proxy_restore(&already_on).unwrap());
    assert!(!already_on
        .launch_proxy_restore_attempted
        .load(Ordering::SeqCst));
    let _ = fs::remove_dir_all(already_on.paths.root());
}

#[test]
fn recover_launch_leftovers_is_a_noop_on_a_fresh_data_dir() {
    let state = temp_state_with_node("launch-recover-fresh");
    recover_launch_leftovers(&state);
    assert!(state.proxy_recovery_warning.lock().unwrap().is_empty());
    assert_eq!(state.capture.active_backend(), TrafficCapture::Inactive);
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn recover_launch_leftovers_clears_applied_proxy_backup_without_enabling_capture() {
    let state = temp_state_with_node("launch-recover-proxy");
    fs::write(
        state.paths.proxy_backup(),
        r#"{
            "applied": true,
            "pending_apply": false,
            "applied_at": null,
            "endpoints": {"http_host": "127.0.0.1", "http_port": 17890},
            "backup": {"enabled": false, "http": null, "https": null, "socks": null, "extra": {}}
        }"#,
    )
    .unwrap();
    recover_launch_leftovers(&state);
    assert!(!is_proxy_applied_on_disk(&state.paths.proxy_backup()));
    assert_eq!(state.capture.active_backend(), TrafficCapture::Inactive);
    assert!(state.proxy_recovery_warning.lock().unwrap().is_empty());
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn parse_proxy_mode_accepts_valid_and_rejects_unknown() {
    assert_eq!(parse_proxy_mode("rule").unwrap(), ProxyMode::Rule);
    assert_eq!(parse_proxy_mode("global").unwrap(), ProxyMode::Global);
    assert_eq!(parse_proxy_mode("direct").unwrap(), ProxyMode::Direct);
    let err = parse_proxy_mode("nope").expect_err("unknown mode");
    assert_eq!(err.code, "config.invalid");
    assert!(err.message.contains("unknown proxy mode"));
}

#[test]
fn proxy_posture_counts_live_recorded_or_tun_as_engaged() {
    // Mirrors Home's `proxyOn`; the tray menu renders the same answer.
    let off = ProxyServicePosture {
        live: Some(false),
        recorded: Some(false),
    };
    assert!(!off.engaged(false));
    assert!(off.engaged(true), "TUN owns capture on its own");
    assert!(ProxyServicePosture {
        live: Some(true),
        recorded: Some(false),
    }
    .engaged(false));
    assert!(
        ProxyServicePosture {
            live: Some(false),
            recorded: Some(true),
        }
        .engaged(false),
        "an on-disk record must stay stoppable"
    );
    let unknown = ProxyServicePosture {
        live: None,
        recorded: None,
    };
    assert!(!unknown.engaged(false), "stopped core is off");
}

#[test]
fn selected_now_reuses_a_fresh_group_head() {
    use std::time::Duration;

    let cache = crate::application::LiveCache::default();
    let endpoints = ice_core::HealthEndpoints::new("127.0.0.1", 9);
    cache.seed_group_heads_for_test(
        endpoints.clone(),
        3,
        vec![ice_core::GroupHead {
            tag: "proxy".into(),
            now: "hk".into(),
        }],
        Duration::from_secs(0),
    );
    // A fresh head must answer without opening a Clash connection.
    assert_eq!(
        crate::application::load_selected_now(&cache, &endpoints, 3, "proxy", true).as_deref(),
        Some("hk")
    );
    assert_eq!(
        crate::application::load_selected_now(&cache, &endpoints, 3, "missing", false).as_deref(),
        None
    );
}

#[test]
fn record_group_now_replaces_a_fresh_exit_without_a_fetch() {
    use std::time::Duration;

    let cache = crate::application::LiveCache::default();
    let endpoints = ice_core::HealthEndpoints::new("127.0.0.1", 9);
    cache.seed_group_heads_for_test(
        endpoints.clone(),
        3,
        vec![
            ice_core::GroupHead {
                tag: "auto".into(),
                now: "a".into(),
            },
            ice_core::GroupHead {
                tag: "proxy".into(),
                now: "hk".into(),
            },
        ],
        Duration::from_secs(0),
    );
    cache.record_group_now(&endpoints, 3, "proxy", "jp");

    assert_eq!(
        crate::application::load_selected_now(&cache, &endpoints, 3, "proxy", true).as_deref(),
        Some("jp")
    );
    let groups =
        crate::application::load_proxy_groups(&cache, &endpoints, 3, true).expect("cached heads");
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].now, "a");
    assert_eq!(groups[1].tag, "proxy");
    assert_eq!(groups[1].now, "jp");
    // A different core generation must not see the patched sample.
    assert_eq!(
        crate::application::load_selected_now(&cache, &endpoints, 4, "proxy", false).as_deref(),
        None
    );
}

#[test]
fn collect_status_snapshots_stopped_core() {
    let state = temp_state_with_node("status");
    let status = collect_status(&state).expect("status");
    assert_eq!(status.core.status, ice_core::CoreStatus::Stopped);
    assert_eq!(status.subscription_count, 1);
    assert_eq!(status.system_proxy_recorded, None);
    assert_eq!(status.system_proxy_applied, None);
    assert!(!status.system_proxy_available);
    assert!(status.has_nodes);
    let outbound = status.selected_outbound.expect("selected outbound");
    assert_eq!(outbound.tag, "n1");
    assert_eq!(outbound.outbound_type, "socks");
    assert_eq!(outbound.group_now, None);
    let again = collect_status(&state).expect("status");
    assert_eq!(again.revision, status.revision);
    assert_eq!(again.sampled_at_ms, status.sampled_at_ms);
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn status_keeps_one_committed_revision_while_a_mutation_is_inflight() {
    let state = temp_state_with_node("status-transaction");
    let before = collect_status(&state).unwrap();
    let guard = lock_orchestrate(&state).unwrap();
    let mut settings = current_settings(&state.paths).unwrap();
    settings.tun.enabled = true;
    persist_settings(&state.paths.settings(), &settings, host_platform()).unwrap();
    let mut core = state.core_snapshot.load().state.clone();
    core.status = CoreStatus::Running;
    state.core_snapshot.publish(core);
    let during = collect_status(&state).unwrap();
    assert_eq!(during.revision, before.revision);
    assert_eq!(during.core.status, before.core.status);
    assert_eq!(during.configured_tun, before.configured_tun);
    assert!(during.refresh_pending);
    drop(guard);
    let after = state.runtime_status.latest().unwrap();
    assert!(after.revision > before.revision);
    assert_eq!(after.core.status, CoreStatus::Running);
    assert!(after.configured_tun);
    assert!(!after.refresh_pending);
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn status_does_not_wait_for_a_slow_probe() {
    let state = temp_state_with_node("status-probe");
    let _probe = state.runtime_status.probe_refresh.lock().unwrap();
    let status = collect_status(&state).unwrap();
    assert!(status.revision > 0);
    assert!(status.diagnostics.stale);
    assert!(status.diagnostics.checked_at_ms.is_none());
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn concurrent_status_reads_do_not_report_a_mutation() {
    let state = Arc::new(temp_state_with_node("parallel-status"));
    let barrier = Arc::new(std::sync::Barrier::new(4));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let state = state.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let status = collect_status(&state).unwrap();
                assert!(!status.refresh_pending);
                status.revision
            })
        })
        .collect();
    let revisions: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(
        revisions.windows(2).all(|pair| pair[0] == pair[1]),
        "quiet parallel reads share one committed revision, got {revisions:?}"
    );
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn a_probe_from_an_older_core_or_settings_is_explicitly_stale() {
    let state = temp_state_with_node("probe-context");
    state.runtime_status.complete_probe(
        0,
        Ok(crate::runtime_status::ProbeValues {
            core_generation: state.core_snapshot.load().generation,
            settings_signature: file_sig(&state.paths.settings()),
            ..Default::default()
        }),
        Instant::now(),
    );
    assert!(!collect_status(&state).unwrap().diagnostics.stale);
    state
        .core_snapshot
        .publish(state.core_snapshot.load().state.clone());
    assert!(collect_status(&state).unwrap().diagnostics.stale);
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn system_proxy_applied_does_not_blink_while_a_probe_refresh_is_due_or_requested() {
    use crate::runtime_status::{ProbeValues, PROBE_INTERVAL, PROBE_MAX_AGE};
    let mut state = temp_state_with_node("probe-grace");
    state.system_proxy_available = true;
    let mut core = state.core_snapshot.load().state.clone();
    core.status = CoreStatus::Running;
    state.core_snapshot.publish(core);
    let sample = |state: &AppState, at: Instant| {
        let values = ProbeValues {
            core_generation: state.core_snapshot.load().generation,
            settings_signature: file_sig(&state.paths.settings()),
            system_proxy_applied: Some(true),
            ..ProbeValues::default()
        };
        let epoch = state.runtime_status.probe_epoch();
        state.runtime_status.complete_probe(epoch, Ok(values), at)
    };

    // One interval old: the worker's refresh is due, but Home still reads the
    // value instead of dropping to `None` for the length of the probe.
    let aged = Instant::now()
        .checked_sub(PROBE_INTERVAL + std::time::Duration::from_millis(100))
        .expect("monotonic clock");
    sample(&state, aged);
    assert!(state.runtime_status.begin_refresh(Instant::now()).is_some());
    let status = collect_status(&state).unwrap();
    assert_eq!(status.system_proxy_applied, Some(true));
    assert!(!status.diagnostics.stale);

    // Past the serving window the sample is withheld.
    let expired = Instant::now()
        .checked_sub(PROBE_MAX_AGE + std::time::Duration::from_millis(100))
        .expect("monotonic clock");
    sample(&state, expired);
    assert_eq!(collect_status(&state).unwrap().system_proxy_applied, None);

    // A refresh request (window focus) keeps the current sample; a mutation's
    // invalidation withholds it.
    sample(&state, Instant::now());
    state.runtime_status.request_refresh();
    assert_eq!(
        collect_status(&state).unwrap().system_proxy_applied,
        Some(true)
    );
    state.runtime_status.invalidate_probes();
    assert_eq!(collect_status(&state).unwrap().system_proxy_applied, None);
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn application_ports_work_without_a_tauri_runtime() {
    struct Host {
        notifications: std::cell::Cell<usize>,
    }
    impl AppResources for Host {
        fn resource_dir(&self) -> Option<std::path::PathBuf> {
            Some("bundled-resources".into())
        }
    }
    impl AppHost for Host {
        fn state_changed(&self) {
            self.notifications.set(self.notifications.get() + 1);
        }
    }
    let host = Host {
        notifications: std::cell::Cell::new(0),
    };
    assert_eq!(resource_dir(&host), Some("bundled-resources".into()));
    broadcast_state_change(&host);
    assert_eq!(host.notifications.get(), 1);
    let state = temp_state_with_node("headless-use-case");
    let subscriptions = list_subscriptions_use_case(&state).unwrap();
    assert_eq!(subscriptions.as_array().unwrap().len(), 1);
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn collect_status_reports_app_memory_and_skips_a_stopped_core() {
    let state = temp_state_with_node("memory-stopped");
    let status = collect_status(&state).expect("status");
    let app = status
        .memory
        .app_bytes
        .expect("app memory must be readable");
    assert!(app > 0, "app memory must be positive, got {app}");
    assert_eq!(status.memory.core_bytes, None);
    assert_eq!(status.memory.total_bytes, app);
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn collect_status_reads_core_memory_from_a_live_pid() {
    let state = temp_state_with_node("memory-running");
    // The test process stands in for the core: a live pid whose memory figure
    // is readable without elevation.
    ice_core::write_pid(&state.paths.pid(), std::process::id()).expect("pid file");
    let mut core = state.core_snapshot.load().state.clone();
    core.status = ice_core::CoreStatus::Running;
    state.core_snapshot.publish(core);

    let status = collect_status(&state).expect("status");
    let core_bytes = status.memory.core_bytes.expect("core memory");
    assert!(
        core_bytes > 0,
        "core memory must be positive, got {core_bytes}"
    );
    let app = status.memory.app_bytes.expect("app memory");
    assert_eq!(status.memory.total_bytes, app + core_bytes);
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn collect_status_skips_core_memory_without_a_usable_pid_file() {
    let state = temp_state_with_node("memory-no-pid");
    let mut core = state.core_snapshot.load().state.clone();
    core.status = ice_core::CoreStatus::Running;
    state.core_snapshot.publish(core);

    // Running, but no pid file yet (startup window): the core figure stays
    // unknown instead of erroring the poll.
    let status = collect_status(&state).expect("status");
    assert_eq!(status.memory.core_bytes, None);

    fs::write(state.paths.pid(), b"not-a-pid").expect("seed");
    let status = collect_status(&state).expect("status");
    assert_eq!(status.memory.core_bytes, None);
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn collect_status_does_not_block_on_held_proxy_lock() {
    let state = temp_state_with_node("proxy-held");
    // Warm one-time / first-probe work outside the measured window: SHA-256
    // of the bundled core (macOS helper drift) and the Windows scheduled-
    // task pin probe (`schtasks /Query /XML`, easily >500ms on CI). The
    // poll itself must stay cheap and must not wait on `state.proxy`.
    let _ = crate::helper_install::helper_core_stale(state.capture.resource_dir());
    let _ = cached_tun_task_ready(&state);
    let _guard = state.proxy.lock().unwrap();
    let started = std::time::Instant::now();
    let status = collect_status(&state).expect("status");
    assert!(
        started.elapsed() < std::time::Duration::from_millis(500),
        "status poll must not wait on system-proxy apply/restore"
    );
    assert_eq!(status.core.status, ice_core::CoreStatus::Stopped);
    assert!(
        !status.system_proxy_available,
        "Noop backend must not flip to available while proxy lock is held"
    );
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn cached_system_proxy_applied_ignores_expired_memo_when_proxy_busy() {
    let state = temp_state_with_node("stale-cache");
    let settings = ice_config::AppSettings::default();
    let endpoints = endpoints_from_settings(&settings);
    {
        let mut cache = state.proxy_applied_cache.lock().unwrap();
        *cache = Some((
            endpoints.clone(),
            std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(60))
                .expect("monotonic clock"),
            true,
        ));
    }
    let _guard = state.proxy.lock().unwrap();
    assert_eq!(
        cached_system_proxy_applied(&state, &settings),
        None,
        "expired memo must not be served while apply/restore holds the proxy lock"
    );
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn cached_system_proxy_applied_serves_fresh_memo_without_proxy_lock() {
    let state = temp_state_with_node("fresh-cache");
    let settings = ice_config::AppSettings::default();
    let endpoints = endpoints_from_settings(&settings);
    {
        let mut cache = state.proxy_applied_cache.lock().unwrap();
        *cache = Some((endpoints, std::time::Instant::now(), true));
    }
    let _guard = state.proxy.lock().unwrap();
    assert_eq!(cached_system_proxy_applied(&state, &settings), Some(true));
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn require_known_node_tag_rejects_unknown() {
    let state = temp_state_with_node("tag");
    let err = require_known_node_tag(&state, "missing").expect_err("unknown tag");
    assert_eq!(err.code, "config.invalid");
    assert!(err.message.contains("unknown node tag"));
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn require_known_node_tag_accepts_merged_node() {
    let state = temp_state_with_node("ok");
    require_known_node_tag(&state, "n1").expect("known tag");
    let first = active_profile(&state).unwrap();
    let second = active_profile(&state).unwrap();
    assert!(
        Arc::ptr_eq(&first, &second),
        "read paths must share the cached profile allocation"
    );
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn profile_cache_serves_unchanged_and_invalidates_on_update() {
    let state = temp_state_with_node("cache");
    let first = merged_outbounds_opt(&state).unwrap().unwrap();
    assert_eq!(first[0].tag, "n1");
    // mtime unchanged: the second read must come from the cache.
    let second = merged_outbounds_opt(&state).unwrap().unwrap();
    assert_eq!(second[0].tag, "n1");
    assert!(
        state.profile_cache.lock().unwrap().is_some(),
        "cache must be populated after a read"
    );

    // Simulate a subscription update: profile.json + index.json are
    // rewritten atomically, which must invalidate the cached entry.
    let sub = SubscriptionPaths::from_app(&state.paths);
    let index = load_index(&sub).unwrap();
    let meta = ice_engine::active_subscription(&index).unwrap().clone();
    let nodes = vec![NormalizedOutbound {
        tag: "n2".into(),
        outbound: std::sync::Arc::new(
            serde_json::json!({"type":"socks","tag":"n2","server":"2.2.2.2","server_port":1}),
        ),
    }];
    write_subscription_success(
        &sub,
        &meta,
        "{}",
        &ice_config::NormalizedProfile::from_nodes_only(nodes),
    )
    .unwrap();
    let updated = merged_outbounds_opt(&state).unwrap().unwrap();
    assert_eq!(updated[0].tag, "n2", "stale cache must not be served");
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn resident_profile_omits_rules_until_the_rules_page_loads_them() {
    let state = temp_state_with_rules("resident-rules", sample_rules());
    let entry = cached_profile(&state).unwrap().unwrap();
    assert!(
        entry.profile.route.rules.is_empty(),
        "status and the tray must not retain route rules"
    );
    assert!(entry.profile.dns.is_none());
    assert!(
        entry.profile.nodes[0].outbound.get("server").is_none(),
        "status and the tray must not retain node connection fields"
    );
    assert_eq!(
        entry.profile.nodes[0]
            .outbound
            .get("type")
            .and_then(|value| value.as_str()),
        Some("socks")
    );
    assert!(
        !subscription_rules_retained(&state),
        "rules stay on disk until the Rules page reads them"
    );

    let overview = rule_overview(&state).unwrap();
    assert_eq!(overview.total, 4);
    assert!(subscription_rules_retained(&state));
    let still = cached_profile(&state).unwrap().unwrap();
    assert!(still.profile.route.rules.is_empty());

    drop_rule_keyword_cache(&state);
    assert!(!subscription_rules_retained(&state));
    let listed = query_rules(
        &state,
        &ListRulesRequest {
            keyword: None,
            rule_type: None,
            disabled: None,
            custom: None,
            offset: 0,
            limit: 10,
        },
    )
    .unwrap();
    assert_eq!(listed.total, 4);
    assert!(subscription_rules_retained(&state));
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn config_build_reloads_connection_fields_then_releases_them() {
    let state = temp_state_with_node("resident-build");
    let entry = cached_profile(&state).unwrap().unwrap();
    assert!(entry.profile.nodes[0].outbound.get("server").is_none());

    let settings = ice_config::load_settings(&state.paths.settings()).unwrap();
    crate::orchestrate::generate_config_with_cache(
        &state.paths,
        &settings,
        None,
        ice_config::CaptureIntent::Diagnostic,
        Some(state.profile_parse_cache.as_ref()),
    )
    .unwrap();

    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(state.paths.config()).unwrap()).unwrap();
    let node = config["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|outbound| outbound["tag"] == "n1")
        .expect("generated node");
    assert_eq!(node["server"], "1.1.1.1");

    let after = cached_profile(&state).unwrap().unwrap();
    assert!(after.profile.nodes[0].outbound.get("server").is_none());
    assert_eq!(
        after.profile.nodes[0]
            .outbound
            .get("type")
            .and_then(|value| value.as_str()),
        Some("socks")
    );
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn validate_static_group_member_accepts_member() {
    let outbounds = vec![NormalizedOutbound {
        tag: "Proxies".into(),
        outbound: std::sync::Arc::new(serde_json::json!({
            "type": "selector",
            "tag": "Proxies",
            "outbounds": ["n1", "n2"],
        })),
    }];
    validate_static_group_member(&outbounds, "Proxies", "n2").expect("member");
}

#[test]
fn validate_static_group_member_rejects_unknown_group() {
    let outbounds = vec![NormalizedOutbound {
        tag: "Proxies".into(),
        outbound: std::sync::Arc::new(serde_json::json!({"type": "selector", "outbounds": ["n1"]})),
    }];
    let err = validate_static_group_member(&outbounds, "missing", "n1").expect_err("unknown");
    assert_eq!(err.code, "config.invalid");
    assert!(err.message.contains("unknown strategy group"));
}

#[test]
fn validate_static_group_member_rejects_non_member() {
    let outbounds = vec![NormalizedOutbound {
        tag: "Proxies".into(),
        outbound: std::sync::Arc::new(serde_json::json!({"type": "selector", "outbounds": ["n1"]})),
    }];
    let err = validate_static_group_member(&outbounds, "Proxies", "nope").expect_err("non member");
    assert_eq!(err.code, "config.invalid");
    assert!(err.message.contains("is not a member"));
}

#[test]
fn validate_static_group_member_rejects_non_selector() {
    let outbounds = vec![NormalizedOutbound {
        tag: "auto".into(),
        outbound: std::sync::Arc::new(serde_json::json!({"type": "urltest", "outbounds": ["n1"]})),
    }];
    let err = validate_static_group_member(&outbounds, "auto", "n1").expect_err("not selector");
    assert_eq!(err.code, "config.invalid");
    assert!(err.message.contains("is not a selector group"));
}

#[test]
fn selection_group_for_flat_profile_is_none() {
    let profile = NormalizedProfile::from_nodes_only(vec![NormalizedOutbound {
        tag: "n1".into(),
        outbound: std::sync::Arc::new(serde_json::json!({"type": "socks", "tag": "n1"})),
    }]);
    assert_eq!(selection_group_for(&profile, "n1"), None);
}

#[test]
fn selection_group_for_prefers_top_level_group() {
    let profile = NormalizedProfile {
        nodes: vec![NormalizedOutbound {
            tag: "HK".into(),
            outbound: std::sync::Arc::new(serde_json::json!({"type": "socks", "tag": "HK"})),
        }],
        groups: vec![
            NormalizedOutbound {
                tag: "Proxies".into(),
                outbound: std::sync::Arc::new(serde_json::json!({
                    "type": "selector",
                    "tag": "Proxies",
                    "outbounds": ["auto", "HK", "direct"],
                })),
            },
            NormalizedOutbound {
                tag: "auto".into(),
                outbound: std::sync::Arc::new(serde_json::json!({
                    "type": "urltest",
                    "tag": "auto",
                    "outbounds": ["HK", "JP"],
                })),
            },
        ],
        route: Default::default(),
        dns: None,
        default_outbound: Some("Proxies".into()),
        parse_stats: Default::default(),
    };
    assert_eq!(
        selection_group_for(&profile, "HK").as_deref(),
        Some("Proxies"),
        "leaf in top group selects there"
    );
    assert_eq!(
        selection_group_for(&profile, "auto").as_deref(),
        Some("Proxies"),
        "sub-group in top group selects there"
    );
    assert_eq!(
        selection_group_for(&profile, "JP").as_deref(),
        Some("auto"),
        "leaf only in sub-group selects there"
    );
    assert_eq!(
        selection_group_for(&profile, "Proxies").as_deref(),
        None,
        "selecting the top group itself is a no-op"
    );
    assert!(is_unselectable_group(&profile, "Proxies"));
    assert!(!is_unselectable_group(&profile, "auto"));
    assert!(!is_unselectable_group(&profile, "HK"));
}

fn sample_rules() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({ "domain_suffix": ["youtube.com"], "outbound": "n1" }),
        serde_json::json!({ "domain_suffix": ["google.com"], "outbound": "n1" }),
        serde_json::json!({ "geoip": ["cn"], "outbound": "direct" }),
        serde_json::json!({ "ip_is_private": true, "outbound": "direct" }),
    ]
}

#[test]
fn list_rules_returns_all_with_indexes_and_filters() {
    let state = temp_state_with_rules("rules-all", sample_rules());
    let resp = query_rules(
        &state,
        &ListRulesRequest {
            keyword: None,
            rule_type: None,
            disabled: None,
            custom: None,
            offset: 0,
            limit: 2,
        },
    )
    .unwrap();
    assert_eq!(resp.total, 4);
    assert_eq!(resp.items.len(), 2);
    assert_eq!(resp.items[0].index, Some(0));
    assert_eq!(resp.items[1].index, Some(1));

    let filtered = query_rules(
        &state,
        &ListRulesRequest {
            keyword: Some("geo".into()),
            rule_type: None,
            disabled: None,
            custom: None,
            offset: 0,
            limit: 50,
        },
    )
    .unwrap();
    assert_eq!(filtered.total, 1);
    assert_eq!(filtered.items[0].rule_type, "geoip");

    let typed = query_rules(
        &state,
        &ListRulesRequest {
            keyword: None,
            rule_type: Some("domain_suffix".into()),
            disabled: None,
            custom: None,
            offset: 1,
            limit: 50,
        },
    )
    .unwrap();
    assert_eq!(typed.total, 2);
    assert_eq!(typed.items[0].index, Some(1));
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn rule_overview_counts_types_and_disabled() {
    let state = temp_state_with_rules("rules-overview", sample_rules());
    let fp = rule_fingerprint(&sample_rules()[0]);
    let mut overrides = load_rule_overrides(&state.paths.rule_overrides());
    overrides.set_disabled(fp, true);
    save_rule_overrides(&state.paths.rule_overrides(), &overrides).unwrap();

    let overview = rule_overview(&state).unwrap();
    assert_eq!(overview.total, 4);
    assert_eq!(overview.disabled, 1);
    assert_eq!(overview.rule_sets, 0);
    assert_eq!(overview.custom, 0);
    let suffix = overview
        .types
        .iter()
        .find(|t| t.rule_type == "domain_suffix")
        .unwrap();
    assert_eq!(suffix.count, 2);
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn set_rule_disabled_persists_and_generates_config_without_rule() {
    let state = temp_state_with_rules("rules-disable", sample_rules());
    let fp = rule_fingerprint(&sample_rules()[0]);
    persist_rule_disabled(
        &state,
        &SetRuleDisabledRequest {
            fingerprint: fp.clone(),
            disabled: true,
        },
    )
    .unwrap();

    generate_config(
        &state.paths,
        &AppSettings::default(),
        None,
        CaptureIntent::Diagnostic,
    )
    .unwrap();
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(state.paths.config()).unwrap()).unwrap();
    let rules = config["route"]["rules"].as_array().unwrap();
    assert_eq!(
        rules.len(),
        5,
        "2 clash_mode rules + 3 remaining sample rules after the disabled one dropped"
    );
    assert!(!serde_json::to_string(rules)
        .unwrap()
        .contains("youtube.com"));

    let listed = query_rules(
        &state,
        &ListRulesRequest {
            keyword: None,
            rule_type: None,
            disabled: Some("disabled".into()),
            custom: None,
            offset: 0,
            limit: 50,
        },
    )
    .unwrap();
    assert_eq!(listed.total, 1);
    assert_eq!(listed.items[0].fingerprint, fp);
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn set_rule_disabled_rejects_unknown_fingerprint() {
    let state = temp_state_with_rules("rules-unknown", sample_rules());
    let err = persist_rule_disabled(
        &state,
        &SetRuleDisabledRequest {
            fingerprint: "nope".into(),
            disabled: true,
        },
    )
    .expect_err("unknown fingerprint");
    assert_eq!(err.code, "config.invalid");
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn add_remove_custom_rule_round_trip_and_validation() {
    let state = temp_state_with_rules("rules-custom", sample_rules());
    let custom = serde_json::json!({ "domain": ["example.com"], "outbound": "block" });
    let fp = persist_add_custom_rule(
        &state,
        &AddCustomRuleRequest {
            rule: custom.clone(),
        },
    )
    .unwrap();

    let listed = query_rules(
        &state,
        &ListRulesRequest {
            keyword: Some("example".into()),
            rule_type: None,
            disabled: None,
            custom: None,
            offset: 0,
            limit: 50,
        },
    )
    .unwrap();
    assert_eq!(listed.total, 1);
    assert!(listed.items[0].custom);
    assert_eq!(listed.items[0].index, None);

    let custom_only = query_rules(
        &state,
        &ListRulesRequest {
            keyword: None,
            rule_type: None,
            disabled: None,
            custom: Some(true),
            offset: 0,
            limit: 50,
        },
    )
    .unwrap();
    assert_eq!(custom_only.total, 1);
    assert!(custom_only.items[0].custom);

    let subscription_only = query_rules(
        &state,
        &ListRulesRequest {
            keyword: None,
            rule_type: None,
            disabled: None,
            custom: Some(false),
            offset: 0,
            limit: 50,
        },
    )
    .unwrap();
    assert_eq!(subscription_only.total, 4);
    assert!(subscription_only.items.iter().all(|r| !r.custom));

    let overview = rule_overview(&state).unwrap();
    assert_eq!(overview.custom, 1);

    persist_remove_custom_rule(&state, &RemoveCustomRuleRequest { fingerprint: fp }).unwrap();

    let overview = rule_overview(&state).unwrap();
    assert_eq!(overview.custom, 0);

    let err = persist_add_custom_rule(
        &state,
        &AddCustomRuleRequest {
            rule: serde_json::json!("not-an-object"),
        },
    )
    .expect_err("non object");
    assert_eq!(err.code, "config.invalid");

    let err = persist_add_custom_rule(
        &state,
        &AddCustomRuleRequest {
            rule: serde_json::json!({ "domain": ["x.com"] }),
        },
    )
    .expect_err("missing outbound");
    assert_eq!(err.code, "config.invalid");
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn add_custom_rule_rejects_geoip_and_geosite_matchers() {
    let state = temp_state_with_rules("rules-custom-geo", sample_rules());
    for key in ["geoip", "geosite"] {
        let err = persist_add_custom_rule(
            &state,
            &AddCustomRuleRequest {
                rule: serde_json::json!({ key: ["cn"], "outbound": "direct" }),
            },
        )
        .expect_err(&format!("{key} must be rejected"));
        assert_eq!(err.code, "config.invalid");
        assert!(
            err.message.contains(key),
            "message should name the matcher: {err}"
        );
    }
    let overrides = load_rule_overrides(&state.paths.rule_overrides());
    assert_eq!(overrides.custom.len(), 0, "nothing persisted");
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn add_custom_rule_validates_rule_set_references() {
    let state = temp_state_with_rules("rules-custom-set", vec![]);
    let sub = SubscriptionPaths::from_app(&state.paths);
    let index = load_index(&sub).unwrap();
    let id = index.items[0].id;
    let mut profile = read_profile(&sub, id).unwrap();
    profile.route.rule_sets = vec![serde_json::json!({
        "type": "remote",
        "tag": "cn",
        "url": "https://example.com/cn.srs",
    })];
    fs::write(sub.profile(id), serde_json::to_vec(&profile).unwrap()).unwrap();

    let fp = persist_add_custom_rule(
        &state,
        &AddCustomRuleRequest {
            rule: serde_json::json!({ "rule_set": ["cn"], "outbound": "direct" }),
        },
    )
    .expect("known rule_set accepted");
    assert!(!fp.is_empty());

    let err = persist_add_custom_rule(
        &state,
        &AddCustomRuleRequest {
            rule: serde_json::json!({ "rule_set": ["missing"], "outbound": "direct" }),
        },
    )
    .expect_err("unknown rule_set");
    assert_eq!(err.code, "config.invalid");
    assert!(err.message.contains("unknown rule_set"));
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn custom_rule_disabled_dropped_from_runtime_config() {
    let state = temp_state_with_rules("rules-custom-off", sample_rules());
    let custom = serde_json::json!({ "domain": ["blockme.com"], "outbound": "block" });
    let fp = rule_fingerprint(&custom);
    let mut overrides = load_rule_overrides(&state.paths.rule_overrides());
    overrides.custom.push(custom);
    overrides.set_disabled(fp, true);
    save_rule_overrides(&state.paths.rule_overrides(), &overrides).unwrap();

    generate_config(
        &state.paths,
        &AppSettings::default(),
        None,
        CaptureIntent::Diagnostic,
    )
    .unwrap();
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(state.paths.config()).unwrap()).unwrap();
    let rules = config["route"]["rules"].as_array().unwrap();
    assert!(!serde_json::to_string(rules)
        .unwrap()
        .contains("blockme.com"));
    let _ = fs::remove_dir_all(state.paths.root());
}

#[test]
fn only_launch_at_login_changed_ignores_other_fields() {
    use crate::application::settings::only_launch_at_login_changed;

    let off = AppSettings::default();
    let on = AppSettings {
        launch_at_login: true,
        ..AppSettings::default()
    };
    assert!(only_launch_at_login_changed(&off, &on));
    // A save that also touches anything else is a full apply, not this
    // persist-only path.
    let mut other = on.clone();
    other.mixed_port = 18080;
    assert!(!only_launch_at_login_changed(&off, &other));
    // Unchanged login item: nothing to do.
    assert!(!only_launch_at_login_changed(&off, &off));
}

#[test]
fn only_tray_display_mode_changed_ignores_other_fields() {
    use crate::application::settings::only_tray_display_mode_changed;

    let both = AppSettings::default();
    let speed = AppSettings {
        tray_display_mode: TrayDisplayMode::Speed,
        ..AppSettings::default()
    };
    assert!(only_tray_display_mode_changed(&both, &speed));
    // A save that also touches anything else is a full apply, not this
    // persist-only path.
    let mut other = speed.clone();
    other.mixed_port = 18080;
    assert!(!only_tray_display_mode_changed(&both, &other));
    // Unchanged mode: nothing to do.
    assert!(!only_tray_display_mode_changed(&both, &both));
}
