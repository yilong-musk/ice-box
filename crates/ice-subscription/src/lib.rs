//! Subscription CRUD, format detection, normalization, fetch, store, merge.
//!
//! Format priority: sing-box JSON first, then Clash-compatible YAML/text (slice 6).

mod clash;
mod decode;
mod error;
mod fetch;
mod merge;
mod store;
mod tls_fetch;
mod uri;
mod url;

/// Upper bound for simultaneous subscription network fetches.
const MAX_FETCH_CONCURRENCY: usize = 8;

#[cfg(test)]
mod tests_g5 {
    use super::{
        detect_format, load_active_profile, load_active_profile_with_default_rules, load_index,
        normalize_raw_body, parse_clash_with_stats, parse_singbox, parse_uri_list_profile,
        resolve_selected_tag, set_active, set_auto_update, write_subscription_error,
        AutoUpdateInterval, DirectFetcher, FetchResponse, FetchedUpdate, HttpFetcher,
        MockFetchMode, MockFetcher, SubscriptionError, SubscriptionFormat, SubscriptionManager,
        SubscriptionMeta, SubscriptionPaths, CLASH_SUPPORTED_TYPES, MAX_CLASH_PROXIES,
        MAX_URI_LINES,
    };
    use base64::Engine;
    use chrono::{Duration as ChronoDuration, Utc};
    use ice_config::{
        build_runtime_config, BuildInput, LocalTemplate, NormalizedOutbound, NormalizedProfile,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../configs/examples")
    }

    fn temp_subs(label: &str) -> SubscriptionPaths {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-sub-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        SubscriptionPaths::from_root(dir)
    }

    fn clone_paths(paths: &SubscriptionPaths) -> SubscriptionPaths {
        SubscriptionPaths::from_root(paths.root().to_path_buf())
    }

    #[derive(Clone)]
    struct ConcurrencyFetcher {
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
    }

    impl HttpFetcher for ConcurrencyFetcher {
        fn bypasses_system_proxy(&self) -> bool {
            true
        }

        fn get(
            &self,
            _url: &str,
            _etag: Option<&str>,
            _last_modified: Option<&str>,
        ) -> Result<FetchResponse, SubscriptionError> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            loop {
                let previous = self.max_active.load(Ordering::SeqCst);
                if active <= previous
                    || self
                        .max_active
                        .compare_exchange(previous, active, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(20));
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(FetchResponse {
                body: r#"{"outbounds":[{"type":"socks","tag":"node","server":"1.1.1.1","server_port":1}]}"#.into(),
                not_modified: false,
                etag: None,
                last_modified: None,
                content_disposition: None,
            })
        }
    }

    #[test]
    fn g5_1_fixture_singbox_outbounds() {
        let raw =
            fs::read_to_string(fixtures_dir().join("subscription-singbox-outbounds.json")).unwrap();
        assert_eq!(detect_format(&raw), SubscriptionFormat::SingBox);
        let nodes = parse_singbox(&raw).unwrap();
        assert!(!nodes.is_empty());
    }

    #[test]
    fn g5_1b_endpoints_only_singbox_config() {
        let raw = r#"{
            "endpoints": [
                {"type":"socks","tag":"ep1","server":"1.2.3.4","server_port":1080}
            ]
        }"#;
        assert_eq!(detect_format(raw), SubscriptionFormat::SingBox);
        let nodes = parse_singbox(raw).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].tag, "ep1");
    }

    #[test]
    fn g5_2_empty_nodes_no_success_write() {
        let raw = fs::read_to_string(fixtures_dir().join("subscription-singbox-empty-nodes.json"))
            .unwrap();
        let err = parse_singbox(&raw).expect_err("empty");
        assert!(matches!(err, SubscriptionError::EmptyNodes));
        assert_eq!(err.code().as_str(), "sub.empty");

        let paths = temp_subs("empty");
        let fetcher = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Ok(FetchResponse {
                body: raw,
                not_modified: false,
                etag: None,
                last_modified: None,
                content_disposition: None,
            }),
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
        let err = mgr
            .add("https://example.com/sub", None, false, None)
            .expect_err("add");
        assert!(matches!(err, SubscriptionError::EmptyNodes));
        assert!(!paths.index().exists() || load_index(&paths).unwrap().items.is_empty());
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g5_3_full_config_strips_non_nodes() {
        let raw = fs::read_to_string(fixtures_dir().join("subscription-singbox-full-config.json"))
            .unwrap();
        let nodes = parse_singbox(&raw).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].tag, "node-a");
        assert_eq!(nodes[0].outbound["type"], "socks");
    }

    #[test]
    fn g5_4_body_over_8mib() {
        let paths = temp_subs("big");
        let fetcher = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::TooLarge,
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
        let err = mgr
            .add("https://example.com/big", None, false, None)
            .expect_err("big");
        assert_eq!(err.code().as_str(), "sub.fetch_failed");
        assert!(err.to_string().contains("exceeds"));
        assert!(!paths.index().exists());
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g5_5_mock_timeout() {
        let paths = temp_subs("timeout");
        let fetcher = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Timeout,
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
        let err = mgr
            .add("https://example.com/t", None, false, None)
            .expect_err("t");
        assert_eq!(err.code().as_str(), "sub.fetch_failed");
        assert!(!paths.root().join("index.json").exists());
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g5_6_update_failure_keeps_old_bytes() {
        let paths = temp_subs("upd");
        let body =
            fs::read_to_string(fixtures_dir().join("subscription-singbox-outbounds.json")).unwrap();
        let ok = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Ok(FetchResponse {
                body: body.clone(),
                not_modified: false,
                etag: Some("v1".into()),
                last_modified: None,
                content_disposition: None,
            }),
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), ok);
        let meta = mgr
            .add("https://example.com/s", Some("t1"), false, None)
            .unwrap();
        let raw_before = fs::read(paths.raw(meta.id)).unwrap();
        let profile_before = fs::read(paths.profile(meta.id)).unwrap();
        assert!(
            !paths.nodes(meta.id).exists(),
            "nodes.json is a legacy duplicate and is no longer written"
        );

        let fail = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Fail("network down".into()),
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fail);
        let err = mgr.update(meta.id).expect_err("upd");
        assert_eq!(err.code().as_str(), "sub.fetch_failed");
        assert_eq!(fs::read(paths.raw(meta.id)).unwrap(), raw_before);
        assert_eq!(fs::read(paths.profile(meta.id)).unwrap(), profile_before);
        let index = load_index(&paths).unwrap();
        let m = index.items.iter().find(|i| i.id == meta.id).unwrap();
        assert!(m.last_error.as_ref().unwrap().contains("network down"));
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn apply_all_commits_index_once_across_mixed_results() {
        let paths = temp_subs("batch");
        let body =
            r#"{"outbounds":[{"type":"socks","tag":"same","server":"1.1.1.1","server_port":1}]}"#;
        let ok = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Ok(FetchResponse {
                body: body.into(),
                not_modified: false,
                etag: None,
                last_modified: None,
                content_disposition: None,
            }),
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), ok);
        let a = mgr
            .add("https://example.com/a", Some("a"), false, None)
            .unwrap();
        let b = mgr
            .add("https://example.com/b", Some("b"), false, None)
            .unwrap();
        // Simulate a previously failed fetch on b.
        write_subscription_error(&paths, b.id, "previous failure".into()).unwrap();

        let index = load_index(&paths).unwrap();
        let a_meta = index.items.iter().find(|m| m.id == a.id).unwrap().clone();
        let b_meta = index.items.iter().find(|m| m.id == b.id).unwrap().clone();
        set_active(&paths, b.id, true).unwrap();
        let results = mgr.apply_all(vec![
            (
                a.id,
                Ok(FetchedUpdate {
                    meta: a_meta,
                    fetched: FetchResponse {
                        body: r#"{"outbounds":[{"type":"socks","tag":"x","server":"1.1.1.1","server_port":1},{"type":"socks","tag":"y","server":"2.2.2.2","server_port":2}]}"#
                            .into(),
                        not_modified: false,
                        etag: Some("v2".into()),
                        last_modified: None,
                        content_disposition: None,
                    },
                }),
            ),
            (
                b.id,
                Ok(FetchedUpdate {
                    meta: b_meta,
                    fetched: FetchResponse {
                        body: String::new(),
                        not_modified: true,
                        etag: None,
                        last_modified: None,
                        content_disposition: None,
                    },
                }),
            ),
            (
                Uuid::new_v4(),
                Err(SubscriptionError::FetchFailed("network down".into())),
            ),
        ]);

        let updated_a = results[0].1.as_ref().expect("a updated");
        assert_eq!(updated_a.node_count, 2);
        assert!(results[1].1.is_ok(), "not-modified returns the meta");
        assert!(results[2].1.is_err(), "fetch error surfaces");

        let index = load_index(&paths).unwrap();
        let a_after = index.items.iter().find(|m| m.id == a.id).unwrap();
        assert_eq!(a_after.node_count, 2);
        assert_eq!(a_after.etag.as_deref(), Some("v2"));
        assert!(a_after.last_error.is_none());
        let b_after = index.items.iter().find(|m| m.id == b.id).unwrap();
        assert!(
            b_after.active,
            "batch apply must preserve a newer active state"
        );
        assert!(
            b_after.last_error.is_none(),
            "not-modified clears the recorded error"
        );
        assert_eq!(
            index.items.iter().filter(|m| m.active).count(),
            1,
            "single active preserved across the batch"
        );
        assert_eq!(
            index.items.len(),
            2,
            "a fetch for a removed/unknown id must not resurrect it"
        );
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn fetch_all_caps_network_concurrency() {
        let paths = temp_subs("fetch-pool");
        let fetcher = ConcurrencyFetcher {
            active: Arc::new(AtomicUsize::new(0)),
            max_active: Arc::new(AtomicUsize::new(0)),
        };
        let max_active = Arc::clone(&fetcher.max_active);
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
        let count = super::MAX_FETCH_CONCURRENCY + 4;
        for index in 0..count {
            mgr.add(&format!("https://example.com/{index}"), None, false, None)
                .expect("add subscription");
        }

        let updates = mgr.fetch_all();
        assert_eq!(updates.len(), count);
        assert!(
            max_active.load(Ordering::SeqCst) > 1,
            "fetch_all should retain useful parallelism"
        );
        assert!(
            max_active.load(Ordering::SeqCst) <= super::MAX_FETCH_CONCURRENCY,
            "fetch_all exceeded its worker limit"
        );
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn auto_update_flag_roundtrips_through_add_and_set() {
        let paths = temp_subs("auto-flag");
        let body =
            r#"{"outbounds":[{"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}]}"#;
        let fetcher = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Ok(FetchResponse {
                body: body.into(),
                not_modified: false,
                etag: None,
                last_modified: None,
                content_disposition: None,
            }),
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
        let meta = mgr
            .add(
                "https://example.com/a",
                Some("a"),
                true,
                Some(crate::AutoUpdateInterval::OneHour),
            )
            .unwrap();
        assert!(meta.auto_update, "add must persist the requested flag");

        let off = mgr.set_auto_update(meta.id, false, None).unwrap();
        assert!(!off.auto_update);
        let on = mgr
            .set_auto_update(meta.id, true, Some(crate::AutoUpdateInterval::TwelveHours))
            .unwrap();
        assert!(on.auto_update);
        assert_eq!(
            on.auto_update_interval,
            Some(crate::AutoUpdateInterval::TwelveHours)
        );

        let index = load_index(&paths).unwrap();
        assert!(
            index
                .items
                .iter()
                .find(|m| m.id == meta.id)
                .unwrap()
                .auto_update
        );
        let disk_meta: SubscriptionMeta =
            serde_json::from_str(&fs::read_to_string(paths.meta(meta.id)).unwrap()).unwrap();
        assert!(disk_meta.auto_update, "flag must persist to meta.json");
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn auto_update_flag_survives_an_update() {
        let paths = temp_subs("auto-update");
        let body =
            r#"{"outbounds":[{"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}]}"#;
        let ok = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Ok(FetchResponse {
                body: body.into(),
                not_modified: false,
                etag: Some("v1".into()),
                last_modified: None,
                content_disposition: None,
            }),
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), ok);
        let meta = mgr
            .add(
                "https://example.com/s",
                Some("t1"),
                true,
                Some(crate::AutoUpdateInterval::OneHour),
            )
            .unwrap();
        let updated = mgr.update(meta.id).unwrap();
        assert!(updated.auto_update, "refresh must preserve the flag");
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn update_uses_latest_metadata_after_fetch() {
        let paths = temp_subs("update-latest-meta");
        let fetcher = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Ok(FetchResponse {
                body: r#"{"outbounds":[{"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}]}"#.into(),
                not_modified: false,
                etag: Some("v2".into()),
                last_modified: None,
                content_disposition: None,
            }),
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
        let meta = mgr
            .add("https://example.com/s", Some("s"), false, None)
            .unwrap();

        let fetched = mgr.fetch_update(meta.id).unwrap();
        set_active(&paths, meta.id, false).unwrap();
        set_auto_update(&paths, meta.id, true, Some(AutoUpdateInterval::ThreeHours)).unwrap();

        let updated = mgr.apply_update(fetched).unwrap();
        assert!(!updated.active, "the latest active state must be preserved");
        assert!(
            updated.auto_update,
            "the latest auto-update state must be preserved"
        );
        assert_eq!(
            updated.auto_update_interval,
            Some(AutoUpdateInterval::ThreeHours)
        );
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn not_modified_refreshes_last_updated() {
        let paths = temp_subs("304-refresh-time");
        let body =
            r#"{"outbounds":[{"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}]}"#;
        let ok = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Ok(FetchResponse {
                body: body.into(),
                not_modified: false,
                etag: Some("v1".into()),
                last_modified: None,
                content_disposition: None,
            }),
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), ok);
        let meta = mgr
            .add(
                "https://example.com/s",
                Some("s"),
                true,
                Some(AutoUpdateInterval::TwentyFourHours),
            )
            .unwrap();
        let old = Utc::now() - ChronoDuration::days(2);
        let mut index = load_index(&paths).unwrap();
        index
            .items
            .iter_mut()
            .find(|item| item.id == meta.id)
            .unwrap()
            .last_updated = Some(old);
        super::save_index(&paths, &index).unwrap();

        let not_modified = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::NotModified,
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), not_modified);
        let updated = mgr.update(meta.id).expect("304 should succeed");
        assert!(updated.last_updated.unwrap() > old);
        let persisted = load_index(&paths)
            .unwrap()
            .items
            .into_iter()
            .find(|item| item.id == meta.id)
            .unwrap();
        assert_eq!(persisted.last_updated, updated.last_updated);
        let disk_meta: SubscriptionMeta =
            serde_json::from_str(&fs::read_to_string(paths.meta(meta.id)).unwrap()).unwrap();
        assert_eq!(disk_meta.last_updated, updated.last_updated);
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn fetch_auto_only_fetches_flagged_subscriptions() {
        let paths = temp_subs("auto-fetch");
        let ok = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Ok(FetchResponse {
                body: r#"{"outbounds":[{"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}]}"#
                    .into(),
                not_modified: false,
                etag: None,
                last_modified: None,
                content_disposition: None,
            }),
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), ok);
        let a = mgr
            .add(
                "https://example.com/a",
                Some("a"),
                true,
                Some(crate::AutoUpdateInterval::OneHour),
            )
            .unwrap();
        let b = mgr
            .add("https://example.com/b", Some("b"), false, None)
            .unwrap();

        let auto = mgr.fetch_auto();
        assert_eq!(auto.len(), 1, "only the flagged subscription is fetched");
        assert_eq!(auto[0].0, a.id);
        assert!(auto[0].1.is_ok());
        assert_ne!(b.id, a.id);

        let all = mgr.fetch_all();
        assert_eq!(all.len(), 2);
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g5_7_single_active_subscription_wins() {
        // Two subscriptions with the same tag: only the active one is loaded; no merge / tag
        // disambiguation needed under the single-active model.
        let paths = temp_subs("merge");
        let body =
            r#"{"outbounds":[{"type":"socks","tag":"same","server":"1.1.1.1","server_port":1}]}"#;
        let mut ids = Vec::new();
        for name in ["a", "b"] {
            let fetcher = MockFetcher {
                bypasses_proxy: true,
                mode: MockFetchMode::Ok(FetchResponse {
                    body: body.into(),
                    not_modified: false,
                    etag: None,
                    last_modified: None,
                    content_disposition: None,
                }),
            };
            let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
            ids.push(
                mgr.add(
                    &format!("https://example.com/{name}"),
                    Some(name),
                    false,
                    None,
                )
                .unwrap()
                .id,
            );
        }
        let index = load_index(&paths).unwrap();
        let active: Vec<_> = index.items.iter().filter(|m| m.active).collect();
        assert_eq!(active.len(), 1, "exactly one active subscription");
        assert_eq!(active[0].id, ids[0], "first import becomes active");
        let profile = load_active_profile(&paths, &index).unwrap();
        assert_eq!(profile.nodes.len(), 1);
        assert_eq!(profile.nodes[0].tag, "same");

        set_active(&paths, ids[1], true).unwrap();
        let index = load_index(&paths).unwrap();
        assert_eq!(index.items.iter().filter(|m| m.active).count(), 1);
        let profile = load_active_profile(&paths, &index).unwrap();
        assert_eq!(profile.nodes.len(), 1, "switch loads the other profile");
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g5_8_selected_via_resolve_and_build() {
        let outbounds = vec![
            NormalizedOutbound {
                tag: "one".into(),
                outbound: serde_json::json!({"type":"socks","tag":"one","server":"x","server_port":1}),
            },
            NormalizedOutbound {
                tag: "two".into(),
                outbound: serde_json::json!({"type":"socks","tag":"two","server":"y","server_port":1}),
            },
        ];
        let profile = NormalizedProfile::from_nodes_only(outbounds);
        let sel = resolve_selected_tag(Some("missing"), &profile);
        assert_eq!(sel.as_deref(), Some("one"));
        let cfg = build_runtime_config(&BuildInput {
            template: LocalTemplate::default(),
            profile,
            selected_tag: sel,
            geoip_rule_set_dir: None,
            group_selections: Default::default(),
            rule_overrides: Default::default(),
            capture_intent: Default::default(),
        })
        .unwrap();
        assert!(cfg["outbounds"].as_array().unwrap().len() >= 3);
    }

    #[test]
    fn g5_15_update_not_modified_keeps_cached_bytes() {
        let paths = temp_subs("304");
        let body =
            fs::read_to_string(fixtures_dir().join("subscription-singbox-outbounds.json")).unwrap();
        let ok = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Ok(FetchResponse {
                body: body.clone(),
                not_modified: false,
                etag: Some("v1".into()),
                last_modified: None,
                content_disposition: None,
            }),
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), ok);
        let meta = mgr
            .add("https://example.com/s", Some("t1"), false, None)
            .unwrap();
        let raw_before = fs::read(paths.raw(meta.id)).unwrap();
        let profile_before = fs::read(paths.profile(meta.id)).unwrap();

        let not_modified = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::NotModified,
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), not_modified);
        let updated = mgr.update(meta.id).expect("304 should succeed");
        assert_eq!(updated.id, meta.id);
        assert_eq!(updated.node_count, meta.node_count);
        assert_eq!(fs::read(paths.raw(meta.id)).unwrap(), raw_before);
        assert_eq!(fs::read(paths.profile(meta.id)).unwrap(), profile_before);
        let index = load_index(&paths).unwrap();
        let m = index.items.iter().find(|i| i.id == meta.id).unwrap();
        assert!(m.last_error.is_none());
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g5_15_304_clears_stale_last_error() {
        let paths = temp_subs("304-clear");
        let body =
            fs::read_to_string(fixtures_dir().join("subscription-singbox-outbounds.json")).unwrap();
        let ok = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Ok(FetchResponse {
                body: body.clone(),
                not_modified: false,
                etag: Some("v1".into()),
                last_modified: None,
                content_disposition: None,
            }),
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), ok);
        let meta = mgr
            .add("https://example.com/s", Some("t1"), false, None)
            .unwrap();

        let fail = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Fail("network down".into()),
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fail);
        mgr.update(meta.id).expect_err("failed update");
        let index = load_index(&paths).unwrap();
        let m = index.items.iter().find(|i| i.id == meta.id).unwrap();
        assert!(m.last_error.is_some(), "error must be recorded first");

        let not_modified = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::NotModified,
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), not_modified);
        let updated = mgr.update(meta.id).expect("304 should succeed");
        assert!(updated.last_error.is_none(), "304 must clear last_error");
        let index = load_index(&paths).unwrap();
        let m = index.items.iter().find(|i| i.id == meta.id).unwrap();
        assert!(m.last_error.is_none(), "index last_error must be cleared");
        let on_disk: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(paths.meta(meta.id)).unwrap()).unwrap();
        assert!(
            on_disk["last_error"].is_null(),
            "meta.json last_error cleared"
        );
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g5_11_disk_layout_order() {
        let paths = temp_subs("layout");
        let body =
            fs::read_to_string(fixtures_dir().join("subscription-singbox-outbounds.json")).unwrap();
        let fetcher = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Ok(FetchResponse {
                body,
                not_modified: false,
                etag: None,
                last_modified: None,
                content_disposition: Some(r#"attachment; filename="my.json""#.into()),
            }),
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
        let meta = mgr
            .add("https://example.com/path/sub.json", None, false, None)
            .unwrap();
        assert_eq!(meta.name, "my.json");
        assert!(paths.raw(meta.id).is_file());
        assert!(
            !paths.nodes(meta.id).exists(),
            "nodes.json is a legacy duplicate and is no longer written"
        );
        assert!(paths.meta(meta.id).is_file());
        let index = load_index(&paths).unwrap();
        assert!(index.items.iter().any(|i| i.id == meta.id));
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g5_12_fetcher_bypasses_system_proxy() {
        assert!(DirectFetcher.bypasses_system_proxy());
        let paths = temp_subs("proxy");
        let fetcher = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Timeout,
        };
        assert!(fetcher.bypasses_system_proxy());
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
        let _ = mgr.add("https://example.com", None, false, None);
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g5_13_base64_wrapped_json() {
        let inner =
            fs::read_to_string(fixtures_dir().join("subscription-singbox-outbounds.json")).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(inner.as_bytes());
        let (format, profile) = normalize_raw_body(&encoded).unwrap();
        assert_eq!(format, SubscriptionFormat::SingBox);
        assert!(!profile.nodes.is_empty());
    }

    fn assert_node_shape(node: &ice_config::NormalizedOutbound, expected_type: &str) {
        assert_eq!(node.outbound["type"], expected_type);
        assert!(node.outbound.get("tag").and_then(|v| v.as_str()).is_some());
        assert!(node
            .outbound
            .get("server")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty()));
        assert!(!node.tag.is_empty());
    }

    #[test]
    fn g6_1_clash_ss() {
        let raw = fs::read_to_string(fixtures_dir().join("subscription-clash-ss.yaml")).unwrap();
        let result = parse_clash_with_stats(&raw).unwrap();
        assert_eq!(result.profile.nodes.len(), 1);
        assert_node_shape(&result.profile.nodes[0], "shadowsocks");
        assert_eq!(result.profile.nodes[0].outbound["method"], "aes-256-gcm");
    }

    #[test]
    fn g6_2_clash_vmess() {
        let raw = fs::read_to_string(fixtures_dir().join("subscription-clash-vmess.yaml")).unwrap();
        let result = parse_clash_with_stats(&raw).unwrap();
        assert_eq!(result.profile.nodes.len(), 1);
        assert_node_shape(&result.profile.nodes[0], "vmess");
        assert_eq!(result.profile.nodes[0].outbound["transport"]["type"], "ws");
    }

    #[test]
    fn g6_3_clash_trojan() {
        let raw =
            fs::read_to_string(fixtures_dir().join("subscription-clash-trojan.yaml")).unwrap();
        let result = parse_clash_with_stats(&raw).unwrap();
        assert_eq!(result.profile.nodes.len(), 1);
        assert_node_shape(&result.profile.nodes[0], "trojan");
        assert_eq!(result.profile.nodes[0].outbound["tls"]["enabled"], true);
    }

    #[test]
    fn g6_4_clash_socks() {
        let raw = fs::read_to_string(fixtures_dir().join("subscription-clash-socks.yaml")).unwrap();
        let result = parse_clash_with_stats(&raw).unwrap();
        assert_eq!(result.profile.nodes.len(), 1);
        assert_node_shape(&result.profile.nodes[0], "socks");
    }

    #[test]
    fn g6_5_clash_http() {
        let raw = fs::read_to_string(fixtures_dir().join("subscription-clash-http.yaml")).unwrap();
        let result = parse_clash_with_stats(&raw).unwrap();
        assert_eq!(result.profile.nodes.len(), 1);
        assert_node_shape(&result.profile.nodes[0], "http");
    }

    #[test]
    fn g6_6_mixed_ignores_proxy_groups() {
        let raw = fs::read_to_string(fixtures_dir().join("subscription-clash-mixed.yaml")).unwrap();
        let result = parse_clash_with_stats(&raw).unwrap();
        assert_eq!(result.profile.nodes.len(), 5);
        let types: Vec<_> = result
            .profile
            .nodes
            .iter()
            .map(|n| n.outbound["type"].as_str().unwrap())
            .collect();
        assert!(types.contains(&"shadowsocks"));
        assert!(types.contains(&"vmess"));
        assert!(types.contains(&"trojan"));
        assert!(types.contains(&"socks"));
        assert!(types.contains(&"http"));
        assert!(!types.iter().any(|t| *t == "selector" || *t == "urltest"));
        assert!(!result
            .profile
            .nodes
            .iter()
            .any(|n| n.tag == "PROXY" || n.tag == "AUTO"));
    }

    #[test]
    fn g6_7_unknown_only_empty() {
        let raw = fs::read_to_string(fixtures_dir().join("subscription-clash-unknown-only.yaml"))
            .unwrap();
        let err = parse_clash_with_stats(&raw).expect_err("empty");
        assert!(matches!(err, SubscriptionError::EmptyNodes));
        assert_eq!(err.code().as_str(), "sub.empty");
    }

    #[test]
    fn g6_8_known_plus_unknown_skip_count() {
        let raw = fs::read_to_string(fixtures_dir().join("subscription-clash-mixed-unknown.yaml"))
            .unwrap();
        let result = parse_clash_with_stats(&raw).unwrap();
        assert_eq!(result.profile.nodes.len(), 2);
        assert!(result.profile.parse_stats.skipped_proxies >= 1);
    }

    #[test]
    fn g6_8b_clash_rejects_too_many_proxies() {
        let mut raw = String::from("proxies:\n");
        for i in 0..=MAX_CLASH_PROXIES {
            raw.push_str(&format!(
                "  - {{ type: ss, name: n{i}, server: 1.1.1.1, port: 443, cipher: aes-128-gcm, password: x }}\n"
            ));
        }
        let err = parse_clash_with_stats(&raw).expect_err("too many proxies");
        assert!(err.to_string().contains("exceeds limit"));
    }

    #[test]
    fn g6_9_detect_proxies_as_clash() {
        let raw = fs::read_to_string(fixtures_dir().join("subscription-clash-mixed.yaml")).unwrap();
        assert_eq!(detect_format(&raw), SubscriptionFormat::Clash);
        assert!(CLASH_SUPPORTED_TYPES.contains(&"ss"));
        assert!(CLASH_SUPPORTED_TYPES.contains(&"vmess"));
        assert!(CLASH_SUPPORTED_TYPES.contains(&"trojan"));
    }

    #[test]
    fn g6_10_checklist_matches_supported_const() {
        for ty in ["ss", "vmess", "trojan", "socks", "http"] {
            assert!(
                CLASH_SUPPORTED_TYPES
                    .iter()
                    .any(|s| *s == ty || (*s == "socks5" && ty == "socks")),
                "missing {ty}"
            );
        }
    }

    #[test]
    fn g5_14_rejects_internal_subscription_url() {
        let paths = temp_subs("ssrf");
        let fetcher = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Ok(FetchResponse {
                body: "{}".into(),
                not_modified: false,
                etag: None,
                last_modified: None,
                content_disposition: None,
            }),
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
        let err = mgr
            .add("http://127.0.0.1/sub", None, false, None)
            .expect_err("blocked");
        assert_eq!(err.code().as_str(), "sub.fetch_failed");
        assert!(err.to_string().contains("not allowed"));
        assert!(!paths.index().exists());
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g6_11_singbox_still_works() {
        let raw =
            fs::read_to_string(fixtures_dir().join("subscription-singbox-outbounds.json")).unwrap();
        assert_eq!(detect_format(&raw), SubscriptionFormat::SingBox);
        assert!(!parse_singbox(&raw).unwrap().is_empty());
    }

    #[test]
    fn g5_16_uri_list_import_and_manager() {
        let raw = fs::read_to_string(fixtures_dir().join("subscription-uri-list.txt")).unwrap();

        // Detection: raw body and base64-wrapped body both resolve to UriList.
        assert_eq!(detect_format(&raw), SubscriptionFormat::UriList);
        let (format, profile) = normalize_raw_body(&raw).unwrap();
        assert_eq!(format, SubscriptionFormat::UriList);
        assert_eq!(profile.nodes.len(), 14, "15 lines, only ssr:// skipped");
        assert_eq!(profile.parse_stats.skipped_proxies, 1);
        assert!(
            profile
                .parse_stats
                .warnings
                .iter()
                .any(|w| w.contains("ssr://")),
            "ssr skip must surface as a warning"
        );

        let types: Vec<&str> = profile
            .nodes
            .iter()
            .map(|n| n.outbound["type"].as_str().unwrap())
            .collect();
        for t in [
            "vless",
            "hysteria2",
            "hysteria",
            "trojan",
            "vmess",
            "tuic",
            "shadowsocks",
            "socks",
            "http",
            "wireguard",
        ] {
            assert!(types.contains(&t), "missing {t}");
        }

        // Fragment names are percent-decoded; v2rayN vmess `ps` names nodes
        // that have no fragment.
        let tags: Vec<&str> = profile.nodes.iter().map(|n| n.tag.as_str()).collect();
        assert!(tags.contains(&"日本东京01|1023.81 GB"));
        assert!(tags.contains(&"香港机场05|BGP|新加坡"));
        assert!(tags.contains(&"VMESS-01"));

        // Rule-less URI lists stay pure at parse time; built-in split-routing
        // rules + DNS are attached at profile load (honoring the setting).
        assert!(profile.route.rules.is_empty());
        assert_eq!(profile.route.final_outbound, "proxy");
        assert!(profile.dns.is_none());

        // vless reality node keeps pbk/sid and requires a utls fingerprint.
        let reality = profile
            .nodes
            .iter()
            .find(|n| n.outbound["tls"]["reality"]["enabled"] == true)
            .expect("reality node");
        assert_eq!(
            reality.outbound["tls"]["reality"]["public_key"],
            "EYa4ic3GAxqznV61U-Oww-WKsu5wuQQptyS3fw7czM"
        );
        assert_eq!(reality.outbound["tls"]["reality"]["short_id"], "c50db39f");
        assert_eq!(reality.outbound["tls"]["server_name"], "www.lamer.com.hk");
        assert_eq!(reality.outbound["flow"], "xtls-rprx-vision");
        assert_eq!(reality.outbound["tls"]["utls"]["fingerprint"], "ios");

        // vless vision over plain TLS keeps utls fingerprint and honors
        // insecure=1; pcs pin is dropped.
        let vision = profile
            .nodes
            .iter()
            .find(|n| n.tag == "香港机场01|BGP|CMCU")
            .expect("vision node");
        assert_eq!(vision.outbound["flow"], "xtls-rprx-vision");
        assert_eq!(vision.outbound["tls"]["utls"]["fingerprint"], "safari");
        assert_eq!(vision.outbound["tls"]["insecure"], true);
        assert!(vision.outbound["tls"]
            .as_object()
            .unwrap()
            .get("pcs")
            .is_none());

        // hysteria2 node: password is userinfo; pinSHA256/mport dropped, obfs kept.
        let hy2 = profile
            .nodes
            .iter()
            .find(|n| n.tag == "HK-01")
            .expect("hy2 obfs node");
        assert_eq!(hy2.outbound["password"], "secret@word");
        assert_eq!(hy2.outbound["obfs"]["type"], "salamander");
        assert_eq!(hy2.outbound["obfs"]["password"], "salty");
        assert!(hy2.outbound.as_object().unwrap().get("pinSHA256").is_none());

        // Full import through SubscriptionManager (base64-wrapped body).
        let encoded = base64::engine::general_purpose::STANDARD.encode(raw.as_bytes());
        let paths = temp_subs("uri");
        let fetcher = MockFetcher {
            bypasses_proxy: true,
            mode: MockFetchMode::Ok(FetchResponse {
                body: encoded,
                not_modified: false,
                etag: None,
                last_modified: None,
                content_disposition: None,
            }),
        };
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
        let meta = mgr
            .add("https://example.com/sub", Some("liangxin"), false, None)
            .unwrap();
        assert_eq!(meta.name, "liangxin");
        assert_eq!(meta.format, SubscriptionFormat::UriList);
        assert_eq!(meta.node_count, 14);
        let profile = load_active_profile(&paths, &load_index(&paths).unwrap()).unwrap();
        assert_eq!(profile.nodes.len(), 14);
        // Load-time defaults: 3 split-routing rules + built-in DNS.
        assert_eq!(profile.route.rules.len(), 3);
        assert_eq!(profile.route.rules[1]["geoip"][0], "cn");
        assert!(profile.route.rules[2]["domain_suffix"]
            .as_array()
            .is_some_and(|a| a.len() > 100));
        assert_eq!(profile.route.final_outbound, "proxy");
        let dns = profile.dns.expect("built-in dns block");
        let dns_tags: Vec<&str> = dns["servers"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| s["tag"].as_str())
            .collect();
        assert!(dns_tags.contains(&"cn-dns"));
        assert!(dns_tags.contains(&"remote-dns"));
        assert_eq!(dns["final"], "remote-dns");
        // Disabling the setting keeps the cached profile pure.
        let raw_profile =
            load_active_profile_with_default_rules(&paths, &load_index(&paths).unwrap(), false)
                .unwrap();
        assert!(raw_profile.route.rules.is_empty());
        assert!(raw_profile.dns.is_none());
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g5_17_uri_list_rejects_empty() {
        let err = parse_uri_list_profile("vmess://\ntrojan://\nss://").expect_err("no nodes");
        assert!(matches!(err, SubscriptionError::EmptyNodes));
    }

    #[test]
    fn g5_18_uri_list_line_limit() {
        let mut raw = String::new();
        for _ in 0..=MAX_URI_LINES {
            raw.push_str("vless://u@h:443?encryption=none#n\n");
        }
        let err = parse_uri_list_profile(&raw).expect_err("too many lines");
        assert!(err.to_string().contains("exceeds"));
    }
}

pub use clash::{
    parse_clash_profile, parse_clash_with_stats, ClashParseResult, CLASH_SUPPORTED_TYPES,
    MAX_CLASH_PROXIES,
};
pub use decode::maybe_decode_base64;
pub use error::SubscriptionError;
pub use fetch::{
    DirectFetcher, FetchResponse, HttpFetcher, MockFetchMode, MockFetcher, FETCH_TIMEOUT,
    MAX_BODY_BYTES,
};
pub use merge::{
    active_subscription, list_profile_outbounds, load_active_profile,
    load_active_profile_with_default_rules, resolve_selected_tag, short_id,
};
pub use store::{
    apply_error_to_index, apply_success_to_index, clear_error_in_index, clear_subscription_error,
    commit_subscription_success, load_index, mark_refreshed_in_index, mark_subscription_refreshed,
    read_nodes, read_profile, remove_subscription, save_index, set_active, set_auto_update,
    set_enabled, write_subscription_error, write_subscription_success, SubscriptionPaths,
};
pub use uri::{
    apply_builtin_default_rules, looks_like_uri_list, parse_uri_list_profile, MAX_URI_LINES,
};
pub use url::{
    redact_subscription_url_for_log, redact_subscription_url_for_ui, redact_urls_in_text,
};

use chrono::{DateTime, Utc};
use ice_config::{NormalizedOutbound, NormalizedProfile, NormalizedRoute, ProfileParseStats};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::decode::maybe_decode_base64 as decode_body;
use crate::url::validate_subscription_url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionFormat {
    SingBox,
    Clash,
    /// Proxy share-link list (`vless://`, `trojan://`, `hysteria2://`, ...).
    UriList,
    Unknown,
}

/// Refresh cadence offered for per-subscription auto-update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoUpdateInterval {
    OneHour,
    ThreeHours,
    SixHours,
    TwelveHours,
    TwentyFourHours,
}

impl AutoUpdateInterval {
    pub const ALL: [AutoUpdateInterval; 5] = [
        AutoUpdateInterval::OneHour,
        AutoUpdateInterval::ThreeHours,
        AutoUpdateInterval::SixHours,
        AutoUpdateInterval::TwelveHours,
        AutoUpdateInterval::TwentyFourHours,
    ];

    pub fn hours(self) -> u32 {
        match self {
            AutoUpdateInterval::OneHour => 1,
            AutoUpdateInterval::ThreeHours => 3,
            AutoUpdateInterval::SixHours => 6,
            AutoUpdateInterval::TwelveHours => 12,
            AutoUpdateInterval::TwentyFourHours => 24,
        }
    }

    pub fn duration(self) -> std::time::Duration {
        std::time::Duration::from_secs(self.hours() as u64 * 3600)
    }

    /// Used when a subscription predates interval selection (auto_update on,
    /// no interval stored): the closest cadence to the original 30-minute tick.
    pub fn default_duration() -> std::time::Duration {
        AutoUpdateInterval::OneHour.duration()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionMeta {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    #[serde(default, alias = "enabled")]
    pub active: bool,
    pub format: SubscriptionFormat,
    pub node_count: usize,
    #[serde(default)]
    pub group_count: usize,
    #[serde(default)]
    pub rule_count: usize,
    #[serde(default)]
    pub has_dns: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parse_warnings: Vec<String>,
    pub last_updated: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    /// Refresh this subscription on a background schedule when enabled.
    #[serde(default)]
    pub auto_update: bool,
    /// Cadence for the background refresh; `None` falls back to
    /// [`AutoUpdateInterval::default_duration`] for legacy entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_update_interval: Option<AutoUpdateInterval>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SubscriptionIndex {
    pub items: Vec<SubscriptionMeta>,
}

/// Detect format from raw subscription body.
pub fn detect_format(raw: &str) -> SubscriptionFormat {
    let trimmed = raw.trim_start();
    // Cheap structural sniff instead of a full JSON parse (bodies can be up to
    // 8 MiB): sing-box bodies are objects carrying a quoted `outbounds` /
    // `endpoints` key. The real parse happens later in `parse_singbox_profile`,
    // so a classification here loses no information or error detail.
    if trimmed.starts_with('{')
        && (trimmed.contains("\"outbounds\"") || trimmed.contains("\"endpoints\""))
    {
        return SubscriptionFormat::SingBox;
    }

    // Zero-allocation case-insensitive scan (no `to_ascii_lowercase` copy).
    if crate::decode::contains_ascii_case_insensitive(trimmed, "proxies:")
        || crate::decode::contains_ascii_case_insensitive(trimmed, "proxy-groups:")
        || crate::decode::contains_ascii_case_insensitive(trimmed, "mixed-port:")
    {
        return SubscriptionFormat::Clash;
    }

    if uri::looks_like_uri_list(trimmed) {
        return SubscriptionFormat::UriList;
    }

    SubscriptionFormat::Unknown
}

/// Parse raw content into normalized sing-box outbounds.
pub fn parse_subscription(
    raw: &str,
    format: SubscriptionFormat,
) -> Result<Vec<NormalizedOutbound>, SubscriptionError> {
    match format {
        SubscriptionFormat::SingBox => parse_singbox(raw),
        SubscriptionFormat::Clash => parse_clash(raw),
        SubscriptionFormat::UriList => uri::parse_uri_list_profile(raw).map(|p| p.nodes),
        SubscriptionFormat::Unknown => {
            let detected = detect_format(raw);
            match detected {
                SubscriptionFormat::Unknown => Err(SubscriptionError::UnknownFormat),
                other => parse_subscription(raw, other),
            }
        }
    }
}

/// Decode optional base64 wrapper, detect format, parse full profile.
pub fn normalize_raw_body(
    raw: &str,
) -> Result<(SubscriptionFormat, NormalizedProfile), SubscriptionError> {
    let decoded = decode_body(raw)?;
    let format = detect_format(&decoded);
    if format == SubscriptionFormat::Unknown {
        return Err(SubscriptionError::UnknownFormat);
    }
    let profile = parse_profile(&decoded, format)?;
    Ok((format, profile))
}

pub fn parse_profile(
    raw: &str,
    format: SubscriptionFormat,
) -> Result<NormalizedProfile, SubscriptionError> {
    match format {
        SubscriptionFormat::SingBox => parse_singbox_profile(raw),
        SubscriptionFormat::Clash => parse_clash_profile(raw),
        SubscriptionFormat::UriList => uri::parse_uri_list_profile(raw),
        SubscriptionFormat::Unknown => {
            let detected = detect_format(raw);
            match detected {
                SubscriptionFormat::Unknown => Err(SubscriptionError::UnknownFormat),
                other => parse_profile(raw, other),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn meta_from_profile(
    id: Uuid,
    name: String,
    url: String,
    format: SubscriptionFormat,
    profile: &NormalizedProfile,
    active: bool,
    etag: Option<String>,
    last_modified: Option<String>,
    auto_update: bool,
    auto_update_interval: Option<AutoUpdateInterval>,
) -> SubscriptionMeta {
    SubscriptionMeta {
        id,
        name,
        url,
        active,
        format,
        node_count: profile.nodes.len(),
        group_count: profile.groups.len(),
        rule_count: profile.route.rules.len(),
        has_dns: profile.dns.is_some(),
        parse_warnings: profile.parse_stats.warnings.clone(),
        last_updated: Some(Utc::now()),
        last_error: None,
        etag,
        last_modified,
        auto_update,
        auto_update_interval,
    }
}

fn meta_from_fetched_profile(
    current: &SubscriptionMeta,
    fetched: &FetchResponse,
    format: SubscriptionFormat,
    profile: &NormalizedProfile,
) -> SubscriptionMeta {
    meta_from_profile(
        current.id,
        current.name.clone(),
        current.url.clone(),
        format,
        profile,
        current.active,
        fetched.etag.clone().or_else(|| current.etag.clone()),
        fetched
            .last_modified
            .clone()
            .or_else(|| current.last_modified.clone()),
        current.auto_update,
        current.auto_update_interval,
    )
}

pub fn parse_singbox_profile(raw: &str) -> Result<NormalizedProfile, SubscriptionError> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| SubscriptionError::ParseFailed(format!("sing-box json: {e}")))?;

    let mut nodes = Vec::new();
    let mut groups = Vec::new();

    if let Some(outbounds) = value
        .get("outbounds")
        .or_else(|| value.get("endpoints"))
        .and_then(|v| v.as_array())
    {
        for (idx, item) in outbounds.iter().enumerate() {
            let ty = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let tag = item
                .get("tag")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("outbound-{idx}"));
            let entry = NormalizedOutbound {
                tag: tag.clone(),
                outbound: item.clone(),
            };
            match ty {
                "selector" | "urltest" | "fallback" | "loadbalance" | "load-balance" => {
                    groups.push(entry);
                }
                "direct" | "block" | "dns" => {}
                _ => nodes.push(entry),
            }
        }
    }

    if nodes.is_empty() {
        return Err(SubscriptionError::EmptyNodes);
    }

    let default_outbound = groups.first().map(|g| g.tag.clone());

    let route = if let Some(r) = value.get("route") {
        NormalizedRoute {
            rules: r
                .get("rules")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default(),
            final_outbound: r
                .get("final")
                .and_then(|v| v.as_str())
                .unwrap_or("direct")
                .to_string(),
            rule_sets: r
                .get("rule_set")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default(),
        }
    } else {
        NormalizedRoute {
            final_outbound: groups
                .first()
                .map(|g| g.tag.clone())
                .unwrap_or_else(|| "proxy".into()),
            ..Default::default()
        }
    };

    Ok(NormalizedProfile {
        nodes,
        groups,
        route,
        dns: value.get("dns").cloned(),
        default_outbound,
        parse_stats: ProfileParseStats::default(),
    })
}

pub fn parse_singbox(raw: &str) -> Result<Vec<NormalizedOutbound>, SubscriptionError> {
    Ok(parse_singbox_profile(raw)?.nodes)
}

fn parse_clash(raw: &str) -> Result<Vec<NormalizedOutbound>, SubscriptionError> {
    Ok(parse_clash_profile(raw)?.nodes)
}

fn name_from_disposition(cd: Option<&str>) -> Option<String> {
    let cd = cd?;
    // filename="foo.json" or filename=foo.json
    for part in cd.split(';') {
        let part = part.trim();
        if let Some(rest) = part
            .strip_prefix("filename=")
            .or_else(|| part.strip_prefix("filename*="))
        {
            let name = rest.trim().trim_matches('"');
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn name_from_url(url: &str) -> Option<String> {
    let path = url.split('?').next().unwrap_or(url);
    let seg = path.rsplit('/').next().unwrap_or("");
    if seg.is_empty() || seg == path {
        return None;
    }
    Some(seg.to_string())
}

pub fn resolve_subscription_name(
    user_name: Option<&str>,
    content_disposition: Option<&str>,
    url: &str,
    id: Uuid,
) -> String {
    if let Some(n) = user_name.map(str::trim).filter(|s| !s.is_empty()) {
        return n.to_string();
    }
    if let Some(n) = name_from_disposition(content_disposition) {
        return n;
    }
    if let Some(n) = name_from_url(url) {
        return n;
    }
    format!("Subscription-{}", short_id(&id))
}

/// Disk-backed subscription manager.
pub struct SubscriptionManager<F: HttpFetcher = DirectFetcher> {
    paths: SubscriptionPaths,
    fetcher: F,
}

/// Result of the network phase of an update; the disk phase consumes it via
/// `SubscriptionManager::apply_update`.
#[derive(Debug, Clone)]
pub struct FetchedUpdate {
    pub meta: SubscriptionMeta,
    pub fetched: FetchResponse,
}

/// Result of the network phase of an add; the disk phase consumes it via
/// `SubscriptionManager::apply_add`.
#[derive(Debug)]
pub struct FetchedAdd {
    pub id: Uuid,
    pub url: String,
    pub name: Option<String>,
    pub auto_update: bool,
    pub auto_update_interval: Option<AutoUpdateInterval>,
    pub fetched: FetchResponse,
}

impl SubscriptionManager<DirectFetcher> {
    pub fn open(paths: SubscriptionPaths) -> Self {
        Self {
            paths,
            fetcher: DirectFetcher,
        }
    }
}

impl<F: HttpFetcher> SubscriptionManager<F> {
    pub fn with_fetcher(paths: SubscriptionPaths, fetcher: F) -> Self {
        Self { paths, fetcher }
    }

    pub fn paths(&self) -> &SubscriptionPaths {
        &self.paths
    }

    pub fn list(&self) -> Result<Vec<SubscriptionMeta>, SubscriptionError> {
        Ok(load_index(&self.paths)?.items)
    }

    /// Import URL: fetch → parse → write success files. Empty/unknown → no success write.
    pub fn add(
        &self,
        url: &str,
        name: Option<&str>,
        auto_update: bool,
        auto_update_interval: Option<AutoUpdateInterval>,
    ) -> Result<SubscriptionMeta, SubscriptionError> {
        self.apply_add(self.fetch_add(url, name, auto_update, auto_update_interval)?)
    }

    /// Network fetch phase of an add: validate URL, GET (no disk writes).
    pub fn fetch_add(
        &self,
        url: &str,
        name: Option<&str>,
        auto_update: bool,
        auto_update_interval: Option<AutoUpdateInterval>,
    ) -> Result<FetchedAdd, SubscriptionError> {
        assert!(
            self.fetcher.bypasses_system_proxy(),
            "subscription fetch must bypass system proxy"
        );

        validate_subscription_url(url)?;

        let id = Uuid::new_v4();
        let fetched = self.fetcher.get(url, None, None)?;
        Ok(FetchedAdd {
            id,
            url: url.to_string(),
            name: name.map(str::to_string),
            auto_update,
            auto_update_interval,
            fetched,
        })
    }

    /// Disk phase of an add: normalize + persist (or leave no success artifacts on failure).
    pub fn apply_add(&self, add: FetchedAdd) -> Result<SubscriptionMeta, SubscriptionError> {
        let FetchedAdd {
            id,
            url,
            name,
            auto_update,
            auto_update_interval,
            fetched,
        } = add;
        match normalize_raw_body(&fetched.body) {
            Ok((format, profile)) => {
                let index = load_index(&self.paths)?;
                let make_active = index.items.iter().all(|m| !m.active);
                let meta = meta_from_profile(
                    id,
                    resolve_subscription_name(
                        name.as_deref(),
                        fetched.content_disposition.as_deref(),
                        &url,
                        id,
                    ),
                    url,
                    format,
                    &profile,
                    make_active,
                    fetched.etag,
                    fetched.last_modified,
                    auto_update,
                    auto_update_interval,
                );
                write_subscription_success(&self.paths, &meta, &fetched.body, &profile)?;
                Ok(meta)
            }
            Err(err) => {
                // Do not create success artifacts; leave no index entry.
                Err(err)
            }
        }
    }

    pub fn update(&self, id: Uuid) -> Result<SubscriptionMeta, SubscriptionError> {
        match self.fetch_update(id) {
            Ok(upd) => self.apply_update(upd),
            Err(err) => {
                write_subscription_error(&self.paths, id, err.to_string())?;
                Err(err)
            }
        }
    }

    /// Network fetch phase of an update: load meta, validate URL, GET (no disk writes).
    pub fn fetch_update(&self, id: Uuid) -> Result<FetchedUpdate, SubscriptionError> {
        assert!(self.fetcher.bypasses_system_proxy());

        let index = load_index(&self.paths)?;
        let meta = index
            .items
            .iter()
            .find(|m| m.id == id)
            .cloned()
            .ok_or_else(|| {
                SubscriptionError::ParseFailed(format!("subscription {id} not found"))
            })?;

        validate_subscription_url(&meta.url)?;

        let fetched = self.fetcher.get(
            &meta.url,
            meta.etag.as_deref(),
            meta.last_modified.as_deref(),
        )?;
        Ok(FetchedUpdate { meta, fetched })
    }

    /// Disk phase of an update: normalize + persist, or record `last_error`.
    pub fn apply_update(&self, upd: FetchedUpdate) -> Result<SubscriptionMeta, SubscriptionError> {
        // A subscription removed while its fetch was in flight must not be resurrected:
        // the index no longer contains the id, so stop before writing any files.
        let current = load_index(&self.paths)?
            .items
            .iter()
            .find(|m| m.id == upd.meta.id)
            .cloned()
            .ok_or_else(|| {
                SubscriptionError::ParseFailed(format!("subscription {} not found", upd.meta.id))
            })?;
        if upd.fetched.not_modified {
            return mark_subscription_refreshed(&self.paths, upd.meta.id);
        }
        match normalize_raw_body(&upd.fetched.body) {
            Ok((format, profile)) => {
                let updated = meta_from_fetched_profile(&current, &upd.fetched, format, &profile);
                write_subscription_success(&self.paths, &updated, &upd.fetched.body, &profile)?;
                Ok(updated)
            }
            Err(err) => {
                write_subscription_error(&self.paths, upd.meta.id, err.to_string())?;
                Err(err)
            }
        }
    }

    /// Network phase of updating every subscription. Fetches run in parallel (up to one
    /// `FETCH_TIMEOUT` of wall time instead of N×) and write nothing to disk, so the caller
    /// can run them without holding the orchestrate lock.
    pub fn fetch_all(&self) -> Vec<(Uuid, Result<FetchedUpdate, SubscriptionError>)>
    where
        F: Sync,
    {
        let ids: Vec<Uuid> = load_index(&self.paths)
            .map(|i| i.items.into_iter().map(|m| m.id).collect())
            .unwrap_or_default();
        self.fetch_ids(ids)
    }

    /// Network phase of updating the subscriptions with `auto_update` enabled. Same parallel
    /// fetch as [`SubscriptionManager::fetch_all`], but only touches the flagged entries so a
    /// background refresh never re-fetches subscriptions the user did not opt into.
    pub fn fetch_auto(&self) -> Vec<(Uuid, Result<FetchedUpdate, SubscriptionError>)>
    where
        F: Sync,
    {
        let ids: Vec<Uuid> = load_index(&self.paths)
            .map(|i| {
                i.items
                    .into_iter()
                    .filter(|m| m.auto_update)
                    .map(|m| m.id)
                    .collect()
            })
            .unwrap_or_default();
        self.fetch_ids(ids)
    }

    /// Parallel network phase for an explicit id list. Writes nothing to disk, so callers
    /// can run it without holding the orchestrate lock; [`SubscriptionManager::apply_all`]
    /// persists the results serially.
    pub fn fetch_ids(&self, ids: Vec<Uuid>) -> Vec<(Uuid, Result<FetchedUpdate, SubscriptionError>)>
    where
        F: Sync,
    {
        if ids.is_empty() {
            return Vec::new();
        }

        let worker_count = ids.len().min(MAX_FETCH_CONCURRENCY);
        let queue = std::sync::Arc::new(std::sync::Mutex::new(
            ids.into_iter()
                .enumerate()
                .collect::<std::collections::VecDeque<_>>(),
        ));
        let (sender, receiver) = std::sync::mpsc::channel();

        std::thread::scope(|scope| {
            for _ in 0..worker_count {
                let queue = std::sync::Arc::clone(&queue);
                let sender = sender.clone();
                scope.spawn(move || loop {
                    let job = queue
                        .lock()
                        .expect("subscription fetch queue poisoned")
                        .pop_front();
                    let Some((index, id)) = job else {
                        break;
                    };
                    let result = self.fetch_update(id);
                    sender
                        .send((index, id, result))
                        .expect("subscription fetch receiver dropped");
                });
            }
            drop(sender);
        });

        let mut completed: Vec<_> = receiver.into_iter().collect();
        completed.sort_unstable_by_key(|(index, _, _)| *index);
        completed
            .into_iter()
            .map(|(_, id, result)| (id, result))
            .collect()
    }

    /// Disk phase of [`SubscriptionManager::fetch_all`]: persists each fetched update
    /// serially so `index.json` writes never interleave. Under the orchestrate lock this
    /// cannot race with add/remove/set_active. The index is loaded once and written once
    /// (per-item updates used to re-read and fsync `index.json` twice per subscription).
    pub fn apply_all(
        &self,
        fetched: Vec<(Uuid, Result<FetchedUpdate, SubscriptionError>)>,
    ) -> Vec<(Uuid, Result<SubscriptionMeta, SubscriptionError>)> {
        let mut index = match load_index(&self.paths) {
            Ok(index) => index,
            Err(err) => {
                let message = err.to_string();
                return fetched
                    .into_iter()
                    .map(|(id, _)| (id, Err(SubscriptionError::ParseFailed(message.clone()))))
                    .collect();
            }
        };
        let mut out = Vec::with_capacity(fetched.len());
        for (id, result) in fetched {
            match result {
                Err(err) => {
                    apply_error_to_index(&self.paths, &mut index, id, err.to_string());
                    out.push((id, Err(err)));
                }
                Ok(upd) => {
                    // A subscription removed while its fetch was in flight must not be
                    // resurrected (same guard as apply_update).
                    let Some(current) = index.items.iter().find(|m| m.id == id).cloned() else {
                        out.push((
                            id,
                            Err(SubscriptionError::ParseFailed(format!(
                                "subscription {id} not found"
                            ))),
                        ));
                        continue;
                    };
                    if upd.fetched.not_modified {
                        let updated =
                            mark_refreshed_in_index(&self.paths, &mut index, id).unwrap_or(current);
                        out.push((id, Ok(updated)));
                        continue;
                    }
                    match normalize_raw_body(&upd.fetched.body) {
                        Ok((format, profile)) => {
                            let updated =
                                meta_from_fetched_profile(&current, &upd.fetched, format, &profile);
                            if let Err(err) = commit_subscription_success(
                                &self.paths,
                                &updated,
                                &upd.fetched.body,
                                &profile,
                            ) {
                                out.push((id, Err(err)));
                                continue;
                            }
                            apply_success_to_index(&mut index, &updated);
                            out.push((id, Ok(updated)));
                        }
                        Err(err) => {
                            apply_error_to_index(&self.paths, &mut index, id, err.to_string());
                            out.push((id, Err(err)));
                        }
                    }
                }
            }
        }
        if let Err(err) = save_index(&self.paths, &index) {
            // The index write is the single commit point of the batch; surface
            // the failure on every item so the caller cannot treat the batch
            // as fully persisted.
            let message = err.to_string();
            for (_, item) in out.iter_mut() {
                *item = Err(SubscriptionError::ParseFailed(message.clone()));
            }
        }
        out
    }

    /// Update every subscription. Network fetches run in parallel, disk writes stay
    /// serialized so `index.json` updates never interleave.
    pub fn update_all(&self) -> Vec<(Uuid, Result<SubscriptionMeta, SubscriptionError>)>
    where
        F: Sync,
    {
        self.apply_all(self.fetch_all())
    }

    pub fn remove(&self, id: Uuid) -> Result<(), SubscriptionError> {
        remove_subscription(&self.paths, id)
    }

    pub fn set_active(
        &self,
        id: Uuid,
        active: bool,
    ) -> Result<SubscriptionMeta, SubscriptionError> {
        set_active(&self.paths, id, active)
    }

    pub fn set_auto_update(
        &self,
        id: Uuid,
        auto_update: bool,
        auto_update_interval: Option<AutoUpdateInterval>,
    ) -> Result<SubscriptionMeta, SubscriptionError> {
        set_auto_update(&self.paths, id, auto_update, auto_update_interval)
    }

    pub fn active_profile(&self) -> Result<NormalizedProfile, SubscriptionError> {
        let index = load_index(&self.paths)?;
        load_active_profile(&self.paths, &index)
    }
}

/// In-memory placeholder kept for early shell wiring (prefer `SubscriptionManager::open`).
#[derive(Debug, Default)]
pub struct MemorySubscriptionManager {
    index: SubscriptionIndex,
}

impl MemorySubscriptionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn list(&self) -> &[SubscriptionMeta] {
        &self.index.items
    }

    pub fn add_placeholder(&mut self, name: String, url: String) -> SubscriptionMeta {
        let meta = SubscriptionMeta {
            id: Uuid::new_v4(),
            name,
            url,
            active: false,
            format: SubscriptionFormat::Unknown,
            node_count: 0,
            group_count: 0,
            rule_count: 0,
            has_dns: false,
            parse_warnings: vec![],
            last_updated: None,
            last_error: None,
            etag: None,
            last_modified: None,
            auto_update: false,
            auto_update_interval: None,
        };
        self.index.items.push(meta.clone());
        meta
    }
}
