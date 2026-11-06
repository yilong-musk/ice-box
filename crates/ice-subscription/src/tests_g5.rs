// SPDX-License-Identifier: GPL-3.0-or-later

use super::{
    detect_format, load_active_profile, load_active_profile_with_default_rules, load_index,
    normalize_raw_body, parse_clash_with_stats, parse_singbox, parse_singbox_profile,
    parse_uri_list_profile, read_profile, resolve_selected_tag, set_active, set_auto_update,
    subscription_share_text, write_subscription_error, AutoUpdateInterval, DefaultFetcher,
    DirectFetcher, FetchClient, FetchResponse, FetchedUpdate, HttpFetcher, MockFetchMode,
    MockFetcher, PanicOnceMode, SubscriptionError, SubscriptionFormat, SubscriptionManager,
    SubscriptionMeta, SubscriptionPaths, SubscriptionShareKind, SubscriptionSource,
    SubscriptionUserInfo, CLASH_SUPPORTED_TYPES, MAX_CLASH_PROXIES, MAX_URI_LINES,
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

    fn get_as(
        &self,
        _url: &str,
        _etag: Option<&str>,
        _last_modified: Option<&str>,
        _client: FetchClient,
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
                userinfo: None,
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
            userinfo: None,
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
fn singbox_parse_keeps_fallback_and_loadbalance_as_groups() {
    let raw = r#"{
            "outbounds": [
                { "type": "socks", "tag": "ok", "server": "1.1.1.1", "server_port": 1080 },
                {
                    "type": "fallback",
                    "tag": "fb",
                    "outbounds": ["ok"],
                    "url": "http://www.gstatic.com/generate_204"
                },
                {
                    "type": "loadbalance",
                    "tag": "lb",
                    "outbounds": ["ok"],
                    "strategy": "round-robin"
                }
            ]
        }"#;
    let profile = parse_singbox_profile(raw).unwrap();
    assert_eq!(profile.nodes.len(), 1);
    assert_eq!(profile.nodes[0].tag, "ok");
    let tags: Vec<_> = profile.groups.iter().map(|g| g.tag.as_str()).collect();
    assert!(tags.contains(&"fb"), "fallback must be a group, not a node");
    assert!(
        tags.contains(&"lb"),
        "loadbalance must be a group, not a node"
    );
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
            userinfo: None,
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
            userinfo: None,
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
                    spooled: None,
                    fetched_as: FetchClient::Generic,
                    fetched: FetchResponse {
                        body: r#"{"outbounds":[{"type":"socks","tag":"x","server":"1.1.1.1","server_port":1},{"type":"socks","tag":"y","server":"2.2.2.2","server_port":2}]}"#
                            .into(),
                        not_modified: false,
                        etag: Some("v2".into()),
                        last_modified: None,
                        userinfo: None,
                        content_disposition: None,
                    },
                }),
            ),
            (
                b.id,
                Ok(FetchedUpdate {
                    meta: b_meta,
                    spooled: None,
                    fetched_as: FetchClient::Generic,
                    fetched: FetchResponse {
                        body: String::new(),
                        not_modified: true,
                        etag: None,
                        last_modified: None,
                        userinfo: None,
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
fn fetch_ids_caps_network_concurrency() {
    let paths = temp_subs("fetch-pool");
    let fetcher = ConcurrencyFetcher {
        active: Arc::new(AtomicUsize::new(0)),
        max_active: Arc::new(AtomicUsize::new(0)),
    };
    let max_active = Arc::clone(&fetcher.max_active);
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
    let count = super::MAX_FETCH_CONCURRENCY + 4;
    let mut ids = Vec::with_capacity(count);
    for index in 0..count {
        let meta = mgr
            .add(&format!("https://example.com/{index}"), None, false, None)
            .expect("add subscription");
        ids.push(meta.id);
    }

    let updates = mgr.fetch_ids(ids);
    assert_eq!(updates.len(), count);
    for (_, update) in &updates {
        let update = update.as_ref().expect("fetch succeeded");
        assert!(
            update.fetched.body.is_empty(),
            "batch bodies must not remain in memory"
        );
        assert!(update.spooled.is_some(), "completed bodies are disk-backed");
    }
    assert!(
        max_active.load(Ordering::SeqCst) > 1,
        "fetch_ids should retain useful parallelism"
    );
    assert!(
        max_active.load(Ordering::SeqCst) <= super::MAX_FETCH_CONCURRENCY,
        "fetch_ids exceeded its worker limit"
    );
    assert!(
        mgr.apply_all(updates)
            .iter()
            .all(|(_, result)| result.is_ok()),
        "spooled bodies must remain readable until apply"
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
            userinfo: None,
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
            userinfo: None,
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
            userinfo: None,
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
            userinfo: None,
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
fn userinfo_header_is_persisted_and_refreshed() {
    let paths = temp_subs("userinfo");
    let body = r#"{"outbounds":[{"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}]}"#;
    let with_header = MockFetcher {
        bypasses_proxy: true,
        mode: MockFetchMode::Ok(FetchResponse {
            body: body.into(),
            not_modified: false,
            etag: None,
            last_modified: None,
            userinfo: Some(SubscriptionUserInfo {
                upload: 10,
                download: 90,
                total: 1000,
                expire: Some(1_790_000_000),
            }),
            content_disposition: None,
        }),
    };
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), with_header);
    let meta = mgr
        .add("https://example.com/s", Some("s"), false, None)
        .unwrap();
    assert_eq!(meta.userinfo.unwrap().used(), 100);
    let disk_meta: SubscriptionMeta =
        serde_json::from_str(&fs::read_to_string(paths.meta(meta.id)).unwrap()).unwrap();
    assert_eq!(
        disk_meta.userinfo.unwrap().remaining(),
        Some(900),
        "counters must survive a reload of meta.json"
    );

    // A later fetch that omits the header keeps the last counters the provider
    // reported instead of clearing them.
    let without_header = MockFetcher {
        bypasses_proxy: true,
        mode: MockFetchMode::Ok(FetchResponse {
            body: body.into(),
            not_modified: false,
            etag: None,
            last_modified: None,
            userinfo: None,
            content_disposition: None,
        }),
    };
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), without_header);
    let updated = mgr.update(meta.id).unwrap();
    assert_eq!(updated.userinfo.unwrap().total, 1000);

    // A conditional (304) refresh carries no body but may refresh the counters.
    let conditional = MockFetcher {
        bypasses_proxy: true,
        mode: MockFetchMode::Ok(FetchResponse {
            body: String::new(),
            not_modified: true,
            etag: None,
            last_modified: None,
            userinfo: Some(SubscriptionUserInfo {
                upload: 20,
                download: 180,
                total: 1000,
                expire: None,
            }),
            content_disposition: None,
        }),
    };
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), conditional);
    let refreshed = mgr.update(meta.id).unwrap();
    let info = refreshed.userinfo.unwrap();
    assert_eq!(info.used(), 200);
    assert_eq!(info.expire, None);
    let persisted = load_index(&paths)
        .unwrap()
        .items
        .into_iter()
        .find(|item| item.id == meta.id)
        .unwrap();
    assert_eq!(persisted.userinfo.unwrap().download, 180);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn provider_info_entries_are_recorded_and_persisted() {
    let paths = temp_subs("provider-info");
    // Panel-style entries: the provider publishes quota and expiry as extra
    // "proxies" whose names carry the text.
    let body = r#"proxies:
  - {name: "Traffic: 11.84 GB | 150 GB", server: 203.0.113.30, port: 443, type: trojan, password: "p"}
  - {name: "Expire: 2026-09-26", server: 203.0.113.30, port: 443, type: trojan, password: "p"}
  - {name: "🇭🇰 香港实验性 IEPL 专线 1", server: 203.0.113.31, port: 443, type: trojan, password: "p"}
"#;
    let fetcher = MockFetcher {
        bypasses_proxy: true,
        mode: MockFetchMode::Ok(FetchResponse {
            body: body.into(),
            not_modified: false,
            etag: None,
            last_modified: None,
            userinfo: None,
            content_disposition: None,
        }),
    };
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
    let meta = mgr
        .add("https://example.com/s", Some("s"), false, None)
        .unwrap();
    assert_eq!(
        meta.node_count, 1,
        "info entries are omitted from the node list"
    );
    assert_eq!(
        meta.provider_info,
        vec!["Traffic: 11.84 GB | 150 GB", "Expire: 2026-09-26"]
    );
    let stored = read_profile(&paths, meta.id).unwrap();
    assert_eq!(stored.nodes.len(), 1);
    assert_eq!(stored.nodes[0].tag, "🇭🇰 香港实验性 IEPL 专线 1");
    let disk_meta: SubscriptionMeta =
        serde_json::from_str(&fs::read_to_string(paths.meta(meta.id)).unwrap()).unwrap();
    assert_eq!(disk_meta.provider_info, meta.provider_info);
    let indexed = load_index(&paths)
        .unwrap()
        .items
        .into_iter()
        .find(|item| item.id == meta.id)
        .unwrap();
    assert_eq!(indexed.provider_info, meta.provider_info);
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
            userinfo: None,
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
                userinfo: None,
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
        rule_set_cache_dir: None,
        group_selections: Default::default(),
        rule_overrides: Default::default(),
        capture_intent: Default::default(),
        tun_exclude_package: None,
        shared_root: None,
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
            userinfo: None,
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
            userinfo: None,
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
            userinfo: None,
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
    assert!(DefaultFetcher::new(None).unwrap().bypasses_system_proxy());
    assert!(!DefaultFetcher::new(Some("http://127.0.0.1:17890".into()))
        .unwrap()
        .bypasses_system_proxy());
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
fn g5_12b_update_fetch_works_when_the_fetcher_uses_a_proxy() {
    let paths = temp_subs("proxy-update");
    let fetcher = MockFetcher {
        bypasses_proxy: false,
        mode: MockFetchMode::Ok(FetchResponse {
            body: r#"{"outbounds":[{"type":"socks","tag":"node","server":"1.1.1.1","server_port":1}]}"#.into(),
            not_modified: false,
            etag: None,
            last_modified: None,
            userinfo: None,
            content_disposition: None,
        }),
    };
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
    // `.invalid` would fail a local DNS check; a proxied fetcher resolves it.
    let meta = mgr
        .add("https://does-not-resolve.invalid/sub", None, false, None)
        .unwrap();
    let fetched = mgr
        .fetch_update(meta.id)
        .expect("a proxied update fetch must not be treated as misuse");
    assert_eq!(fetched.meta.id, meta.id);
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
fn singbox_profile_downloads_https_rule_sets_and_drops_hosts_dns() {
    let raw = r#"{
        "outbounds": [
            {"type":"socks","tag":"n","server":"1.1.1.1","server_port":1080}
        ],
        "route": {
            "final": "n",
            "rules": [
                {"rule_set":["evil"],"outbound":"n"},
                {"rule_set":["remote"],"outbound":"n"},
                {"domain_suffix":["keep.com"],"outbound":"n"}
            ],
            "rule_set": [
                {"type":"remote","tag":"evil","url":"http://evil.example/x.srs"},
                {"type":"remote","tag":"remote","url":"https://rules.example/x.srs","download_detour":"n"},
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
    // Plain-http sets cannot be downloaded and are dropped with their rules.
    assert!(profile.route.rule_sets.iter().all(|s| s["tag"] != "evil"));
    assert!(profile.route.rule_sets.iter().any(|s| s["tag"] == "ok"));
    // An https set becomes an app download; the core never fetches it.
    let remote = profile
        .route
        .rule_sets
        .iter()
        .find(|s| s["tag"] == "remote")
        .expect("download marker");
    assert_eq!(
        remote,
        &serde_json::json!({"type":"download","tag":"remote","url":"https://rules.example/x.srs"})
    );
    assert!(profile
        .route
        .rules
        .iter()
        .any(|r| r.get("rule_set") == Some(&serde_json::json!(["remote"]))));
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
            userinfo: None,
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
    assert_eq!(
        profile.nodes.len(),
        14,
        "16 lines; ssr:// and ss plugin skipped"
    );
    assert_eq!(profile.parse_stats.skipped_proxies, 2);
    assert!(
        profile
            .parse_stats
            .warnings
            .iter()
            .any(|w| w.to_string().contains("ssr://")),
        "ssr skip must surface as a warning"
    );
    assert!(
        profile
            .parse_stats
            .warnings
            .iter()
            .any(|w| w.to_string().contains("plugin")),
        "ss plugin skip must surface as a warning"
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
        "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI"
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

    // hysteria2 node with mport: the port range becomes sing-box
    // `server_ports` (`:`-separated) while the fixed port is kept.
    let hy2_mport = profile
        .nodes
        .iter()
        .find(|n| n.tag == "香港机场05|BGP|新加坡")
        .expect("hy2 mport node");
    assert_eq!(hy2_mport.outbound["server_port"], 60000);
    assert_eq!(
        hy2_mport.outbound["server_ports"],
        serde_json::json!(["60000:65530"])
    );

    // hysteria2 node: password is userinfo; pinSHA256 dropped, obfs kept.
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
            userinfo: None,
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
    assert_eq!(profile.route.rules[1]["geosite"][0], "cn");
    assert_eq!(profile.route.rules[2]["geoip"][0], "cn");
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
        userinfo: None,
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

#[test]
fn html_page_is_an_unknown_subscription_not_a_panic() {
    let html = "<!DOCTYPE html><html><body>proxies:\n  - name: no</body></html>";
    let err = normalize_raw_body(html, HostPlatform::Android).expect_err("html");
    assert!(matches!(err, SubscriptionError::UnknownFormat));

    let err = normalize_raw_body("<HTML><BODY>proxies: []</BODY></HTML>", HostPlatform::Ios)
        .expect_err("html");
    assert!(matches!(err, SubscriptionError::UnknownFormat));
}

#[test]
fn share_returns_the_full_url_and_a_singbox_document() {
    let paths = temp_subs("share");
    let body = r#"{"outbounds":[{"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}]}"#;
    let fetcher = MockFetcher {
        bypasses_proxy: true,
        mode: MockFetchMode::Ok(FetchResponse {
            body: body.into(),
            not_modified: false,
            etag: None,
            last_modified: None,
            userinfo: None,
            content_disposition: None,
        }),
    };
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
    let meta = mgr
        .add(
            "https://example.com/s?token=secret",
            Some("shared"),
            false,
            None,
        )
        .unwrap();

    let url = subscription_share_text(&paths, meta.id, SubscriptionShareKind::Url).unwrap();
    assert_eq!(url, "https://example.com/s?token=secret");

    let json = subscription_share_text(&paths, meta.id, SubscriptionShareKind::Singbox).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(value["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .any(|outbound| outbound["tag"] == "n1"));
    assert!(value.get("inbounds").is_none());
    assert_eq!(
        value["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|outbound| outbound["tag"] == "n1")
            .unwrap()["server"],
        "1.1.1.1"
    );

    let missing = subscription_share_text(&paths, Uuid::nil(), SubscriptionShareKind::Url);
    assert!(matches!(missing, Err(SubscriptionError::NotFound)));
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn share_singbox_document_keeps_remote_rule_sets_and_endpoints() {
    let paths = temp_subs("share-full");
    let body = r#"{
        "inbounds": [{"type": "mixed", "tag": "in", "listen": "127.0.0.1", "listen_port": 2080}],
        "outbounds": [
            {"type": "socks", "tag": "n1", "server": "1.1.1.1", "server_port": 1},
            {"type": "selector", "tag": "proxy", "outbounds": ["n1", "wg"], "default": "n1"},
            {"type": "dns", "tag": "dns-out"}
        ],
        "endpoints": [{
            "type": "wireguard",
            "tag": "wg",
            "private_key": "priv",
            "address": ["10.0.0.2/32"],
            "peers": [{"address": "1.2.3.4", "port": 51820, "public_key": "pub"}]
        }],
        "route": {
            "final": "proxy",
            "rules": [
                {"protocol": "dns", "outbound": "dns-out"},
                {"rule_set": "geosite-cn", "outbound": "proxy"}
            ],
            "rule_set": [{
                "type": "remote",
                "tag": "geosite-cn",
                "format": "binary",
                "url": "https://cdn.example/geosite-cn.srs"
            }]
        }
    }"#;
    let fetcher = MockFetcher {
        bypasses_proxy: true,
        mode: MockFetchMode::Ok(FetchResponse {
            body: body.into(),
            not_modified: false,
            etag: None,
            last_modified: None,
            userinfo: None,
            content_disposition: None,
        }),
    };
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
    let meta = mgr
        .add("https://example.com/full", Some("full"), false, None)
        .unwrap();
    let json = subscription_share_text(&paths, meta.id, SubscriptionShareKind::Singbox).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(value.get("inbounds").is_none());
    assert_eq!(value["endpoints"][0]["tag"], "wg");
    assert_eq!(value["endpoints"][0]["private_key"], "priv");
    assert!(value["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .any(|outbound| outbound["tag"] == "dns-out"));
    assert_eq!(
        value["route"]["rule_set"][0]["url"],
        "https://cdn.example/geosite-cn.srs"
    );
    assert!(value["route"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .any(|rule| rule["rule_set"] == "geosite-cn"));
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn share_omits_quota_and_expiry_nodes_hidden_from_the_node_list() {
    let paths = temp_subs("share-info");
    let body = r#"{
        "outbounds": [
            {"type": "socks", "tag": "剩余流量：1 GB", "server": "1.1.1.1", "server_port": 1},
            {"type": "socks", "tag": "套餐到期：2026-01-01", "server": "1.1.1.1", "server_port": 1},
            {"type": "socks", "tag": "hk", "server": "8.8.8.8", "server_port": 1},
            {
                "type": "selector",
                "tag": "proxy",
                "outbounds": ["剩余流量：1 GB", "套餐到期：2026-01-01", "hk"],
                "default": "剩余流量：1 GB"
            }
        ]
    }"#;
    let fetcher = MockFetcher {
        bypasses_proxy: true,
        mode: MockFetchMode::Ok(FetchResponse {
            body: body.into(),
            not_modified: false,
            etag: None,
            last_modified: None,
            userinfo: None,
            content_disposition: None,
        }),
    };
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
    let meta = mgr
        .add("https://example.com/info", Some("info"), false, None)
        .unwrap();
    let json = subscription_share_text(&paths, meta.id, SubscriptionShareKind::Singbox).unwrap();
    assert!(!json.contains("剩余流量"));
    assert!(!json.contains("套餐到期"));
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let proxy = value["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|outbound| outbound["tag"] == "proxy")
        .unwrap();
    assert_eq!(proxy["default"], "hk");
    assert_eq!(proxy["outbounds"], serde_json::json!(["hk"]));
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn clash_shaped_garbage_does_not_panic() {
    let result = std::panic::catch_unwind(|| {
        normalize_raw_body("proxies: *not yaml*\n", HostPlatform::MacOs)
    });
    assert!(result.is_ok(), "a parser panic must become an error");
    assert!(result.unwrap().is_err());
}

#[test]
fn file_import_persists_a_singbox_document_and_skips_refresh() {
    let paths = temp_subs("file-import");
    let body = "\u{feff}{\"outbounds\":[{\"type\":\"socks\",\"tag\":\"n1\",\"server\":\"1.1.1.1\",\"server_port\":1}]}";
    let mgr = SubscriptionManager::open(clone_paths(&paths), HostPlatform::MacOs);
    let meta = mgr.import_file(body, Some("  tokyo\n")).unwrap();
    assert_eq!(meta.name, "tokyo");
    assert_eq!(meta.source, SubscriptionSource::File);
    assert!(!meta.auto_update);
    assert!(meta.auto_update_interval.is_none());
    assert!(meta.url.is_empty());
    assert!(meta.active);
    assert_eq!(meta.node_count, 1);
    assert_eq!(meta.format, SubscriptionFormat::SingBox);

    let stored = load_index(&paths).unwrap();
    assert_eq!(stored.items[0].source, SubscriptionSource::File);

    let clash = "proxies:\n  - {name: a, server: 1.1.1.1, port: 443, type: trojan, password: p}\n";
    let rejected = mgr.import_file(clash, Some("clash")).unwrap_err();
    assert!(matches!(rejected, SubscriptionError::InvalidSingBox(_)));
    assert_eq!(load_index(&paths).unwrap().items.len(), 1);

    let update = mgr.fetch_update(meta.id).unwrap_err();
    assert!(matches!(update, SubscriptionError::FetchFailed(_)));
    let fetcher = MockFetcher {
        bypasses_proxy: true,
        mode: MockFetchMode::Fail("should not fetch a file subscription".into()),
    };
    let refreshing = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
    assert!(refreshing.fetch_auto().is_empty());
    let auto = refreshing
        .set_auto_update(meta.id, true, Some(AutoUpdateInterval::OneHour))
        .unwrap_err();
    assert!(matches!(auto, SubscriptionError::FetchFailed(_)));

    let share = subscription_share_text(&paths, meta.id, SubscriptionShareKind::Url).unwrap_err();
    assert!(matches!(share, SubscriptionError::FetchFailed(_)));
    let json = subscription_share_text(&paths, meta.id, SubscriptionShareKind::Singbox).unwrap();
    assert!(json.contains("n1"));

    let legacy = serde_json::json!({
        "id": "11111111-1111-1111-1111-111111111111",
        "name": "old",
        "url": "https://example.com/s",
        "active": false,
        "format": "sing_box",
        "node_count": 0,
        "last_updated": null,
        "last_error": null,
        "etag": null,
        "last_modified": null,
    });
    let old: SubscriptionMeta = serde_json::from_value(legacy).unwrap();
    assert_eq!(old.source, SubscriptionSource::Remote);
    let _ = fs::remove_dir_all(paths.root());
}

// --- Provider routing that depends on remote rule sets ---------------------

/// Shape of a panel's sing-box answer: split routing through remote rule
/// sets, legacy-format DNS servers.
const PANEL_SINGBOX_BODY: &str = r#"{
    "outbounds": [
        {"type":"selector","tag":"select","outbounds":["auto","n1","n2"]},
        {"type":"urltest","tag":"auto","outbounds":["n1","n2"]},
        {"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1080},
        {"type":"socks","tag":"n2","server":"1.1.1.2","server_port":1080},
        {"type":"direct","tag":"direct"}
    ],
    "route": {
        "rules": [
            {"action":"sniff"},
            {"rule_set":["geosite-cn","geoip-cn"],"outbound":"direct"},
            {"ip_is_private":true,"outbound":"direct"}
        ],
        "rule_set": [
            {"type":"remote","tag":"geosite-cn","format":"binary","url":"http://example.com/geosite-cn.srs"},
            {"type":"remote","tag":"geoip-cn","format":"binary","url":"http://example.com/geoip-cn.srs"}
        ]
    },
    "dns": {
        "servers": [
            {"tag":"remote","address":"1.1.1.1","detour":"select"},
            {"tag":"local","address":"https://223.5.5.5/dns-query","detour":"direct"}
        ],
        "rules": [{"rule_set":["geosite-cn"],"server":"local"}],
        "final": "remote"
    }
}"#;

#[test]
fn singbox_profile_keeps_provider_rules_that_do_not_need_remote_rule_sets() {
    let profile = parse_singbox_profile(PANEL_SINGBOX_BODY).expect("parse");
    assert_eq!(profile.nodes.len(), 2);
    assert_eq!(profile.groups.len(), 2);
    // Only the rule that needs the remote sets is dropped, with a warning.
    assert_eq!(
        profile.route.rules,
        vec![
            serde_json::json!({"action":"sniff"}),
            serde_json::json!({"ip_is_private":true,"outbound":"direct"}),
        ]
    );
    assert!(profile
        .parse_stats
        .warnings
        .iter()
        .any(|w| w.key == "parse.routeRuleSetMissing"));
    // Legacy-format servers are filtered out, so no DNS block is left.
    assert!(profile.dns.is_none());
}

#[test]
fn singbox_profile_drops_a_dns_block_without_servers() {
    let raw = r#"{
        "outbounds": [
            {"type":"socks","tag":"n","server":"1.1.1.1","server_port":1080}
        ],
        "dns": {"rules": [{"domain_suffix":["example.cn"],"server":"x"}], "final": "x"}
    }"#;
    let profile = parse_singbox_profile(raw).expect("parse");
    assert!(profile.dns.is_none());
}

#[test]
fn singbox_profile_keeps_provider_route_final_and_dns_next_to_a_dropped_rule() {
    let raw = r#"{
        "outbounds": [
            {"type":"selector","tag":"select","outbounds":["n"]},
            {"type":"selector","tag":"media","outbounds":["n"]},
            {"type":"socks","tag":"n","server":"1.1.1.1","server_port":1080}
        ],
        "route": {
            "final": "media",
            "rules": [
                {"rule_set":["geosite-ads"],"action":"reject"},
                {"domain_suffix":["example.cn"],"outbound":"direct"}
            ],
            "rule_set": [
                {"type":"remote","tag":"geosite-ads","url":"http://example.com/ads.srs"}
            ]
        },
        "dns": {
            "servers": [{"type":"udp","tag":"cn","server":"223.5.5.5"}],
            "final": "cn"
        }
    }"#;
    let profile = parse_singbox_profile(raw).expect("parse");
    assert_eq!(profile.route.final_outbound, "media");
    assert_eq!(profile.route.rules.len(), 1);
    assert!(profile.dns.is_some());
}

#[test]
fn singbox_profile_keeps_complete_provider_rules_but_drops_serverless_dns() {
    let raw = r#"{
        "outbounds": [
            {"type":"socks","tag":"n","server":"1.1.1.1","server_port":1080}
        ],
        "route": {
            "final": "n",
            "rules": [{"domain_suffix":["example.cn"],"outbound":"direct"}]
        },
        "dns": {
            "servers": [{"tag":"remote","address":"1.1.1.1"}],
            "final": "remote"
        }
    }"#;
    let profile = parse_singbox_profile(raw).expect("parse");
    assert_eq!(profile.route.rules.len(), 1, "complete rules stay");
    assert!(
        profile.dns.is_none(),
        "a DNS block whose servers were all filtered out is dropped"
    );
}

// --- sing-box-first fetch with generic fallback -----------------------------

const URI_LIST_BODY: &str = "trojan://secret@1.2.3.4:443?sni=example.com#n1\n";

/// A sing-box answer whose routing needs no remote rule set.
const COMPLETE_SINGBOX_BODY: &str = r#"{
    "outbounds": [
        {"type":"selector","tag":"select","outbounds":["n1","n2"]},
        {"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1080},
        {"type":"socks","tag":"n2","server":"1.1.1.2","server_port":1080}
    ],
    "route": {
        "final": "select",
        "rules": [{"domain_suffix":["example.cn"],"outbound":"direct"}]
    }
}"#;

#[derive(Clone)]
enum Reply {
    Body(
        &'static str,
        Option<SubscriptionUserInfo>,
        Option<&'static str>,
    ),
    Status(u16),
    NotModified,
    TooLarge,
    Transport,
}

/// One recorded request: the client and the `If-None-Match` it sent.
type Call = (FetchClient, Option<String>);

/// Answers each client with its own scripted reply and records every request.
#[derive(Clone)]
struct ClientAwareFetcher {
    singbox: Reply,
    generic: Reply,
    calls: Arc<std::sync::Mutex<Vec<Call>>>,
}

impl ClientAwareFetcher {
    fn new(singbox: Reply, generic: Reply) -> Self {
        Self {
            singbox,
            generic,
            calls: Arc::default(),
        }
    }

    fn take_calls(&self) -> Vec<Call> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }
}

impl HttpFetcher for ClientAwareFetcher {
    fn bypasses_system_proxy(&self) -> bool {
        false
    }

    fn get_as(
        &self,
        url: &str,
        etag: Option<&str>,
        _last_modified: Option<&str>,
        client: FetchClient,
    ) -> Result<FetchResponse, SubscriptionError> {
        self.calls
            .lock()
            .unwrap()
            .push((client, etag.map(str::to_string)));
        let reply = match client {
            FetchClient::SingBox => &self.singbox,
            FetchClient::Generic => &self.generic,
        };
        match reply {
            Reply::Body(body, userinfo, new_etag) => Ok(FetchResponse {
                body: (*body).into(),
                not_modified: false,
                etag: new_etag.map(str::to_string),
                last_modified: None,
                userinfo: *userinfo,
                content_disposition: None,
            }),
            Reply::Status(status) => Err(SubscriptionError::HttpStatus {
                url: url.into(),
                status: *status,
            }),
            Reply::NotModified => Ok(FetchResponse {
                body: String::new(),
                not_modified: true,
                etag: None,
                last_modified: None,
                userinfo: None,
                content_disposition: None,
            }),
            Reply::TooLarge => Err(SubscriptionError::BodyRejected("body exceeds".into())),
            Reply::Transport => Err(SubscriptionError::FetchFailed("connect: timed out".into())),
        }
    }
}

fn quota() -> Option<SubscriptionUserInfo> {
    Some(SubscriptionUserInfo {
        upload: 1,
        download: 2,
        total: 100,
        expire: None,
    })
}

const SUB_URL: &str = "https://example.com/sub";

#[test]
fn fetch_uses_the_singbox_answer_when_it_is_usable() {
    let paths = temp_subs("ua-singbox");
    let fetcher = ClientAwareFetcher::new(
        Reply::Body(COMPLETE_SINGBOX_BODY, quota(), None),
        Reply::Body(URI_LIST_BODY, None, None),
    );
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher.clone());
    let meta = mgr.add(SUB_URL, None, false, None).unwrap();
    assert_eq!(meta.format, SubscriptionFormat::SingBox);
    assert_eq!(meta.fetched_as, Some(FetchClient::SingBox));
    assert_eq!(meta.group_count, 1);
    assert_eq!(meta.userinfo, quota());
    assert_eq!(fetcher.take_calls(), vec![(FetchClient::SingBox, None)]);
}

#[test]
fn fetch_falls_back_when_the_singbox_answer_is_unusable() {
    let paths = temp_subs("ua-html");
    let fetcher = ClientAwareFetcher::new(
        Reply::Body("<!doctype html><html>login</html>", quota(), None),
        Reply::Body(URI_LIST_BODY, None, None),
    );
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher.clone());
    let meta = mgr.add(SUB_URL, None, false, None).unwrap();
    assert_eq!(meta.format, SubscriptionFormat::UriList);
    assert_eq!(
        meta.userinfo,
        quota(),
        "counters sent only to the sing-box client are kept"
    );
    let clients: Vec<FetchClient> = fetcher.take_calls().into_iter().map(|c| c.0).collect();
    assert_eq!(clients, vec![FetchClient::SingBox, FetchClient::Generic]);
}

#[test]
fn fetch_falls_back_when_the_singbox_request_is_refused() {
    let paths = temp_subs("ua-403");
    let fetcher =
        ClientAwareFetcher::new(Reply::Status(403), Reply::Body(URI_LIST_BODY, None, None));
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher.clone());
    let meta = mgr.add(SUB_URL, None, false, None).unwrap();
    assert_eq!(meta.format, SubscriptionFormat::UriList);
    assert_eq!(fetcher.take_calls().len(), 2);
}

#[test]
fn fetch_does_not_retry_a_transport_failure() {
    let paths = temp_subs("ua-transport");
    let fetcher = ClientAwareFetcher::new(Reply::Transport, Reply::Body(URI_LIST_BODY, None, None));
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher.clone());
    let err = mgr
        .add(SUB_URL, None, false, None)
        .expect_err("transport failure");
    assert!(matches!(err, SubscriptionError::FetchFailed(_)), "{err:?}");
    assert_eq!(fetcher.take_calls(), vec![(FetchClient::SingBox, None)]);
}

#[test]
fn fetch_reports_the_generic_error_when_both_clients_fail() {
    let paths = temp_subs("ua-both");
    let fetcher = ClientAwareFetcher::new(Reply::Status(403), Reply::Status(404));
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
    let err = mgr
        .add(SUB_URL, None, false, None)
        .expect_err("both refused");
    assert!(
        matches!(err, SubscriptionError::HttpStatus { status: 404, .. }),
        "{err:?}"
    );
}

#[test]
fn fetch_sends_validators_only_to_the_client_whose_body_is_cached() {
    let paths = temp_subs("ua-validators");
    // Cached as a share-link list from the generic client.
    let generic_only = ClientAwareFetcher::new(
        Reply::Status(403),
        Reply::Body(URI_LIST_BODY, None, Some("generic-v1")),
    );
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), generic_only.clone());
    let meta = mgr.add(SUB_URL, None, false, None).unwrap();
    assert_eq!(meta.etag.as_deref(), Some("generic-v1"));
    generic_only.take_calls();
    mgr.update(meta.id).unwrap();
    assert_eq!(
        generic_only.take_calls(),
        vec![
            (FetchClient::SingBox, None),
            (FetchClient::Generic, Some("generic-v1".into())),
        ]
    );

    // The provider starts answering the sing-box client: the generic
    // validator is not sent to it, so the cached list cannot be pinned by a
    // 304, and the subscription switches to the sing-box config.
    let both = ClientAwareFetcher::new(
        Reply::Body(COMPLETE_SINGBOX_BODY, None, Some("singbox-v1")),
        Reply::Body(URI_LIST_BODY, None, Some("generic-v2")),
    );
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), both.clone());
    let updated = mgr.update(meta.id).unwrap();
    assert_eq!(updated.format, SubscriptionFormat::SingBox);
    assert_eq!(both.take_calls(), vec![(FetchClient::SingBox, None)]);

    // Cached as sing-box now: its validator goes to the sing-box client only.
    mgr.update(meta.id).unwrap();
    assert_eq!(
        both.take_calls(),
        vec![(FetchClient::SingBox, Some("singbox-v1".into()))]
    );
}

#[test]
fn singbox_client_user_agent_names_the_bundled_core() {
    assert_eq!(
        FetchClient::SingBox.user_agent(),
        format!("sing-box/{}", ice_config::ENGINE_COMPAT_CORE_VERSION)
    );
    assert_eq!(FetchClient::Generic.user_agent(), "ice-box/0.1");
}

#[test]
fn fetch_prefers_the_generic_answer_when_the_singbox_config_loses_rules() {
    let paths = temp_subs("ua-lossy");
    let fetcher = ClientAwareFetcher::new(
        Reply::Body(PANEL_SINGBOX_BODY, quota(), None),
        Reply::Body(URI_LIST_BODY, None, None),
    );
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher.clone());
    let meta = mgr.add(SUB_URL, None, false, None).unwrap();
    assert_eq!(meta.format, SubscriptionFormat::UriList);
    assert_eq!(meta.fetched_as, Some(FetchClient::Generic));
    assert_eq!(meta.userinfo, quota());
    let clients: Vec<FetchClient> = fetcher.take_calls().into_iter().map(|c| c.0).collect();
    assert_eq!(clients, vec![FetchClient::SingBox, FetchClient::Generic]);
}

#[test]
fn fetch_keeps_a_lossy_singbox_config_when_the_generic_answer_is_unusable() {
    let paths = temp_subs("ua-lossy-keep");
    for generic in [
        Reply::Status(403),
        Reply::Body("<html>nope</html>", None, None),
    ] {
        let fetcher = ClientAwareFetcher::new(Reply::Body(PANEL_SINGBOX_BODY, None, None), generic);
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
        let meta = mgr.add(SUB_URL, None, false, None).unwrap();
        assert_eq!(meta.format, SubscriptionFormat::SingBox);
        assert_eq!(meta.fetched_as, Some(FetchClient::SingBox));
    }
}

#[test]
fn fetch_does_not_retry_server_errors_or_rate_limits() {
    for status in [500, 502, 503, 408, 429] {
        let paths = temp_subs("ua-5xx");
        let fetcher = ClientAwareFetcher::new(
            Reply::Status(status),
            Reply::Body(URI_LIST_BODY, None, None),
        );
        let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher.clone());
        let err = mgr.add(SUB_URL, None, false, None).expect_err("no retry");
        assert!(
            matches!(err, SubscriptionError::HttpStatus { status: s, .. } if s == status),
            "{err:?}"
        );
        assert_eq!(fetcher.take_calls().len(), 1, "HTTP {status}");
    }
}

#[test]
fn fetch_retries_a_rejected_singbox_body_as_the_generic_client() {
    let paths = temp_subs("ua-too-large");
    let fetcher = ClientAwareFetcher::new(Reply::TooLarge, Reply::Body(URI_LIST_BODY, None, None));
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher.clone());
    let meta = mgr.add(SUB_URL, None, false, None).unwrap();
    assert_eq!(meta.format, SubscriptionFormat::UriList);
    assert_eq!(fetcher.take_calls().len(), 2);
}

#[test]
fn fetch_keeps_conditional_requests_for_providers_that_ignore_the_user_agent() {
    let paths = temp_subs("ua-ignored");
    // A subscription saved before the sing-box client existed: its validator
    // was issued for the generic client.
    let legacy = ClientAwareFetcher::new(
        Reply::Status(403),
        Reply::Body(URI_LIST_BODY, None, Some("v1")),
    );
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), legacy);
    let meta = mgr.add(SUB_URL, None, false, None).unwrap();
    assert_eq!(meta.fetched_as, Some(FetchClient::Generic));

    // The provider sends the same share links to every client.
    let same = ClientAwareFetcher::new(
        Reply::Body(URI_LIST_BODY, None, Some("v2")),
        Reply::Body(URI_LIST_BODY, None, Some("v2")),
    );
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), same.clone());
    let updated = mgr.update(meta.id).unwrap();
    assert_eq!(updated.fetched_as, Some(FetchClient::SingBox));
    assert_eq!(updated.etag.as_deref(), Some("v2"));
    assert_eq!(same.take_calls(), vec![(FetchClient::SingBox, None)]);

    // From now on the validator goes with the sing-box request, so a 304
    // ends the update without a second request or a full download.
    let unchanged =
        ClientAwareFetcher::new(Reply::NotModified, Reply::Body(URI_LIST_BODY, None, None));
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), unchanged.clone());
    mgr.update(meta.id).unwrap();
    assert_eq!(
        unchanged.take_calls(),
        vec![(FetchClient::SingBox, Some("v2".into()))]
    );
}

#[test]
fn switching_clients_does_not_carry_the_old_validator_over() {
    let paths = temp_subs("ua-switch");
    let generic = ClientAwareFetcher::new(
        Reply::Status(403),
        Reply::Body(URI_LIST_BODY, None, Some("generic-v1")),
    );
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), generic);
    let meta = mgr.add(SUB_URL, None, false, None).unwrap();

    // The sing-box answer carries no validator of its own.
    let singbox = ClientAwareFetcher::new(
        Reply::Body(COMPLETE_SINGBOX_BODY, None, None),
        Reply::Body(URI_LIST_BODY, None, None),
    );
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), singbox);
    let updated = mgr.update(meta.id).unwrap();
    assert_eq!(updated.fetched_as, Some(FetchClient::SingBox));
    assert_eq!(updated.etag, None);
}

#[test]
fn builtin_rules_are_appended_when_provider_rules_were_lost_to_rule_sets() {
    let mut profile = parse_singbox_profile(PANEL_SINGBOX_BODY).expect("parse");
    let provider_rules = profile.route.rules.clone();
    super::apply_builtin_default_rules(&mut profile, HostPlatform::Windows);
    assert_eq!(
        &profile.route.rules[..provider_rules.len()],
        provider_rules.as_slice(),
        "the provider's own rules keep their priority"
    );
    assert!(
        profile.route.rules.len() > provider_rules.len(),
        "domestic-direct defaults fill the gap"
    );
    assert!(profile.route.rules.iter().any(|r| r.get("geoip").is_some()));
    assert!(
        profile.dns.is_some(),
        "built-in DNS replaces the filtered one"
    );
}

#[test]
fn builtin_rules_leave_a_complete_provider_route_alone() {
    let mut profile = parse_singbox_profile(COMPLETE_SINGBOX_BODY).expect("parse");
    super::apply_builtin_default_rules(&mut profile, HostPlatform::Windows);
    assert_eq!(profile.route.rules.len(), 1);
    assert_eq!(profile.route.final_outbound, "select");
}

#[test]
fn fetch_keeps_a_lossy_singbox_config_over_a_generic_body_that_does_not_parse() {
    let paths = temp_subs("ua-lossy-bad-generic");
    // Detected as Clash, but no proxy survives parsing.
    let fetcher = ClientAwareFetcher::new(
        Reply::Body(PANEL_SINGBOX_BODY, None, None),
        Reply::Body(
            "proxies:\n  - {name: x, type: not-a-protocol}\n",
            None,
            None,
        ),
    );
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher);
    let meta = mgr.add(SUB_URL, None, false, None).unwrap();
    assert_eq!(meta.format, SubscriptionFormat::SingBox);
}

#[test]
fn a_kept_lossy_singbox_config_is_not_pinned_by_a_304() {
    let paths = temp_subs("ua-lossy-304");
    let first = ClientAwareFetcher::new(
        Reply::Body(PANEL_SINGBOX_BODY, None, Some("sb-v1")),
        Reply::Status(403),
    );
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), first);
    let meta = mgr.add(SUB_URL, None, false, None).unwrap();
    assert_eq!(meta.fetched_as, Some(FetchClient::SingBox));
    assert_eq!(meta.etag.as_deref(), Some("sb-v1"));

    // The generic client works again: the sing-box request carries no
    // validator, so the lossy config is judged again and replaced.
    let later = ClientAwareFetcher::new(
        Reply::Body(PANEL_SINGBOX_BODY, None, Some("sb-v1")),
        Reply::Body(URI_LIST_BODY, None, None),
    );
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), later.clone());
    let updated = mgr.update(meta.id).unwrap();
    assert_eq!(updated.format, SubscriptionFormat::UriList);
    assert_eq!(
        later.take_calls(),
        vec![(FetchClient::SingBox, None), (FetchClient::Generic, None)]
    );
}

// --- Panel sing-box config whose remote rule sets have bundled copies -------

/// Shape of a real panel's sing-box answer (nodes replaced).
fn panel_fixture() -> String {
    fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../configs/examples/subscription-singbox-panel.json"),
    )
    .expect("fixture")
}

#[test]
fn panel_singbox_config_keeps_its_routing_through_bundled_rule_sets() {
    let profile = parse_singbox_profile(&panel_fixture()).expect("parse");
    assert!(
        !super::lost_rule_set_rules(&profile.parse_stats.warnings),
        "every remote set is downloadable: {:?}",
        profile.parse_stats.warnings
    );
    assert_eq!(profile.route.rules.len(), 7, "provider rules are kept");
    let names: Vec<(&str, &str)> = profile
        .route
        .rule_sets
        .iter()
        .map(|s| (s["tag"].as_str().unwrap(), s["bundled"].as_str().unwrap()))
        .collect();
    assert_eq!(
        names,
        [
            ("geosite-cn", "geosite-cn"),
            ("geosite-category-ads-all", "geosite-category-ads-all"),
            ("geoip-cn", "geoip-cn"),
        ]
    );
    assert!(profile
        .route
        .rule_sets
        .iter()
        .all(|s| s["type"] == "download"
            && s["url"].as_str().is_some_and(|u| u.starts_with("https://"))));
    // No `final`: sing-box uses the first outbound, the panel's main selector.
    assert_eq!(profile.route.final_outbound, "节点选择");
    // Legacy-format DNS servers are filtered out; the built-in DNS fills in.
    assert!(profile.dns.is_none());
    let mut profile = profile;
    super::apply_builtin_default_rules(&mut profile, HostPlatform::Windows);
    assert_eq!(profile.route.rules.len(), 7, "no built-in rules appended");
    let dns = profile.dns.expect("built-in DNS");
    assert!(dns["servers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["tag"] == "remote-dns" && s["detour"] == "节点选择"));
}

#[test]
fn panel_singbox_answer_is_used_over_the_generic_one() {
    let paths = temp_subs("ua-panel");
    let body: &'static str = Box::leak(panel_fixture().into_boxed_str());
    let fetcher = ClientAwareFetcher::new(
        Reply::Body(body, quota(), None),
        Reply::Body(URI_LIST_BODY, None, None),
    );
    let mgr = SubscriptionManager::with_fetcher(clone_paths(&paths), fetcher.clone());
    let meta = mgr.add(SUB_URL, None, false, None).unwrap();
    assert_eq!(meta.format, SubscriptionFormat::SingBox);
    assert_eq!(meta.group_count, 2);
    assert_eq!(meta.rule_count, 7);
    assert!(meta.parse_warnings.is_empty(), "{:?}", meta.parse_warnings);
    assert_eq!(fetcher.take_calls(), vec![(FetchClient::SingBox, None)]);
}

#[test]
fn builtin_dns_detours_through_a_group_when_final_is_direct() {
    let raw = r#"{
        "outbounds": [
            {"type":"selector","tag":"select","outbounds":["n"]},
            {"type":"socks","tag":"n","server":"1.1.1.1","server_port":1080}
        ],
        "route": {"final": "direct", "rules": [{"domain_suffix":["x.com"],"outbound":"select"}]}
    }"#;
    let mut profile = parse_singbox_profile(raw).expect("parse");
    super::apply_builtin_default_rules(&mut profile, HostPlatform::MacOs);
    let dns = profile.dns.expect("built-in DNS");
    assert!(dns["servers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["tag"] == "remote-dns" && s["detour"] == "select"));
    assert_eq!(profile.route.final_outbound, "direct", "final is kept");
}
