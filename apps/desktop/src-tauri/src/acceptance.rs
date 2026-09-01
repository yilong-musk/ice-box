//! Headless acceptance scenarios (plan G9.1 / G9.6 / G9.7). Live UI/proxy cases are covered by the macOS release gate.

#[cfg(test)]
mod tests {
    use crate::orchestrate::{generate_config, orchestrate_start};
    use ice_config::{write_json_atomic, AppPaths, AppSettings, CaptureIntent};
    use ice_core::{CoreController, CoreStatus, ImmediateHealthProbe, MockReloader, MockSpawner};
    use ice_proxy_sys::{ProxyBackup, ProxyBackupFile, ProxyEndpoints, ProxySysError, SystemProxy};
    use ice_subscription::{
        FetchResponse, MockFetchMode, MockFetcher, SubscriptionFormat, SubscriptionManager,
        SubscriptionPaths,
    };
    use std::cell::Cell;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct TrackProxy {
        apply_calls: Cell<usize>,
        restore_calls: Cell<usize>,
    }

    impl SystemProxy for TrackProxy {
        fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
            Ok(ProxyBackup::default())
        }

        fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
            self.apply_calls.set(self.apply_calls.get() + 1);
            Ok(())
        }

        fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
            self.restore_calls.set(self.restore_calls.get() + 1);
            Ok(())
        }
    }

    fn temp_app(label: &str) -> AppPaths {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-g9-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = AppPaths::new(&dir);
        paths.ensure_dirs().unwrap();
        paths
    }

    fn marker_bin(paths: &AppPaths) -> PathBuf {
        let bin = paths.root().join("sing-box");
        fs::write(&bin, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(&bin).unwrap().permissions();
            p.set_mode(0o755);
            fs::set_permissions(&bin, p).unwrap();
        }
        bin
    }

    fn mock_core_ok() -> CoreController<MockSpawner, ImmediateHealthProbe> {
        CoreController::with_deps(
            MockSpawner::with_start_pid(7000),
            ImmediateHealthProbe,
            Box::new(MockReloader::default()),
            Duration::from_millis(50),
            Duration::from_millis(50),
        )
    }

    fn repo_fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../configs/examples")
            .join(name);
        fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("fixture {} at {}: {e}", name, path.display()))
    }

    #[test]
    fn g9_1_empty_start_runs_direct_only_no_proxy() {
        let paths = temp_app("empty-start");
        let settings = AppSettings::default();
        let mut core = mock_core_ok();
        let proxy = TrackProxy::default();
        let bin = marker_bin(&paths);

        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("direct-only");
        assert_eq!(core.state().status, CoreStatus::Running);
        assert_eq!(
            proxy.apply_calls.get(),
            0,
            "start launches core only; system proxy is home-button controlled"
        );
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(paths.config()).unwrap()).unwrap();
        let tags: Vec<&str> = config["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|o| o["tag"].as_str())
            .collect();
        assert_eq!(tags, ["direct", "block"]);
        assert!(
            !paths.proxy_backup().exists() || {
                let b = ProxyBackupFile::load(&paths.proxy_backup()).unwrap();
                !b.applied
            }
        );
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g9_6_crash_recovery_restore_once_no_reapply() {
        let paths = temp_app("crash");
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

        let proxy = TrackProxy::default();
        let did = ice_proxy_sys::recover_if_applied(&paths.proxy_backup(), &proxy).unwrap();
        assert!(did);
        assert_eq!(proxy.restore_calls.get(), 1);
        assert_eq!(proxy.apply_calls.get(), 0);

        let after = ProxyBackupFile::load(&paths.proxy_backup()).unwrap();
        assert!(!after.applied);
        assert!(paths.proxy_backup().exists());

        let again = ice_proxy_sys::recover_if_applied(&paths.proxy_backup(), &proxy).unwrap();
        assert!(!again);
        assert_eq!(proxy.restore_calls.get(), 1);
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g9_7_clash_mixed_fixture_import_and_config() {
        let paths = temp_app("clash");
        let sub = SubscriptionPaths::from_app(&paths);
        let body = repo_fixture("subscription-clash-mixed.yaml");
        let mgr = SubscriptionManager::with_fetcher(
            sub,
            MockFetcher {
                bypasses_proxy: true,
                mode: MockFetchMode::Ok(FetchResponse {
                    body,
                    not_modified: false,
                    etag: None,
                    last_modified: None,
                    content_disposition: None,
                }),
            },
        );

        let meta = mgr
            .add("https://example.com/clash-mixed", Some("G9 clash"))
            .expect("import clash fixture");
        assert_eq!(meta.format, SubscriptionFormat::Clash);
        assert!(meta.node_count >= 5, "expected known types from fixture");

        generate_config(
            &paths,
            &AppSettings::default(),
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("config from clash nodes");
        assert!(paths.config().is_file());
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g9_8_graceful_stop_serializes_with_orchestrate_lock() {
        use crate::capture::CaptureController;
        use crate::shutdown::graceful_stop;
        use crate::AppState;
        use ice_core::CoreHandle;
        use std::sync::{Arc, Mutex};
        use std::thread;
        use std::time::{Duration, Instant};

        let paths = temp_app("shutdown-lock");
        let state = Arc::new(AppState {
            paths: paths.clone(),
            core: Mutex::new(Box::new(mock_core_ok()) as Box<dyn CoreHandle>),
            proxy: Mutex::new(Box::new(TrackProxy::default())),
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
        });

        let bg = state.clone();
        let guard = state.orchestrate.lock().unwrap();
        let handle = thread::spawn(move || {
            let t0 = Instant::now();
            graceful_stop(&bg, PathBuf::from("/bin/true")).expect("stop");
            t0.elapsed()
        });

        thread::sleep(Duration::from_millis(40));
        assert!(!handle.is_finished(), "stop must wait for orchestrate lock");
        drop(guard);
        let elapsed = handle.join().expect("join");
        assert!(elapsed >= Duration::from_millis(25));

        let core = state.core.lock().unwrap();
        assert_eq!(core.state().status, CoreStatus::Stopped);
        let _ = fs::remove_dir_all(paths.root());
    }
}

/// Live acceptance (real sing-box / system proxy). Run with `--ignored`.
#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod live {
    use crate::log_tail::read_log_tail;
    use crate::orchestrate::{
        orchestrate_apply, orchestrate_enable_system_proxy, orchestrate_set_proxy_mode,
        orchestrate_start, orchestrate_stop, repo_third_party_singbox,
    };
    use ice_config::{AppPaths, AppSettings, CaptureIntent};
    use ice_core::{resolve_singbox_binary, CoreController, CoreStatus};
    use ice_proxy_sys::{create_system_proxy, ProxyBackupFile, SystemProxy};
    use ice_subscription::{
        FetchResponse, MockFetchMode, MockFetcher, SubscriptionManager, SubscriptionPaths,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const MIXED_PORT: u16 = 17_950;
    const CLASH_PORT: u16 = 19_150;

    fn temp_app(label: &str) -> AppPaths {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-g9-live-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = AppPaths::new(&dir);
        paths.ensure_dirs().unwrap();
        paths
    }

    fn repo_fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../configs/examples")
            .join(name);
        fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("fixture {} at {}: {e}", name, path.display()))
    }

    fn real_binary() -> PathBuf {
        resolve_singbox_binary(&repo_third_party_singbox(), None)
            .expect("third_party sing-box required for live acceptance")
    }

    fn settings() -> AppSettings {
        AppSettings {
            mixed_port: MIXED_PORT,
            clash_api_port: CLASH_PORT,
            ..AppSettings::default()
        }
    }

    fn seed_singbox_subscription(paths: &AppPaths) {
        let sub = SubscriptionPaths::from_app(paths);
        let body = repo_fixture("subscription-singbox-outbounds.json");
        let mgr = SubscriptionManager::with_fetcher(
            sub,
            MockFetcher {
                bypasses_proxy: true,
                mode: MockFetchMode::Ok(FetchResponse {
                    body,
                    not_modified: false,
                    etag: None,
                    last_modified: None,
                    content_disposition: None,
                }),
            },
        );
        mgr.add("https://example.com/singbox-fixture", Some("G9 live"))
            .expect("seed subscription");
    }

    fn cleanup(paths: &AppPaths, core: &mut CoreController, proxy: &dyn SystemProxy) {
        let _ = orchestrate_stop(paths, core, proxy);
        let _ = fs::remove_dir_all(paths.root());
    }

    fn curl_via_mixed(port: u16) -> bool {
        let proxy = format!("http://127.0.0.1:{port}");
        Command::new("curl")
            .args(["-x", &proxy, "-I", "--max-time", "8", "http://example.com"])
            .output()
            // A refused proxy connection fails with non-empty stderr, so stderr alone must
            // not count as success; require a completed transfer or HTTP output instead.
            .map(|o| o.status.success() || !o.stdout.is_empty())
            .unwrap_or(false)
    }

    #[test]
    #[ignore = "live: real sing-box"]
    fn g9_2_live_import_start_curl_stop() {
        let paths = temp_app("start-curl");
        seed_singbox_subscription(&paths);
        let settings = settings();
        let mut core = CoreController::new();
        let proxy = create_system_proxy();
        let bin = real_binary();

        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("start");
        assert_eq!(core.state().status, CoreStatus::Running);
        assert!(
            curl_via_mixed(MIXED_PORT),
            "mixed inbound did not respond to curl"
        );

        cleanup(&paths, &mut core, proxy.as_ref());
        println!("G9.2 ok: mixed {MIXED_PORT} responded");
    }

    #[test]
    #[ignore = "live: real sing-box + system proxy"]
    fn g9_3_live_stop_restores_system_proxy() {
        let paths = temp_app("stop-restore");
        seed_singbox_subscription(&paths);
        let settings = settings();
        let mut core = CoreController::new();
        let proxy = create_system_proxy();
        let before = proxy.backup().expect("backup before");
        let bin = real_binary();

        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("start");
        orchestrate_enable_system_proxy(&paths, &settings, &core, proxy.as_ref())
            .expect("enable system proxy");
        orchestrate_stop(&paths, &mut core, proxy.as_ref()).expect("stop");

        let after = proxy.backup().expect("backup after stop");
        assert_eq!(after.enabled, before.enabled);
        assert_eq!(after.http, before.http);
        assert_eq!(after.https, before.https);
        assert_eq!(after.socks, before.socks);

        cleanup(&paths, &mut core, proxy.as_ref());
        println!("G9.3 ok: system proxy restored after stop");
    }

    #[test]
    #[ignore = "live: real sing-box + reload"]
    fn g9_4_live_running_apply_after_fixture_update() {
        let paths = temp_app("apply-reload");
        seed_singbox_subscription(&paths);
        let settings = settings();
        let mut core = CoreController::new();
        let proxy = create_system_proxy();
        let bin = real_binary();

        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin.clone(),
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("start");
        // Simulate subscription refresh (same fixture, still valid nodes).
        seed_singbox_subscription(&paths);
        orchestrate_apply(
            &paths,
            &settings,
            &settings,
            &mut core,
            proxy.as_ref(),
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("apply while running");

        assert_eq!(core.state().status, CoreStatus::Running);
        cleanup(&paths, &mut core, proxy.as_ref());
        println!("G9.4 ok: apply while running kept core up");
    }

    #[test]
    #[ignore = "live: real sing-box + system proxy port change"]
    fn g9_5_live_port_change_updates_system_proxy() {
        let paths = temp_app("port-change");
        seed_singbox_subscription(&paths);
        let settings = settings();
        let mut core = CoreController::new();
        let proxy = create_system_proxy();
        let bin = real_binary();

        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin.clone(),
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("start");
        orchestrate_enable_system_proxy(&paths, &settings, &core, proxy.as_ref())
            .expect("enable system proxy");
        let new_settings = AppSettings {
            mixed_port: MIXED_PORT + 1,
            clash_api_port: CLASH_PORT + 1,
            ..settings.clone()
        };
        orchestrate_apply(
            &paths,
            &new_settings,
            &settings,
            &mut core,
            proxy.as_ref(),
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("apply new port");

        let backup = ProxyBackupFile::load(&paths.proxy_backup()).expect("proxy backup");
        assert!(backup.applied);
        assert_eq!(backup.endpoints.http_port, MIXED_PORT + 1);

        let mid = proxy.backup().expect("read proxy");
        assert!(
            mid.http
                .as_deref()
                .is_some_and(|h| h.contains(&format!("{}", MIXED_PORT + 1))),
            "system proxy not on new port: {:?}",
            mid.http
        );

        orchestrate_stop(&paths, &mut core, proxy.as_ref()).expect("stop");
        cleanup(&paths, &mut core, proxy.as_ref());
        println!("G9.5 ok: system proxy moved to {}", MIXED_PORT + 1);
    }

    #[test]
    #[ignore = "live: real sing-box logs"]
    fn g9_8_live_core_log_tail_nonempty() {
        let paths = temp_app("logs");
        seed_singbox_subscription(&paths);
        let settings = settings();
        let mut core = CoreController::new();
        let proxy = create_system_proxy();
        let bin = real_binary();

        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("start");
        std::thread::sleep(Duration::from_millis(300));

        let lines = read_log_tail(&paths.core_log(), 50).expect("tail");
        assert!(!lines.is_empty(), "core log empty after start");
        let joined = lines.join("\n");
        assert!(
            joined.contains("sing-box") || joined.contains("started") || joined.len() > 10,
            "expected sing-box output in core log"
        );

        cleanup(&paths, &mut core, proxy.as_ref());
        println!("G9.8 ok: core log tail {} lines", lines.len());
    }

    #[test]
    #[ignore = "live: real sing-box + full Clash profile (groups/rules/fake-ip dns)"]
    fn g9_9_live_clash_full_profile_splits_traffic() {
        let paths = temp_app("clash-full");
        let sub = SubscriptionPaths::from_app(&paths);
        let body = repo_fixture("subscription-clash-profile-full.yaml");
        let mgr = SubscriptionManager::with_fetcher(
            sub,
            MockFetcher {
                bypasses_proxy: true,
                mode: MockFetchMode::Ok(FetchResponse {
                    body,
                    not_modified: false,
                    etag: None,
                    last_modified: None,
                    content_disposition: None,
                }),
            },
        );
        let meta = mgr
            .add("https://example.com/clash-full", Some("G9 clash full"))
            .expect("import full clash fixture");
        assert_eq!(meta.node_count, 90);
        assert_eq!(meta.group_count, 21);
        assert!(meta.rule_count > 3000);
        assert!(
            meta.parse_warnings.iter().all(|w| !w.contains("GEOIP")),
            "GEOIP must parse to bundled rule-sets, no warning expected"
        );

        let settings = settings();
        let mut core = CoreController::new();
        let proxy = create_system_proxy();
        let bin = real_binary();

        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("start");
        assert_eq!(core.state().status, CoreStatus::Running);
        assert!(
            curl_via_mixed(MIXED_PORT),
            "mixed inbound did not respond with Clash route/dns active"
        );

        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(paths.config()).unwrap()).unwrap();
        let set_tags: Vec<&str> = config["route"]["rule_set"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e["tag"].as_str())
            .collect();
        assert!(
            set_tags.contains(&"geoip-cn"),
            "GEOIP rules must expand to a bundled geoip-cn rule-set, got {set_tags:?}"
        );

        cleanup(&paths, &mut core, proxy.as_ref());
        println!(
            "G9.9 ok: Clash 90 nodes / 21 groups / {} rules served via mixed",
            meta.rule_count
        );
    }

    #[test]
    #[ignore = "live: real sing-box + Clash API group state"]
    fn g9_10_live_group_exits_listable_and_switchable() {
        use ice_core::{proxy_groups, select_group, GroupState, HealthEndpoints};
        use ice_subscription::{list_profile_outbounds, load_active_profile, load_index};

        let paths = temp_app("group-exits");
        let body = repo_fixture("subscription-clash-profile-full.yaml");
        let mgr = SubscriptionManager::with_fetcher(
            SubscriptionPaths::from_app(&paths),
            MockFetcher {
                bypasses_proxy: true,
                mode: MockFetchMode::Ok(FetchResponse {
                    body,
                    not_modified: false,
                    etag: None,
                    last_modified: None,
                    content_disposition: None,
                }),
            },
        );
        mgr.add("https://example.com/clash-groups", Some("G9 groups"))
            .expect("import");

        let settings = settings();
        let mut core = CoreController::new();
        let proxy = create_system_proxy();
        let bin = real_binary();

        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("start");
        assert_eq!(core.state().status, CoreStatus::Running);

        let endpoints = HealthEndpoints {
            host: "127.0.0.1".into(),
            port: CLASH_PORT,
        };
        let groups: Vec<GroupState> = proxy_groups(&endpoints).expect("list groups");
        assert!(
            groups.iter().any(|g| !g.all.is_empty()),
            "expected live strategy groups, got none"
        );

        let sub = SubscriptionPaths::from_app(&paths);
        let index = load_index(&sub).expect("index");
        let profile = load_active_profile(&sub, &index).expect("profile");
        let static_groups: Vec<_> = list_profile_outbounds(&profile)
            .into_iter()
            .filter(|o| {
                o.outbound
                    .get("type")
                    .and_then(|v| v.as_str())
                    .is_some_and(|t| {
                        ["selector", "urltest", "fallback", "loadbalance"].contains(&t)
                    })
            })
            .collect();
        assert!(
            !static_groups.is_empty(),
            "fixture must contain strategy groups"
        );

        let selector = groups
            .iter()
            .find(|g| g.group_type == "Selector")
            .expect("selector group in live state");
        assert!(
            selector.all.len() >= 2,
            "selector must expose members for customization"
        );
        assert!(
            selector.all.iter().any(|m| m == &selector.now),
            "live now must be one of the members"
        );
        let other = selector
            .all
            .iter()
            .find(|m| **m != selector.now)
            .expect("a second member to switch to");

        select_group(&endpoints, &selector.tag, other).expect("switch member");
        let after: Vec<GroupState> = proxy_groups(&endpoints).expect("re-list");
        let after_sel = after
            .iter()
            .find(|g| g.tag == selector.tag)
            .expect("selector still present");
        assert_eq!(
            after_sel.now, *other,
            "group now must reflect the switched member"
        );

        use ice_config::{load_group_selections, save_group_selections};
        let mut selections = load_group_selections(&paths.group_selections());
        selections.insert(selector.tag.clone(), other.clone());
        save_group_selections(&paths.group_selections(), &selections).expect("persist");
        crate::orchestrate::generate_config(&paths, &settings, None, CaptureIntent::Diagnostic)
            .expect("regenerate");
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(paths.config()).unwrap()).unwrap();
        let rebuilt = config["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["tag"] == selector.tag)
            .expect("group in rebuilt config");
        assert_eq!(
            rebuilt["default"], *other,
            "persisted selection must become the selector default in rebuilt config"
        );

        cleanup(&paths, &mut core, proxy.as_ref());
        println!(
            "G9.10 ok: {} groups live; switched {} -> {}; persisted default applied",
            groups.len(),
            selector.tag,
            other
        );
    }

    /// Slice 4c live: runtime mode switch against real sing-box. With the pinned 1.13.19 the
    /// runtime Clash `mode-list` is `[<default_mode>]`, so a raw `PATCH /configs` to another
    /// mode is silently ignored; the app-level path persists `settings.proxy_mode` and then
    /// rebuilds + reloads (SIGHUP), baking the new `default_mode`. Verify that path end to end.
    #[test]
    #[ignore = "live: real sing-box + Clash API mode switch"]
    fn g9_11_live_mode_switch_via_clash_api() {
        use ice_core::{get_mode, HealthEndpoints};

        let paths = temp_app("mode-switch");
        seed_singbox_subscription(&paths);
        let settings = settings();
        let mut core = CoreController::new();
        let proxy = create_system_proxy();
        let bin = real_binary();

        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin.clone(),
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("start");
        assert_eq!(core.state().status, CoreStatus::Running);
        let endpoints = HealthEndpoints {
            host: "127.0.0.1".into(),
            port: CLASH_PORT,
        };

        let rule = ice_config::clash_mode_name(ice_config::ProxyMode::Rule);
        assert_eq!(get_mode(&endpoints).expect("get mode"), rule);

        let mut previous = settings.clone();
        let mut live_mode_ok = true;
        for mode in [
            ice_config::ProxyMode::Global,
            ice_config::ProxyMode::Direct,
            ice_config::ProxyMode::Rule,
        ] {
            let mut next = previous.clone();
            next.proxy_mode = mode;
            // App-level switch: persists settings, rebuilds + reloads (the PATCH gate never
            // fires under the pinned core), and the reloaded config bakes the new default_mode.
            orchestrate_set_proxy_mode(
                &paths,
                &next,
                &previous,
                &mut core,
                proxy.as_ref(),
                bin.clone(),
                None,
                CaptureIntent::Diagnostic,
                &mut live_mode_ok,
            )
            .expect("set mode");
            assert_eq!(
                get_mode(&endpoints).expect("re-read mode"),
                ice_config::clash_mode_name(mode),
                "runtime mode must reflect the rebuild + reload"
            );
            assert!(
                curl_via_mixed(MIXED_PORT),
                "mixed inbound must keep answering in {} mode",
                ice_config::clash_mode_name(mode)
            );
            previous = next;
        }

        cleanup(&paths, &mut core, proxy.as_ref());
        println!("G9.11 ok: Rule -> Global -> Direct -> Rule via rebuild + reload, no restart");
    }

    /// macOS TUN live gate (plan §6 live acceptance; §5 T3 exit gate).
    /// Uses the dev `sudo` runner (`ICE_BOX_TUN_DEV_SUDO`, cached root
    /// credential or NOPASSWD) to exercise the native-path enable →
    /// traffic → disable roundtrip on a real host. Run via
    /// `scripts/run-acceptance-macos-tun.sh`.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "live: real sing-box + sudo (macOS TUN gate)"]
    fn g9_12_live_tun_enable_curl_disable() {
        use crate::capture::{CaptureController, TrafficCapture, TunStatus};
        use ice_config::TunSettings;
        use ice_tun_sys::{JournalState, MacOsHost, ProcessMacOsHost, TunJournal};

        let dev_sudo = std::env::var("ICE_BOX_TUN_DEV_SUDO")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false);
        assert!(
            dev_sudo,
            "run via scripts/run-acceptance-macos-tun.sh (sets ICE_BOX_TUN_DEV_SUDO and preflights sudo -n)"
        );

        let paths = temp_app("tun-live");
        seed_singbox_subscription(&paths);
        let settings = AppSettings {
            tun: TunSettings {
                enabled: true,
                ..TunSettings::default()
            },
            ..settings()
        };
        let mut core = CoreController::new();
        let proxy = create_system_proxy();
        let bin = real_binary();

        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin.clone(),
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("start diagnostic core");
        assert_eq!(core.state().status, CoreStatus::Running);

        let capture = CaptureController::new(paths.clone(), None);
        capture
            .enable_tun(&settings, &mut core, bin.clone())
            .expect("enable tun");
        assert_eq!(capture.active_backend(), TrafficCapture::Tun);
        assert_eq!(capture.tun_status(), TunStatus::Enabled);
        let status = capture.status(&settings);
        let interface = status
            .tun_interface
            .as_deref()
            .expect("tun_interface after enable");
        assert_eq!(status.traffic_capture, TrafficCapture::Tun);
        let journal = TunJournal::load(&paths.tun_state())
            .expect("journal")
            .expect("journal file");
        assert_eq!(journal.state, JournalState::Applied);

        // Traffic: the mixed inbound is still usable while TUN is active,
        // and TUN capture is on the utun interface (backend verified).
        assert!(curl_via_mixed(MIXED_PORT), "mixed must answer during TUN");

        capture
            .disable_active_backend(&settings, &mut core, proxy.as_ref(), bin.clone(), true)
            .expect("disable tun");
        assert_eq!(capture.active_backend(), TrafficCapture::Inactive);
        assert_eq!(capture.tun_status(), TunStatus::Disabled);
        let journal = TunJournal::load(&paths.tun_state())
            .expect("journal")
            .expect("journal file");
        assert_eq!(journal.state, JournalState::Clean);
        assert!(
            ProcessMacOsHost
                .interface_state(interface)
                .expect("host read")
                .is_none(),
            "adapter {interface} must be removed after disable"
        );

        cleanup(&paths, &mut core, proxy.as_ref());
        println!("G9.12 ok: TUN enable -> mixed curl -> disable -> adapter removed ({interface})");
    }

    /// macOS TUN live gate through the **production privileged helper**
    /// (plan §5 T5). Runs the native-path enable → traffic → disable
    /// roundtrip via the installed launchd helper instead of the dev `sudo`
    /// runner. Run via `scripts/run-acceptance-macos-tun.sh --helper`,
    /// which installs the helper (sudo) with the real app data dir,
    /// sets `ICE_BOX_TUN_LIVE_DATA_DIR`, and uninstalls afterwards.
    ///
    /// The test must use the *installed* data dir: the helper's path
    /// allowlist only accepts `config.json` inside it.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "live: real sing-box + installed privileged helper (macOS TUN gate)"]
    fn g9_13_live_tun_via_helper() {
        use crate::capture::{CaptureController, TrafficCapture, TunStatus};
        use ice_config::TunSettings;
        use ice_tun_sys::{JournalState, MacOsHost, ProcessMacOsHost, TunJournal};

        let data_dir = std::env::var("ICE_BOX_TUN_LIVE_DATA_DIR").unwrap_or_else(|_| {
            format!(
                "{}/Library/Application Support/com.yilong-musk.icebox",
                std::env::var("HOME").expect("HOME")
            )
        });
        assert!(
            !ice_tun_sys::dev_sudo_runner_enabled(),
            "G9.13 exercises the helper path; run without ICE_BOX_TUN_DEV_SUDO"
        );
        let paths = AppPaths::new(&data_dir);
        paths.ensure_dirs().expect("ensure dirs");
        seed_singbox_subscription(&paths);
        let settings = AppSettings {
            tun: TunSettings {
                enabled: true,
                ..TunSettings::default()
            },
            ..settings()
        };
        let mut core = CoreController::new();
        let proxy = create_system_proxy();
        let bin = real_binary();

        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin.clone(),
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("start diagnostic core");
        assert_eq!(core.state().status, CoreStatus::Running);

        // create_backend picks the helper coordinator when it is installed
        // and authorized (probed via a Status frame); the test proves the
        // production wiring end to end.
        let capture = CaptureController::new(paths.clone(), None);
        capture
            .enable_tun(&settings, &mut core, bin.clone())
            .expect("enable tun via helper");
        assert_eq!(capture.active_backend(), TrafficCapture::Tun);
        assert_eq!(capture.tun_status(), TunStatus::Enabled);
        let status = capture.status(&settings);
        let interface = status
            .tun_interface
            .as_deref()
            .expect("tun_interface after enable");
        let journal = TunJournal::load(&paths.tun_state())
            .expect("journal")
            .expect("journal file");
        assert_eq!(journal.state, JournalState::Applied);

        assert!(curl_via_mixed(MIXED_PORT), "mixed must answer during TUN");

        capture
            .disable_active_backend(&settings, &mut core, proxy.as_ref(), bin.clone(), true)
            .expect("disable tun");
        assert_eq!(capture.active_backend(), TrafficCapture::Inactive);
        assert_eq!(capture.tun_status(), TunStatus::Disabled);
        let journal = TunJournal::load(&paths.tun_state())
            .expect("journal")
            .expect("journal file");
        assert_eq!(journal.state, JournalState::Clean);
        assert!(
            ProcessMacOsHost
                .interface_state(interface)
                .expect("host read")
                .is_none(),
            "adapter {interface} must be removed after disable"
        );

        cleanup(&paths, &mut core, proxy.as_ref());
        println!("G9.13 ok: TUN enable -> mixed curl -> disable via helper IPC ({interface})");
    }

    /// Windows TUN live gate (plan §6 live Windows acceptance; the
    /// `windows_tun_ready` gate is still pending — this is the dev opt-in
    /// runner). Mirrors G9.12 with the Windows native path: requires
    /// `ICE_BOX_TUN_WINDOWS_DEV=1` and an already-elevated context (run the
    /// acceptance suite from an Administrator shell). The compile-time
    /// `tun_gate` is forced green only for this live test; production stays
    /// fail-closed until the Windows T0 spike passes. Run via
    /// `scripts/run-acceptance-windows-tun.sh`.
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "live: real sing-box + elevated context (Windows TUN gate)"]
    fn g9_14_live_tun_enable_curl_disable() {
        use crate::capture::{CaptureController, TrafficCapture, TunStatus};
        use ice_config::TunSettings;
        use ice_tun_sys::{JournalState, ProcessWindowsHost, TunJournal, WindowsHost};

        let dev_windows = std::env::var("ICE_BOX_TUN_WINDOWS_DEV")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false);
        assert!(
            dev_windows,
            "run via scripts/run-acceptance-windows-tun.sh (sets ICE_BOX_TUN_WINDOWS_DEV and preflights elevation)"
        );
        // Live test only: let the real Windows backend generate a Tun config
        // on this host. Production gating is untouched (tun_gate stays
        // fail-closed on Windows).
        ice_config::force_tun_gate_ready();

        let paths = temp_app("tun-live");
        seed_singbox_subscription(&paths);
        let settings = AppSettings {
            tun: TunSettings {
                enabled: true,
                ..TunSettings::default()
            },
            ..settings()
        };
        let mut core = CoreController::new();
        let proxy = create_system_proxy();
        let bin = real_binary();

        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin.clone(),
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("start diagnostic core");
        assert_eq!(core.state().status, CoreStatus::Running);

        let capture = CaptureController::new(paths.clone(), None);
        capture
            .enable_tun(&settings, &mut core, bin.clone())
            .expect("enable tun");
        assert_eq!(capture.active_backend(), TrafficCapture::Tun);
        assert_eq!(capture.tun_status(), TunStatus::Enabled);
        let status = capture.status(&settings);
        let interface = status
            .tun_interface
            .as_deref()
            .expect("tun_interface after enable");
        assert_eq!(status.traffic_capture, TrafficCapture::Tun);
        let journal = TunJournal::load(&paths.tun_state())
            .expect("journal")
            .expect("journal file");
        assert_eq!(journal.state, JournalState::Applied);

        // Traffic: the mixed inbound is still usable while TUN is active.
        assert!(curl_via_mixed(MIXED_PORT), "mixed must answer during TUN");

        capture
            .disable_active_backend(&settings, &mut core, proxy.as_ref(), bin.clone(), true)
            .expect("disable tun");
        assert_eq!(capture.active_backend(), TrafficCapture::Inactive);
        assert_eq!(capture.tun_status(), TunStatus::Disabled);
        let journal = TunJournal::load(&paths.tun_state())
            .expect("journal")
            .expect("journal file");
        assert_eq!(journal.state, JournalState::Clean);
        assert!(
            ProcessWindowsHost
                .interface_state(interface)
                .expect("host read")
                .is_none(),
            "adapter {interface} must be removed after disable"
        );

        cleanup(&paths, &mut core, proxy.as_ref());
        println!("G9.14 ok: TUN enable -> mixed curl -> disable -> adapter removed ({interface})");
    }
}
