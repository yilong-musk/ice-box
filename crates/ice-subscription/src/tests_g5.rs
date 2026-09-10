// SPDX-License-Identifier: GPL-3.0-or-later

use super::{
    detect_format, load_active_profile, load_active_profile_with_default_rules, load_index,
    normalize_raw_body, parse_clash_with_stats, parse_singbox, parse_singbox_profile,
    parse_uri_list_profile, resolve_selected_tag, set_active, set_auto_update,
    write_subscription_error, AutoUpdateInterval, DirectFetcher, FetchResponse, FetchedUpdate,
    HttpFetcher, MockFetchMode, MockFetcher, PanicOnceMode, SubscriptionError, SubscriptionFormat,
    SubscriptionManager, SubscriptionMeta, SubscriptionPaths, CLASH_SUPPORTED_TYPES,
    MAX_CLASH_PROXIES, MAX_URI_LINES,
};
use base64::Engine;
use chrono::{Duration as ChronoDuration, Utc};
use ice_config::{
    build_runtime_config, BuildInput, HostPlatform, LocalTemplate, NormalizedOutbound,
    NormalizedProfile,
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
    let raw =
        fs::read_to_string(fixtures_dir().join("subscription-singbox-empty-nodes.json")).unwrap();
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
    let raw =
        fs::read_to_string(fixtures_dir().join("subscription-singbox-full-config.json")).unwrap();
    let nodes = parse_singbox(&raw).unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].tag, "node-a");
    assert_eq!(nodes[0].outbound["type"], "socks");
}

#[test]
fn singbox_parse_skips_disallowed_outbound_types() {
    let raw = r#"{
            "outbounds": [
                { "type": "socks", "tag": "ok", "server": "1.1.1.1", "server_port": 1080 },
                { "type": "tor", "tag": "evil", "executable_path": "/usr/bin/tor" }
            ]
        }"#;
    let profile = parse_singbox_profile(raw).unwrap();
    assert_eq!(profile.nodes.len(), 1);
    assert_eq!(profile.nodes[0].tag, "ok");
    assert_eq!(profile.parse_stats.skipped_proxies, 1);
    assert!(profile
        .parse_stats
        .warnings
        .iter()
        .any(|w| w.to_string().contains("tor")));
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
    assert!(m
        .last_error
        .as_ref()
        .unwrap()
        .to_string()
        .contains("network down"));
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
    let body = r#"{"outbounds":[{"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}]}"#;
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
    let body = r#"{"outbounds":[{"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}]}"#;
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
            body:
                r#"{"outbounds":[{"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}]}"#
                    .into(),
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
    let body = r#"{"outbounds":[{"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}]}"#;
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
            body:
                r#"{"outbounds":[{"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}]}"#
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
    let profile = load_active_profile(&paths, &index, HostPlatform::MacOs).unwrap();
    assert_eq!(profile.nodes.len(), 1);
    assert_eq!(profile.nodes[0].tag, "same");

    set_active(&paths, ids[1], true).unwrap();
    let index = load_index(&paths).unwrap();
    assert_eq!(index.items.iter().filter(|m| m.active).count(), 1);
    let profile = load_active_profile(&paths, &index, HostPlatform::MacOs).unwrap();
    assert_eq!(profile.nodes.len(), 1, "switch loads the other profile");
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn g5_8_selected_via_resolve_and_build() {
    let outbounds = vec![
        NormalizedOutbound {
            tag: "one".into(),
            outbound: std::sync::Arc::new(
                serde_json::json!({"type":"socks","tag":"one","server":"x","server_port":1}),
            ),
        },
        NormalizedOutbound {
            tag: "two".into(),
            outbound: std::sync::Arc::new(
                serde_json::json!({"type":"socks","tag":"two","server":"y","server_port":1}),
            ),
        },
    ];
    let profile = NormalizedProfile::from_nodes_only(outbounds);
    let sel = resolve_selected_tag(Some("missing"), &profile);
    assert_eq!(sel.as_deref(), Some("one"));
    let cfg = build_runtime_config(&BuildInput {
        template: LocalTemplate::default(),
        profile: Arc::new(profile),
        selected_tag: sel,
        geoip_rule_set_dir: None,
        group_selections: Default::default(),
        rule_overrides: Default::default(),
        capture_intent: Default::default(),
        platform: HostPlatform::MacOs,
    })
    .unwrap()
    .to_json_value()
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
    let (format, profile) = normalize_raw_body(&encoded, HostPlatform::MacOs).unwrap();
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
    let result = parse_clash_with_stats(&raw, HostPlatform::MacOs).unwrap();
    assert_eq!(result.profile.nodes.len(), 1);
    assert_node_shape(&result.profile.nodes[0], "shadowsocks");
    assert_eq!(result.profile.nodes[0].outbound["method"], "aes-256-gcm");
}

#[test]
fn g6_2_clash_vmess() {
    let raw = fs::read_to_string(fixtures_dir().join("subscription-clash-vmess.yaml")).unwrap();
    let result = parse_clash_with_stats(&raw, HostPlatform::MacOs).unwrap();
    assert_eq!(result.profile.nodes.len(), 1);
    assert_node_shape(&result.profile.nodes[0], "vmess");
    assert_eq!(result.profile.nodes[0].outbound["transport"]["type"], "ws");
}

#[test]
fn g6_3_clash_trojan() {
    let raw = fs::read_to_string(fixtures_dir().join("subscription-clash-trojan.yaml")).unwrap();
    let result = parse_clash_with_stats(&raw, HostPlatform::MacOs).unwrap();
    assert_eq!(result.profile.nodes.len(), 1);
    assert_node_shape(&result.profile.nodes[0], "trojan");
    assert_eq!(result.profile.nodes[0].outbound["tls"]["enabled"], true);
}

#[test]
fn g6_4_clash_socks() {
    let raw = fs::read_to_string(fixtures_dir().join("subscription-clash-socks.yaml")).unwrap();
    let result = parse_clash_with_stats(&raw, HostPlatform::MacOs).unwrap();
    assert_eq!(result.profile.nodes.len(), 1);
    assert_node_shape(&result.profile.nodes[0], "socks");
}

#[test]
fn g6_5_clash_http() {
    let raw = fs::read_to_string(fixtures_dir().join("subscription-clash-http.yaml")).unwrap();
    let result = parse_clash_with_stats(&raw, HostPlatform::MacOs).unwrap();
    assert_eq!(result.profile.nodes.len(), 1);
    assert_node_shape(&result.profile.nodes[0], "http");
}

#[test]
fn g6_6_mixed_ignores_proxy_groups() {
    let raw = fs::read_to_string(fixtures_dir().join("subscription-clash-mixed.yaml")).unwrap();
    let result = parse_clash_with_stats(&raw, HostPlatform::MacOs).unwrap();
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
    let raw =
        fs::read_to_string(fixtures_dir().join("subscription-clash-unknown-only.yaml")).unwrap();
    let err = parse_clash_with_stats(&raw, HostPlatform::MacOs).expect_err("empty");
    assert!(matches!(err, SubscriptionError::EmptyNodes));
    assert_eq!(err.code().as_str(), "sub.empty");
}

#[test]
fn g6_8_known_plus_unknown_skip_count() {
    let raw =
        fs::read_to_string(fixtures_dir().join("subscription-clash-mixed-unknown.yaml")).unwrap();
    let result = parse_clash_with_stats(&raw, HostPlatform::MacOs).unwrap();
    assert_eq!(result.profile.nodes.len(), 2);
    assert!(result.profile.parse_stats.skipped_proxies >= 1);
}

#[test]
fn g6_8b_clash_truncates_too_many_proxies() {
    let mut raw = String::from("proxies:\n");
    for i in 0..=MAX_CLASH_PROXIES {
        raw.push_str(&format!(
                "  - {{ type: ss, name: n{i}, server: 1.1.1.1, port: 443, cipher: aes-128-gcm, password: x }}\n"
            ));
    }
    let result =
        parse_clash_with_stats(&raw, HostPlatform::MacOs).expect("truncate, do not hard-fail");
    assert_eq!(result.profile.nodes.len(), MAX_CLASH_PROXIES);
    assert!(result
        .profile
        .parse_stats
        .warnings
        .iter()
        .any(|w| w.key == "parse.truncated"));
}

#[test]
fn singbox_truncates_too_many_nodes() {
    let mut outbounds = Vec::new();
    for i in 0..=MAX_CLASH_PROXIES {
        outbounds.push(format!(
            r#"{{"type":"socks","tag":"n{i}","server":"1.1.1.1","server_port":1080}}"#
        ));
    }
    let raw = format!(r#"{{"outbounds":[{}]}}"#, outbounds.join(","));
    let profile = parse_singbox_profile(&raw).expect("truncate, do not hard-fail");
    assert_eq!(profile.nodes.len(), MAX_CLASH_PROXIES);
    assert!(profile
        .parse_stats
        .warnings
        .iter()
        .any(|w| w.key == "parse.truncated"));
}

#[test]
fn singbox_profile_drops_remote_rule_sets_and_hosts_dns() {
    let raw = r#"{
        "outbounds": [
            {"type":"socks","tag":"n","server":"1.1.1.1","server_port":1080}
        ],
        "route": {
            "final": "n",
            "rules": [
                {"rule_set":["evil"],"outbound":"n"},
                {"domain_suffix":["keep.com"],"outbound":"n"}
            ],
            "rule_set": [
                {"type":"remote","tag":"evil","url":"https://evil.example/x.srs"},
                {"type":"local","tag":"ok","path":"geoip/ok.srs"}
            ]
        },
        "dns": {
            "servers": [
                {"type":"hosts","tag":"hosts","path":"/etc/passwd"},
                {"type":"local","tag":"local"}
            ],
            "final": "local"
        }
    }"#;
    let profile = parse_singbox_profile(raw).expect("parse");
    assert!(profile.route.rule_sets.iter().all(|s| s["tag"] != "evil"));
    assert!(profile.route.rule_sets.iter().any(|s| s["tag"] == "ok"));
    assert!(profile
        .route
        .rules
        .iter()
        .all(|r| r.get("rule_set") != Some(&serde_json::json!(["evil"]))));
    assert!(profile.dns.as_ref().unwrap()["servers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["type"] != "hosts"));
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
    let (format, profile) = normalize_raw_body(&raw, HostPlatform::MacOs).unwrap();
    assert_eq!(format, SubscriptionFormat::UriList);
    assert_eq!(profile.nodes.len(), 14, "15 lines, only ssr:// skipped");
    assert_eq!(profile.parse_stats.skipped_proxies, 1);
    assert!(
        profile
            .parse_stats
            .warnings
            .iter()
            .any(|w| w.to_string().contains("ssr://")),
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
    let profile =
        load_active_profile(&paths, &load_index(&paths).unwrap(), HostPlatform::MacOs).unwrap();
    assert_eq!(profile.nodes.len(), 14);
    // Load-time defaults: 3 split-routing rules + built-in DNS.
    assert_eq!(profile.route.rules.len(), 3);
    assert_eq!(profile.route.rules[1]["geoip"][0], "cn");
    assert!(profile.route.rules[2]["domain_suffix"]
        .as_array()
        .is_some_and(|a| a.len() > 100));
    assert_eq!(profile.route.final_outbound, "proxy");
    let dns = profile.dns.clone().expect("built-in dns block");
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
    let raw_profile = load_active_profile_with_default_rules(
        &paths,
        &load_index(&paths).unwrap(),
        false,
        HostPlatform::MacOs,
    )
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
    let profile = parse_uri_list_profile(&raw).expect("truncate, do not hard-fail");
    assert_eq!(profile.nodes.len(), MAX_URI_LINES);
    assert!(profile
        .parse_stats
        .warnings
        .iter()
        .any(|w| w.key == "parse.truncated"));
}

#[test]
fn fetch_ids_continues_after_injected_panic() {
    let paths = temp_subs("panic-worker");
    let body =
        r#"{"outbounds":[{"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1080}]}"#;
    let once = PanicOnceMode::new(FetchResponse {
        body: body.into(),
        not_modified: false,
        etag: None,
        last_modified: None,
        content_disposition: None,
    });
    let fetcher = MockFetcher {
        bypasses_proxy: true,
        mode: MockFetchMode::PanicOnce(once.clone()),
    };
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
    let a = mgr.add("https://example.com/a", None, false, None).unwrap();
    let b = mgr.add("https://example.com/b", None, false, None).unwrap();
    once.arm();
    let results = mgr.fetch_ids(vec![a.id, b.id]);
    assert_eq!(results.len(), 2);
    assert!(
        results.iter().any(|(_, r)| {
            r.as_ref()
                .err()
                .is_some_and(|e| e.to_string().contains("panicked"))
        }),
        "one job must surface the injected panic"
    );
    assert!(
        mgr.fetch_workers_alive(),
        "a per-job panic must not mark the worker pool dead"
    );
    assert!(
        results.iter().any(|(_, r)| r.is_ok()),
        "the next job must still run after a panic"
    );
    let _ = fs::remove_dir_all(paths.root());
}
