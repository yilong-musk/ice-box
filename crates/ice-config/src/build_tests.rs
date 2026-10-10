// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::{
    build_input_from_nodes, rule_fingerprint, GroupSelections, NormalizedOutbound,
    NormalizedProfile, NormalizedRoute, ProfileParseStats, ProxyMode, RuleOverrides, TunSettings,
};
use serde_json::{json, Value};
use std::fs;
use std::sync::Arc;

fn socks(tag: &str) -> NormalizedOutbound {
    NormalizedOutbound {
        tag: tag.into(),
        outbound: std::sync::Arc::new(json!({
            "type": "socks",
            "tag": tag,
            "server": "127.0.0.1",
            "server_port": 1080
        })),
    }
}

/// Extract the two prepended `clash_mode` rules as (mode, outbound) pairs, in order.
fn clash_rules(cfg: &Value) -> Vec<(&str, &str)> {
    cfg["route"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| {
            let mode = r.get("clash_mode").and_then(|v| v.as_str())?;
            let outbound = r.get("outbound").and_then(|v| v.as_str())?;
            Some((mode, outbound))
        })
        .collect()
}

#[test]
fn tun_network_cidr_rejects_out_of_range_prefix() {
    assert_eq!(tun_network_cidr("10.0.0.1/0").as_deref(), Some("0.0.0.0/0"));
    assert_eq!(
        tun_network_cidr("10.0.0.1/32").as_deref(),
        Some("10.0.0.1/32")
    );
    assert_eq!(tun_network_cidr("10.0.0.1/33"), None);
    assert_eq!(
        tun_network_cidr("fdfe:dcba:9876::1/128").as_deref(),
        Some("fdfe:dcba:9876::1/128")
    );
    assert_eq!(tun_network_cidr("fdfe:dcba:9876::1/129"), None);
    assert_eq!(
        tun_network_cidr("10.0.0.1/30").as_deref(),
        Some("10.0.0.0/30")
    );
}

#[test]
fn direct_only_config_has_builtin_outbounds_and_direct_final() {
    let cfg = build_direct_only_config(
        &LocalTemplate::default(),
        CaptureIntent::Diagnostic,
        HostPlatform::MacOs,
    )
    .expect("build");
    let outbounds = cfg["outbounds"].as_array().unwrap();
    let tags: Vec<&str> = outbounds.iter().filter_map(|o| o["tag"].as_str()).collect();
    assert_eq!(tags, ["direct", "block"]);
    assert_eq!(cfg["route"]["final"], "direct");
    assert_eq!(cfg["inbounds"][0]["type"], "mixed");
    assert_eq!(cfg["inbounds"][0]["listen_port"], 17890);
    assert_eq!(
        clash_rules(&cfg),
        [("global", "direct"), ("direct", "direct")],
        "mode switching stays wired; every mode routes direct"
    );
    assert_eq!(cfg["experimental"]["clash_api"]["default_mode"], "Rule");
    assert_eq!(cfg["log"]["level"], "info");
    validate_config(&cfg).expect("generated config must pass CFG-1");
}

#[test]
fn validate_config_rejects_empty_arrays_duplicate_tags_and_bad_refs() {
    let empty = json!({"inbounds": [], "outbounds": [{"type":"direct","tag":"direct"}]});
    let err = validate_config(&empty).unwrap_err();
    assert!(err.to_string().contains("/inbounds"), "{err}");

    let dup = json!({
        "inbounds": [{"type":"mixed","tag":"in","listen":"127.0.0.1","listen_port":17890}],
        "outbounds": [
            {"type":"direct","tag":"x"},
            {"type":"block","tag":"x"}
        ]
    });
    let err = validate_config(&dup).unwrap_err();
    assert!(err.to_string().contains("/outbounds/1/tag"), "{err}");

    let dup_in = json!({
        "inbounds": [
            {"type":"mixed","tag":"in","listen":"127.0.0.1","listen_port":17890},
            {"type":"tun","tag":"in"}
        ],
        "outbounds": [{"type":"direct","tag":"direct"}]
    });
    let err = validate_config(&dup_in).unwrap_err();
    assert!(err.to_string().contains("/inbounds/1/tag"), "{err}");

    let bad_rule = json!({
        "inbounds": [{"type":"mixed","tag":"in","listen":"127.0.0.1","listen_port":17890}],
        "outbounds": [{"type":"direct","tag":"direct"}],
        "route": {"rules": [{"domain_suffix": ["x.com"], "outbound": "ghost"}]}
    });
    let err = validate_config(&bad_rule).unwrap_err();
    assert!(err.to_string().contains("/route/rules/0/outbound"), "{err}");

    let bad_final = json!({
        "inbounds": [{"type":"mixed","tag":"in","listen":"127.0.0.1","listen_port":17890}],
        "outbounds": [{"type":"direct","tag":"direct"}],
        "route": {"final": "missing"}
    });
    let err = validate_config(&bad_final).unwrap_err();
    assert!(err.to_string().contains("/route/final"), "{err}");

    let bad_member = json!({
        "inbounds": [{"type":"mixed","tag":"in","listen":"127.0.0.1","listen_port":17890}],
        "outbounds": [
            {"type":"direct","tag":"direct"},
            {"type":"selector","tag":"proxy","outbounds":["nope"]}
        ]
    });
    let err = validate_config(&bad_member).unwrap_err();
    assert!(
        err.to_string().contains("/outbounds/1/outbounds/0"),
        "{err}"
    );

    let bad_clash = json!({
        "inbounds": [{"type":"mixed","tag":"in","listen":"127.0.0.1","listen_port":17890}],
        "outbounds": [{"type":"direct","tag":"direct"}],
        "experimental": {"clash_api": {"external_controller": "0.0.0.0:19090"}}
    });
    let err = validate_config(&bad_clash).unwrap_err();
    assert!(
        err.to_string()
            .contains("/experimental/clash_api/external_controller"),
        "{err}"
    );
}

#[test]
fn direct_only_config_validates_template() {
    let invalid = LocalTemplate {
        mixed_port: 80,
        ..LocalTemplate::default()
    };
    assert!(
        build_direct_only_config(&invalid, CaptureIntent::Diagnostic, HostPlatform::MacOs).is_err()
    );
}

#[test]
fn g5_8_selected_tag_missing_falls_back_to_first() {
    let cfg = build_runtime_json(&build_input_from_nodes(
        LocalTemplate::default(),
        vec![socks("a"), socks("b")],
        Some("gone".into()),
    ))
    .expect("build");
    let selector = cfg["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["tag"] == "proxy")
        .unwrap();
    assert_eq!(selector["default"], "a");
}

#[test]
fn group_selection_overrides_selector_default_in_built_config() {
    let profile = NormalizedProfile {
        nodes: vec![socks("a"), socks("b")],
        groups: vec![NormalizedOutbound {
            tag: "Proxies".into(),
            outbound: std::sync::Arc::new(json!({
                "type": "selector",
                "tag": "Proxies",
                "outbounds": ["a", "b"],
                "default": "a",
            })),
        }],
        route: Default::default(),
        dns: None,
        default_outbound: Some("Proxies".into()),
        parse_stats: ProfileParseStats::default(),
    };
    let mut selections = GroupSelections::new();
    selections.insert("Proxies".into(), "b".into());
    let cfg = build_runtime_json(&BuildInput {
        template: LocalTemplate::default(),
        profile: Arc::new(profile),
        selected_tag: Some("Proxies".into()),
        geoip_rule_set_dir: None,
        group_selections: selections,
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Diagnostic,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .expect("build");
    let group = cfg["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["tag"] == "Proxies")
        .unwrap();
    assert_eq!(group["default"], "b");
}

#[test]
fn g5_9_port_validation() {
    let t = LocalTemplate {
        mixed_port: 80,
        ..LocalTemplate::default()
    };
    assert!(matches!(
        validate_template(&t),
        Err(ConfigError::Invalid(_))
    ));

    let t = LocalTemplate {
        mixed_port: 19090,
        clash_api_port: 19090,
        ..LocalTemplate::default()
    };
    assert!(matches!(
        validate_template(&t),
        Err(ConfigError::Invalid(_))
    ));

    let err = build_runtime_config(&build_input_from_nodes(
        LocalTemplate {
            mixed_port: 80,
            ..LocalTemplate::default()
        },
        vec![socks("a")],
        None,
    ))
    .expect_err("low port");
    assert!(matches!(err, ConfigError::Invalid(_)));

    let err = build_runtime_config(&build_input_from_nodes(
        LocalTemplate {
            mixed_listen: "192.168.1.1".into(),
            ..LocalTemplate::default()
        },
        vec![socks("a")],
        None,
    ))
    .expect_err("non-loopback mixed");
    assert!(matches!(err, ConfigError::Invalid(_)));

    let err = build_runtime_config(&build_input_from_nodes(
        LocalTemplate {
            clash_api_listen: "0.0.0.0".into(),
            ..LocalTemplate::default()
        },
        vec![socks("a")],
        None,
    ))
    .expect_err("non-loopback clash api");
    assert!(matches!(err, ConfigError::Invalid(_)));

    let err = build_runtime_config(&build_input_from_nodes(
        LocalTemplate {
            clash_api_secret: String::new(),
            ..LocalTemplate::default()
        },
        vec![socks("a")],
        None,
    ))
    .expect_err("empty clash api secret");
    assert!(matches!(err, ConfigError::Invalid(_)));
}

#[test]
fn g5_10_empty_outbounds() {
    let err = build_runtime_config(&BuildInput {
        template: LocalTemplate::default(),
        profile: Arc::new(NormalizedProfile::from_nodes_only(vec![])),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Diagnostic,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .expect_err("empty");
    assert!(matches!(err, ConfigError::EmptyOutbounds));
}

#[test]
fn allow_lan_binds_0_0_0_0_and_no_dns_inbound() {
    let cfg = build_runtime_json(&build_input_from_nodes(
        LocalTemplate {
            allow_lan: true,
            ..LocalTemplate::default()
        },
        vec![socks("a")],
        None,
    ))
    .unwrap();

    assert_eq!(cfg["inbounds"][0]["listen"], "0.0.0.0");
    assert_eq!(cfg["inbounds"].as_array().unwrap().len(), 1);
    assert_eq!(cfg["route"]["default_domain_resolver"], "local");
    assert_guard_unchanged(cfg);
}

#[test]
fn dns_listen_key_dropped_without_dns_inbound() {
    let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
    let mut dns = json!({
        "servers": [{ "type": "local", "tag": "local" }],
        "final": "local",
    });
    dns.as_object_mut().unwrap().insert(
        "__ice_dns_listen".into(),
        json!({ "listen": "127.0.0.1", "listen_port": 7874 }),
    );
    profile.dns = Some(dns);

    let cfg = build_runtime_json(&BuildInput {
        template: LocalTemplate::default(),
        profile: Arc::new(profile),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Diagnostic,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .unwrap();

    assert_eq!(cfg["inbounds"].as_array().unwrap().len(), 1);
    assert_eq!(cfg["inbounds"][0]["listen"], "127.0.0.1");
    assert!(
        cfg["dns"].get("__ice_dns_listen").is_none(),
        "internal dns listen key must not leak into runtime config"
    );
    assert_eq!(cfg["route"]["default_domain_resolver"], "local");
}

fn profile_with_geoip(codes: &[&str]) -> NormalizedProfile {
    let nodes = vec![socks("a")];
    let mut profile = NormalizedProfile::from_nodes_only(nodes);
    profile.route.rules = codes
        .iter()
        .map(|c| {
            json!({
                "geoip": [c],
                "outbound": "direct",
            })
        })
        .collect();
    profile
}

#[test]
fn geoip_rules_expand_to_local_rule_sets_when_files_present() {
    let dir = std::env::temp_dir().join(format!(
        "ice-box-geoip-ok-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("geoip-cn.srs"), b"srs").unwrap();

    let cfg = build_runtime_json(&BuildInput {
        template: LocalTemplate::default(),
        profile: Arc::new(profile_with_geoip(&["cn"])),
        selected_tag: None,
        geoip_rule_set_dir: Some(dir.clone()),
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Diagnostic,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .unwrap();

    let geoip_rule = cfg["route"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r.get("rule_set").is_some())
        .expect("geoip rule survives behind the clash_mode rules");
    assert_eq!(geoip_rule["rule_set"][0], "geoip-cn");
    assert!(
        geoip_rule.get("geoip").is_none(),
        "geoip option must be removed"
    );
    let set = &cfg["route"]["rule_set"][0];
    assert_eq!(set["type"], "local");
    assert_eq!(set["format"], "binary");
    assert_eq!(
        set["path"],
        serde_json::Value::String(dir.join("geoip-cn.srs").to_string_lossy().into())
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn geoip_rules_dropped_when_rule_set_file_missing() {
    let dir = std::env::temp_dir().join(format!(
        "ice-box-geoip-missing-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();

    let cfg = build_runtime_json(&BuildInput {
        template: LocalTemplate::default(),
        profile: Arc::new(profile_with_geoip(&["kz"])),
        selected_tag: None,
        geoip_rule_set_dir: Some(dir.clone()),
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Diagnostic,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .unwrap();

    let rules = cfg["route"]["rules"].as_array().unwrap();
    assert_eq!(
        rules.len(),
        2,
        "only the two clash_mode rules survive; the unresolvable geoip rule is dropped"
    );
    assert!(
        rules.iter().all(|r| r.get("rule_set").is_none()),
        "no rule_set references for dropped codes"
    );
    assert!(
        cfg["route"].get("rule_set").is_none(),
        "no rule_set entries for dropped codes"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn custom_geoip_expanded_and_geosite_dropped_at_build() {
    let dir = std::env::temp_dir().join(format!(
        "ice-box-custom-geo-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("geoip-cn.srs"), b"srs").unwrap();

    let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
    profile.route.rules = vec![
        json!({ "domain_suffix": ["keep.com"], "outbound": "direct" }),
        json!({ "geosite": ["google"], "outbound": "direct" }),
    ];
    let mut overrides = RuleOverrides::default();
    // A custom geoip rule persisted before add-time validation existed.
    overrides
        .custom
        .push(json!({ "geoip": ["cn"], "outbound": "direct" }));
    // A custom geosite rule is dropped too.
    overrides
        .custom
        .push(json!({ "geosite": ["netflix"], "outbound": "direct" }));

    let cfg = build_runtime_json(&BuildInput {
        template: LocalTemplate::default(),
        profile: Arc::new(profile),
        selected_tag: None,
        geoip_rule_set_dir: Some(dir.clone()),
        group_selections: GroupSelections::new(),
        rule_overrides: overrides,
        capture_intent: CaptureIntent::Diagnostic,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .unwrap();

    let rules = cfg["route"]["rules"].as_array().unwrap();
    assert_eq!(
        rules.len(),
        4,
        "2 clash_mode rules + custom geoip + subscription domain rule survive"
    );
    let all = serde_json::to_string(rules).unwrap();
    assert!(
        !all.contains("geosite"),
        "geosite rules must be dropped: {all}"
    );
    assert!(
        cfg["route"]["rule_set"]
            .as_array()
            .is_some_and(|s| s.iter().any(|e| e["tag"] == "geoip-cn")),
        "custom geoip must materialize its rule-set"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn dns_block_present() {
    let cfg = build_runtime_json(&build_input_from_nodes(
        LocalTemplate::default(),
        vec![socks("a")],
        None,
    ))
    .unwrap();
    assert_eq!(cfg["dns"]["final"], "local");
    assert_eq!(cfg["dns"]["servers"][0]["type"], "local");

    let mut windows_input =
        build_input_from_nodes(LocalTemplate::default(), vec![socks("a")], None);
    windows_input.platform = HostPlatform::Windows;
    let win = build_runtime_json(&windows_input).unwrap();
    assert_eq!(win["dns"]["final"], "dns-remote-0");
    assert_eq!(win["dns"]["servers"][0]["type"], "tls");
    assert_eq!(win["dns"]["strategy"], "ipv4_only");
    assert_eq!(win["route"]["default_domain_resolver"], "dns-remote-0");
}

#[test]
fn windows_resolver_keeps_a_direct_final_tag() {
    // The minimal block: the `final` server is an IP-hosted DoT anchor.
    let dns = json!({
        "servers": [
            { "type": "tls", "tag": "dns-remote-0", "server": "223.5.5.5", "server_port": 853 },
            { "type": "tls", "tag": "dns-remote-1", "server": "119.29.29.29", "server_port": 853 },
        ],
        "final": "dns-remote-0",
    });
    assert_eq!(
        windows_default_domain_resolver(&dns).as_deref(),
        Some("dns-remote-0")
    );

    // An explicit `detour: direct` dials directly too.
    let explicit_direct = json!({
        "servers": [
            { "type": "tls", "tag": "cn-dns", "server": "223.5.5.5", "detour": "direct" },
            { "type": "https", "tag": "remote-dns", "server": "1.1.1.1", "detour": "proxy" },
        ],
        "final": "cn-dns",
    });
    assert_eq!(
        windows_default_domain_resolver(&explicit_direct).as_deref(),
        Some("cn-dns")
    );

    // A `final` tag matching no server keeps the tag; the builder replaces
    // such blocks via `dns_block_is_usable` before this runs.
    let dangling_final = json!({
        "servers": [{ "type": "tls", "tag": "cn-dns", "server": "223.5.5.5" }],
        "final": "missing",
    });
    assert_eq!(
        windows_default_domain_resolver(&dangling_final).as_deref(),
        Some("missing")
    );
}

#[test]
fn windows_resolver_skips_a_detoured_final_tag() {
    // The injected uri_list block: `final` is the proxy-detoured `remote-dns`,
    // so the route default must move to the directly-dialable `cn-dns`.
    let dns = json!({
        "servers": [
            { "type": "tls", "tag": "cn-dns", "server": "223.5.5.5", "server_port": 853 },
            {
                "type": "https",
                "tag": "remote-dns",
                "server": "1.1.1.1",
                "server_port": 443,
                "path": "/dns-query",
                "detour": "proxy",
            },
        ],
        "final": "remote-dns",
    });
    assert_eq!(
        windows_default_domain_resolver(&dns).as_deref(),
        Some("cn-dns")
    );

    // Untagged servers are never picked, even when they dial directly.
    let untagged_first = json!({
        "servers": [
            { "type": "tls", "server": "223.5.5.5", "server_port": 853 },
            { "type": "tls", "tag": "cn-dns", "server": "223.5.5.5", "server_port": 853 },
            { "type": "https", "tag": "remote-dns", "server": "1.1.1.1", "detour": "proxy" },
        ],
        "final": "remote-dns",
    });
    assert_eq!(
        windows_default_domain_resolver(&untagged_first).as_deref(),
        Some("cn-dns")
    );
}

#[test]
fn windows_resolver_skips_direct_servers_that_cannot_resolve_upstreams() {
    // `local` / `fakeip` / `rcode` dial directly but answer from the OS, a
    // synthetic pool, or a fixed error: none of them can resolve a node's
    // server domain, so the fallback walks past them to a real upstream.
    let dns = json!({
        "servers": [
            { "type": "local", "tag": "local" },
            { "type": "fakeip", "tag": "fakeip", "inet4_range": "198.18.0.0/15" },
            { "type": "rcode", "tag": "rcode" },
            { "type": "tls", "tag": "cn-dns", "server": "223.5.5.5", "server_port": 853 },
            { "type": "https", "tag": "remote-dns", "server": "1.1.1.1", "detour": "proxy" },
        ],
        "final": "remote-dns",
    });
    assert_eq!(
        windows_default_domain_resolver(&dns).as_deref(),
        Some("cn-dns")
    );
}

#[test]
fn windows_resolver_stays_unset_without_a_direct_upstream_resolver() {
    // The only direct servers cannot resolve upstream names and every real
    // resolver is detoured: the key stays unset instead of pointing at one of
    // them.
    let dns = json!({
        "servers": [
            { "type": "rcode", "tag": "rcode" },
            { "type": "local", "tag": "local" },
            { "type": "https", "tag": "remote-dns", "server": "1.1.1.1", "detour": "proxy" },
        ],
        "final": "remote-dns",
    });
    assert_eq!(windows_default_domain_resolver(&dns), None);
}

#[test]
fn windows_resolver_stays_unset_when_every_server_is_detoured() {
    // A detoured resolver is the DNS loop itself (the proxy dial needs the
    // node's server domain resolved first), so there is no fallback: the
    // route key stays unset and sing-box keeps its own resolution instead.
    let dns = json!({
        "servers": [
            { "type": "tls", "tag": "cn-dns", "server": "223.5.5.5", "detour": "proxy" },
            { "type": "https", "tag": "remote-dns", "server": "1.1.1.1", "detour": "proxy" },
        ],
        "final": "remote-dns",
    });
    assert_eq!(windows_default_domain_resolver(&dns), None);
}

#[test]
fn windows_resolver_is_none_without_a_final_tag() {
    let dns = json!({
        "servers": [{ "type": "tls", "tag": "cn-dns", "server": "223.5.5.5" }],
    });
    assert_eq!(windows_default_domain_resolver(&dns), None);
}

#[test]
fn disabled_rules_dropped_and_custom_rules_prepended() {
    let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
    profile.route.rules = vec![
        json!({ "domain_suffix": ["keep.com"], "outbound": "direct" }),
        json!({ "domain_suffix": ["drop.com"], "outbound": "direct" }),
    ];
    let drop_fp = rule_fingerprint(&profile.route.rules[1]);
    let mut overrides = RuleOverrides::default();
    overrides.set_disabled(drop_fp, true);
    overrides.custom.push(json!({
        "domain_suffix": ["custom.com"],
        "outbound": "block",
    }));
    overrides
        .custom
        .push(json!({ "domain": ["off.com"], "outbound": "direct" }));
    let off_fp = rule_fingerprint(&overrides.custom[1]);
    overrides.set_disabled(off_fp, true);

    let cfg = build_runtime_json(&BuildInput {
        template: LocalTemplate::default(),
        profile: Arc::new(profile),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: overrides,
        capture_intent: CaptureIntent::Diagnostic,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .unwrap();

    let rules = cfg["route"]["rules"].as_array().unwrap();
    let tags: Vec<String> = rules
        .iter()
        .filter(|r| r.get("domain_suffix").is_some() || r.get("domain").is_some())
        .map(|r| {
            r["domain_suffix"][0]
                .as_str()
                .or_else(|| r["domain"][0].as_str())
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(
        tags,
        ["custom.com", "keep.com"],
        "clash_mode rules first, then custom, disabled dropped"
    );
    assert!(!tags.iter().any(|t| t == "drop.com"));
    assert!(!tags.iter().any(|t| t == "off.com"));
}

#[test]
fn custom_rules_with_unknown_outbound_skipped_not_fatal() {
    let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
    profile.route.rules = vec![json!({
        "domain_suffix": ["keep.com"],
        "outbound": "direct",
    })];
    let mut overrides = RuleOverrides::default();
    overrides.custom.push(json!({
        "domain_suffix": ["ghost.com"],
        "outbound": "ghost-node",
    }));
    overrides.custom.push(json!({
        "domain_suffix": ["ok.com"],
        "outbound": "a",
    }));

    let cfg = build_runtime_json(&BuildInput {
        template: LocalTemplate::default(),
        profile: Arc::new(profile),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: overrides,
        capture_intent: CaptureIntent::Diagnostic,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .unwrap();

    let rules = cfg["route"]["rules"].as_array().unwrap();
    let kept: Vec<String> = rules
        .iter()
        .filter(|r| r.get("domain_suffix").is_some())
        .map(|r| r["domain_suffix"][0].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        kept,
        ["ok.com", "keep.com"],
        "custom rule referencing a missing outbound is skipped, not fatal"
    );
}

#[test]
fn custom_rules_with_unknown_rule_set_skipped_keeps_existing() {
    let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
    profile.route.rule_sets = vec![json!({
        "tag": "geoip-cn",
        "type": "local",
        "path": "geoip/geoip-cn.srs"
    })];
    let mut overrides = RuleOverrides::default();
    overrides.custom.push(json!({
        "rule_set": ["geoip-cn"],
        "outbound": "direct",
    }));
    overrides.custom.push(json!({
        "rule_set": ["geoip-us"],
        "outbound": "direct",
    }));

    let cfg = build_runtime_json(&BuildInput {
        template: LocalTemplate::default(),
        profile: Arc::new(profile),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: overrides,
        capture_intent: CaptureIntent::Diagnostic,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .unwrap();

    let rules = cfg["route"]["rules"].as_array().unwrap();
    let kept: Vec<String> = rules
        .iter()
        .filter_map(|r| {
            r.get("rule_set")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    assert_eq!(
        kept,
        ["geoip-cn"],
        "custom rule referencing a missing rule-set is skipped, not fatal"
    );
}

#[test]
fn remote_rule_sets_and_hosts_dns_are_dropped_for_user_mode() {
    let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
    profile.route.rule_sets = vec![
        json!({
            "tag": "evil",
            "type": "remote",
            "url": "https://evil.example/geoip.srs"
        }),
        json!({
            "tag": "ok",
            "type": "local",
            "path": "geoip/ok.srs"
        }),
    ];
    profile.route.rules = vec![
        json!({ "rule_set": ["evil"], "outbound": "direct" }),
        json!({ "domain_suffix": ["keep.com"], "outbound": "direct" }),
    ];
    profile.dns = Some(json!({
        "servers": [
            { "type": "hosts", "tag": "hosts", "path": "/etc/passwd" }
        ],
        "final": "hosts"
    }));

    let cfg = build_runtime_json(&BuildInput {
        template: LocalTemplate::default(),
        profile: Arc::new(profile),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Diagnostic,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .unwrap();

    let sets = cfg["route"]["rule_set"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        sets.iter().all(|s| s["tag"] != "evil"),
        "remote rule_set must not reach the user-mode core: {sets:?}"
    );
    assert!(
        sets.iter().any(|s| s["tag"] == "ok"),
        "local rule_set is kept: {sets:?}"
    );
    let rules = cfg["route"]["rules"].as_array().unwrap();
    assert!(
        rules
            .iter()
            .all(|r| r.get("rule_set") != Some(&json!(["evil"]))),
        "rules that only referenced the remote set must be dropped"
    );
    assert!(
        rules
            .iter()
            .any(|r| r.get("domain_suffix") == Some(&json!(["keep.com"]))),
        "unrelated rules stay"
    );
    assert!(
        cfg["dns"]["servers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["type"] != "hosts"),
        "hosts DNS must not reach the core"
    );
    assert_eq!(cfg["dns"]["final"], "local");
}

#[test]
fn subscription_rule_with_unknown_outbound_still_fails() {
    let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
    profile.route.rules = vec![json!({
        "domain_suffix": ["bad.com"],
        "outbound": "ghost-node",
    })];
    let err = build_runtime_config(&BuildInput {
        template: LocalTemplate::default(),
        profile: Arc::new(profile),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Diagnostic,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .expect_err("subscription rule with unknown outbound must still fail");
    assert!(matches!(err, ConfigError::RouteInvalid(_)));
}

#[test]
fn proxy_mode_rule_keeps_rules_and_subscription_final() {
    let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
    profile.route.final_outbound = "direct".into();
    profile.route.rules = vec![
        json!({ "domain_suffix": ["keep.com"], "outbound": "direct" }),
        json!({ "domain_suffix": ["proxy.com"], "outbound": "a" }),
    ];
    let cfg = build_runtime_json(&BuildInput {
        template: LocalTemplate::default(),
        profile: Arc::new(profile),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Diagnostic,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .unwrap();
    assert_eq!(cfg["route"]["final"], "direct");
    let rules = cfg["route"]["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 4, "2 clash_mode rules + 2 subscription rules");
    assert_eq!(
        clash_rules(&cfg),
        [("global", "proxy"), ("direct", "direct")]
    );
}

#[test]
fn proxy_mode_global_keeps_rules_with_clash_mode_global_target() {
    let profile = NormalizedProfile {
        nodes: vec![socks("a"), socks("b")],
        groups: vec![NormalizedOutbound {
            tag: "Proxies".into(),
            outbound: std::sync::Arc::new(json!({
                "type": "selector",
                "tag": "Proxies",
                "outbounds": ["a", "b"],
                "default": "a",
            })),
        }],
        route: NormalizedRoute {
            final_outbound: "direct".into(),
            rules: vec![
                json!({ "domain_suffix": ["keep.com"], "outbound": "direct" }),
                json!({ "domain_suffix": ["proxy.com"], "outbound": "a" }),
            ],
            rule_sets: vec![json!({"type": "local", "tag": "set-a", "path": "geoip/set-a.srs"})],
        },
        dns: None,
        default_outbound: Some("Proxies".into()),
        parse_stats: ProfileParseStats::default(),
    };
    let cfg = build_runtime_json(&BuildInput {
        template: LocalTemplate {
            proxy_mode: ProxyMode::Global,
            ..LocalTemplate::default()
        },
        profile: Arc::new(profile),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Diagnostic,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .unwrap();
    assert_eq!(
        cfg["route"]["final"], "direct",
        "final stays at rule-mode value"
    );
    assert_eq!(
        clash_rules(&cfg),
        [("global", "Proxies"), ("direct", "direct")],
        "global clash rule targets the top group so node selection keeps working"
    );
    assert!(
        cfg["route"]["rule_set"]
            .as_array()
            .is_some_and(|s| s.iter().any(|e| e["tag"] == "set-a")),
        "global mode must keep rule_sets"
    );
}

#[test]
fn proxy_mode_global_without_groups_uses_injected_proxy_selector() {
    let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
    profile.route.final_outbound = "direct".into();
    let cfg = build_runtime_json(&BuildInput {
        template: LocalTemplate {
            proxy_mode: ProxyMode::Global,
            ..LocalTemplate::default()
        },
        profile: Arc::new(profile),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Diagnostic,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .unwrap();
    assert_eq!(cfg["route"]["final"], "direct");
    assert_eq!(
        clash_rules(&cfg)[0],
        ("global", "proxy"),
        "flat profiles route global through the injected proxy selector"
    );
    assert!(
        cfg["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["tag"] == "proxy" && o["type"] == "selector"),
        "injected proxy selector must exist"
    );
}

#[test]
fn proxy_mode_direct_keeps_rules_with_clash_mode_direct() {
    let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
    profile.route.final_outbound = "a".into();
    profile.route.rules = vec![json!({ "domain_suffix": ["keep.com"], "outbound": "a" })];
    let cfg = build_runtime_json(&BuildInput {
        template: LocalTemplate {
            proxy_mode: ProxyMode::Direct,
            ..LocalTemplate::default()
        },
        profile: Arc::new(profile),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Diagnostic,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .unwrap();
    assert_eq!(cfg["route"]["final"], "a", "final stays at rule-mode value");
    assert_eq!(
        clash_rules(&cfg)[1],
        ("direct", "direct"),
        "direct clash rule routes everything direct"
    );
}

#[test]
fn clash_api_block_carries_default_mode_in_all_modes() {
    for mode in [ProxyMode::Rule, ProxyMode::Global, ProxyMode::Direct] {
        let cfg = build_runtime_json(&build_input_from_nodes(
            LocalTemplate {
                proxy_mode: mode,
                ..LocalTemplate::default()
            },
            vec![socks("a")],
            None,
        ))
        .unwrap();
        let api = &cfg["experimental"]["clash_api"];
        assert_eq!(api["default_mode"], clash_mode_name(mode));
        assert_eq!(api["secret"], ice_types::EXAMPLE_CLASH_API_SECRET);
        assert!(
            api.get("mode_list").is_none(),
            "mode_list must not be emitted (rejected by pinned sing-box 1.13.19)"
        );
        assert!(
            cfg["experimental"].get("cache_file").is_none(),
            "cache_file must stay disabled (Slice 4c lock)"
        );
    }
}

#[test]
fn restore_runtime_config_from_bak_copies_previous() {
    let dir = std::env::temp_dir().join(format!(
        "ice-box-config-bak-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let config = dir.join("config.json");
    let bak = dir.join("config.json.bak");
    fs::write(&config, b"new-bad").unwrap();
    fs::write(&bak, b"old-good").unwrap();

    assert!(restore_runtime_config_from_bak(&config, &bak).unwrap());
    assert_eq!(fs::read_to_string(&config).unwrap(), "old-good");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn restore_runtime_config_from_bak_missing_returns_false() {
    let dir = std::env::temp_dir().join(format!(
        "ice-box-config-no-bak-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let config = dir.join("config.json");
    fs::write(&config, b"keep").unwrap();

    assert!(!restore_runtime_config_from_bak(&config, &dir.join("config.json.bak")).unwrap());
    assert_eq!(fs::read_to_string(&config).unwrap(), "keep");

    let _ = fs::remove_dir_all(&dir);
}

// --- CaptureIntent, TUN config generation, structural intent checks ---

#[test]
fn tun_gate_status_is_stable_per_platform() {
    let macos = tun_gate_for(HostPlatform::MacOs);
    assert!(macos.ready, "macos_tun_ready is green after the T0 spike");
    assert_eq!(macos.reason, None);

    let windows = tun_gate_for(HostPlatform::Windows);
    assert!(
        windows.ready,
        "windows_tun_ready is green after the V1-V13 host spike"
    );
    assert_eq!(windows.reason, None);

    let linux = tun_gate_for(HostPlatform::Linux);
    assert!(!linux.ready, "Linux has no TUN backend");
    assert!(linux.reason.is_some());

    assert!(tun_gate_for(HostPlatform::Android).ready);
    assert!(tun_gate_for(HostPlatform::Ios).ready);
}

/// TUN parameters with an explicit interface name (required at build time).
fn tun_template() -> LocalTemplate {
    LocalTemplate {
        tun: TunSettings {
            enabled: true,
            interface_name: Some("utun420".into()),
            ..TunSettings::default()
        },
        ..LocalTemplate::default()
    }
}

#[test]
fn diagnostic_intent_never_emits_tun_inbound_even_when_tun_enabled() {
    let template = LocalTemplate {
        tun: TunSettings {
            enabled: true,
            ..TunSettings::default()
        },
        ..LocalTemplate::default()
    };
    // A requested Tun config is rejected by the gate / interface-name checks on
    // non-green platforms, but Diagnostic must build identically everywhere.
    let cfg = build_runtime_json(&BuildInput {
        template: template.clone(),
        profile: Arc::new(NormalizedProfile::from_nodes_only(vec![socks("a")])),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Diagnostic,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .expect("diagnostic build");
    assert_eq!(
        cfg["inbounds"].as_array().unwrap().len(),
        1,
        "tun.enabled=true must not add a tun inbound under Diagnostic intent"
    );
    assert_eq!(cfg["inbounds"][0]["type"], "mixed");

    let direct =
        build_direct_only_config(&template, CaptureIntent::Diagnostic, HostPlatform::MacOs)
            .expect("diagnostic direct-only");
    assert_eq!(direct["inbounds"].as_array().unwrap().len(), 1);
    validate_config_for_intent(&cfg, CaptureIntent::Diagnostic).expect("structural check");
}

#[test]
fn tun_config_has_both_inbounds_and_locked_shape() {
    let cfg = build_runtime_json(&BuildInput {
        template: tun_template(),
        profile: Arc::new(NormalizedProfile::from_nodes_only(vec![socks("a")])),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Tun,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .expect("tun build");
    validate_config_for_intent(&cfg, CaptureIntent::Tun).expect("structural check");

    let inbounds = cfg["inbounds"].as_array().unwrap();
    assert_eq!(inbounds.len(), 2, "mixed + tun");
    assert_eq!(inbounds[0]["type"], "mixed");
    assert_eq!(inbounds[0]["tag"], "mixed-in");

    let tun = &inbounds[1];
    assert_eq!(tun["type"], "tun");
    assert_eq!(tun["tag"], "tun-in");
    assert_eq!(tun["interface_name"], "utun420");
    assert_eq!(tun["address"][0], "10.0.0.1/30");
    assert_eq!(tun["address"][1], "fdfe:dcba:9876::1/126");
    assert_eq!(tun["mtu"], 9000);
    assert_eq!(tun["auto_route"], true);
    assert_eq!(tun["strict_route"], true);
    assert_eq!(tun["stack"], "gvisor");
    let excludes: Vec<&str> = tun["route_exclude_address"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        excludes,
        [
            "192.168.0.0/16",
            "10.0.0.0/8",
            "172.16.0.0/12",
            "127.0.0.0/8",
            "169.254.0.0/16",
            "224.0.0.0/4",
            "fe80::/10",
            "fc00::/7"
        ],
        "route_exclude_address must match the locked T0 shape"
    );
    assert_eq!(
        tun["loopback_address"],
        serde_json::json!(["127.0.0.1", "::1"])
    );
}

#[test]
fn tun_config_reserved_rules_precede_clash_mode_and_sniff_precedes_domain_rules() {
    let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
    profile.route.rules = vec![
        json!({ "domain_suffix": ["keep.com"], "outbound": "direct" }),
        json!({ "domain_suffix": ["proxy.com"], "outbound": "a" }),
    ];
    let cfg = build_runtime_json(&BuildInput {
        template: tun_template(),
        profile: Arc::new(profile),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Tun,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .expect("tun build");
    let rules = cfg["route"]["rules"].as_array().unwrap();
    assert_eq!(
        rules.len(),
        9,
        "4 reserved + 1 dns hijack + 2 clash_mode + 2 subscription rules"
    );

    assert_eq!(rules[0]["process_name"][0], "ice-box");
    assert_eq!(rules[0]["outbound"], "direct");
    assert_eq!(rules[1]["action"], "hijack-dns");
    assert_eq!(rules[1]["port"][0], 53);
    assert_eq!(rules[2]["ip_is_private"], true);
    assert_eq!(rules[2]["outbound"], "direct");
    assert_eq!(rules[3]["ip_cidr"][0], "127.0.0.0/8");
    assert_eq!(rules[4]["action"], "sniff");
    assert_eq!(rules[5]["clash_mode"], "global");
    assert_eq!(rules[6]["clash_mode"], "direct");
    assert_eq!(rules[7]["domain_suffix"][0], "keep.com");
    assert_eq!(rules[8]["domain_suffix"][0], "proxy.com");

    // Global mode must never bypass the reserved rules: the clash_mode rule
    // still targets the proxy while the control path stays direct.
    assert_eq!(rules[5]["outbound"], "proxy");
}

#[test]
fn tun_config_works_for_every_proxy_mode_and_direct_only_keeps_tun() {
    for mode in [ProxyMode::Rule, ProxyMode::Global, ProxyMode::Direct] {
        let template = LocalTemplate {
            proxy_mode: mode,
            ..tun_template()
        };
        let cfg = build_runtime_json(&BuildInput {
            template: template.clone(),
            profile: Arc::new(NormalizedProfile::from_nodes_only(vec![socks("a")])),
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Tun,
            tun_exclude_package: None,
            shared_root: None,
            platform: HostPlatform::MacOs,
        })
        .expect("tun build per mode");
        validate_config_for_intent(&cfg, CaptureIntent::Tun).expect("structural check");
        assert_eq!(
            cfg["experimental"]["clash_api"]["default_mode"],
            clash_mode_name(mode)
        );

        let direct = build_direct_only_config(&template, CaptureIntent::Tun, HostPlatform::MacOs)
            .expect("tun direct-only per mode");
        validate_config_for_intent(&direct, CaptureIntent::Tun)
            .expect("direct-only Tun keeps the tun inbound");
        let rules = direct["route"]["rules"].as_array().unwrap();
        assert_eq!(rules[0]["process_name"][0], "ice-box");
        assert_eq!(rules[1]["action"], "hijack-dns");
        assert_eq!(
            clash_rules(&direct),
            [("global", "direct"), ("direct", "direct")]
        );
    }
}

#[test]
fn tun_config_requires_interface_name_at_build_time() {
    let template = LocalTemplate {
        tun: TunSettings {
            enabled: true,
            interface_name: None,
            ..TunSettings::default()
        },
        ..LocalTemplate::default()
    };
    let err = build_runtime_config(&BuildInput {
        template: template.clone(),
        profile: Arc::new(NormalizedProfile::from_nodes_only(vec![socks("a")])),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Tun,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .expect_err("interface name required");
    assert!(matches!(err, ConfigError::TunInvalid(_)));
    assert!(build_direct_only_config(&template, CaptureIntent::Tun, HostPlatform::MacOs).is_err());
}

#[test]
fn tun_config_rejects_invalid_mtu_and_address_at_build_time() {
    let bad_mtu = LocalTemplate {
        tun: TunSettings {
            mtu: 576,
            interface_name: Some("utun420".into()),
            ..TunSettings::default()
        },
        ..tun_template()
    };
    let err = build_runtime_config(&BuildInput {
        template: bad_mtu,
        profile: Arc::new(NormalizedProfile::from_nodes_only(vec![socks("a")])),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Tun,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .expect_err("bad mtu");
    assert!(matches!(err, ConfigError::TunInvalid(_)));

    let bad_addr = LocalTemplate {
        tun: TunSettings {
            ipv6_address: "10.0.0.1/24".into(),
            interface_name: Some("utun420".into()),
            ..TunSettings::default()
        },
        ..tun_template()
    };
    assert!(build_direct_only_config(&bad_addr, CaptureIntent::Tun, HostPlatform::MacOs).is_err());
}

#[test]
fn tun_intent_is_rejected_on_platforms_without_a_green_gate() {
    // Linux / other hosts: Tun generation must fail closed with the stable
    // unavailable error, never emit a tun inbound.
    let template = LocalTemplate {
        tun: TunSettings {
            enabled: true,
            interface_name: Some("utun420".into()),
            ..TunSettings::default()
        },
        ..LocalTemplate::default()
    };
    let err = build_runtime_config(&BuildInput {
        template: template.clone(),
        profile: Arc::new(NormalizedProfile::from_nodes_only(vec![socks("a")])),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Tun,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::Linux,
    })
    .expect_err("tun gate not green");
    assert!(matches!(err, ConfigError::TunUnavailable(_)));
    assert!(build_direct_only_config(&template, CaptureIntent::Tun, HostPlatform::Linux).is_err());
}

#[test]
fn windows_reserved_rules_put_port_53_hijack_first_and_reject_the_tun_peer() {
    // The locked Windows shape (`docs/tun.md`): IPv4 port-53 hijack first
    // (`protocol: dns` does not fire in time on Windows), process bypass,
    // sniff, then the TUN sub-ranges rejected BEFORE `ip_is_private` can
    // loop them back into the core (#4455). No post-sniff `protocol: dns`
    // hijack: that path feeds STUN/mDNS/LLMNR into the DNS engine (#4199).
    // UDP 443 (QUIC/HTTP3) is rejected with the default ICMP method because
    // UDP user traffic is broken under the Windows shape; a silent
    // `block`/`drop` black-holes HTTP/3 and Chrome/Edge never fall back to
    // the proven IPv4 TCP path.
    let tun = TunSettings {
        dns_hijack: false, // Windows always emits the hijack rules
        ..TunSettings::default()
    };
    let rules = tun_reserved_rules_for(&tun, true);
    assert_eq!(rules.len(), 7);
    assert_eq!(rules[0]["action"], "hijack-dns");
    assert_eq!(rules[0]["port"][0], 53);
    assert_eq!(rules[0]["ip_version"], 4);
    assert_eq!(rules[1]["process_name"][0], "ice-box");
    assert_eq!(rules[1]["outbound"], "direct");
    assert_eq!(rules[2]["action"], "sniff");
    assert!(
        rules
            .iter()
            .all(|r| r.get("protocol") != Some(&json!("dns"))),
        "post-sniff protocol-dns hijack must not be emitted: {rules:?}"
    );
    let reject = &rules[3];
    assert_eq!(reject["action"], "reject");
    assert_eq!(reject["method"], "drop");
    let cidrs: Vec<&str> = reject["ip_cidr"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c.as_str())
        .collect();
    assert_eq!(
        cidrs,
        vec!["10.0.0.0/30", "fdfe:dcba:9876::/126"],
        "peer-reject covers the TUN sub-ranges derived from the addresses"
    );
    let quic = &rules[4];
    assert_eq!(quic["network"], "udp");
    assert_eq!(quic["port"][0], 443);
    assert_eq!(quic["action"], "reject");
    assert!(
        quic.get("method").is_none() && quic.get("outbound").is_none(),
        "default reject (ICMP) — not drop/block, so HTTP/3 fails fast"
    );
    assert_eq!(rules[5]["ip_is_private"], true);
    assert_eq!(rules[6]["ip_cidr"][0], "127.0.0.0/8");

    let generic = tun_reserved_rules_for(&tun, false);
    assert_eq!(generic.len(), 4, "macOS/generic shape is unchanged");
    assert_eq!(generic[0]["process_name"][0], "ice-box");
    assert!(
        generic.iter().all(|r| r["action"] != "reject"),
        "no peer-reject rule in the generic shape"
    );
}

#[test]
fn windows_tun_runtime_config_emits_ipv4_port_53_hijack_without_protocol_dns() {
    let template = LocalTemplate {
        tun: TunSettings {
            enabled: true,
            interface_name: Some("Wintun".into()),
            ..TunSettings::default()
        },
        ..LocalTemplate::default()
    };
    let cfg = build_runtime_json(&BuildInput {
        template,
        profile: Arc::new(NormalizedProfile::from_nodes_only(vec![socks("a")])),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Tun,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::Windows,
    })
    .expect("windows tun build");
    let rules = cfg["route"]["rules"].as_array().unwrap();
    assert_eq!(rules[0]["action"], "hijack-dns");
    assert_eq!(rules[0]["ip_version"], 4);
    assert_eq!(rules[0]["port"][0], 53);
    assert!(
        rules
            .iter()
            .all(|r| r.get("protocol") != Some(&json!("dns"))),
        "protocol-dns hijack must not appear in the Windows TUN config"
    );
}

#[test]
fn validate_config_for_intent_rejects_intent_mismatch_everywhere() {
    // Platform-neutral structural checks: hand-built JSON, no builder gate.
    let mixed_only = json!({
        "inbounds": [ { "type": "mixed", "tag": "mixed-in" } ],
        "outbounds": [ { "type": "direct", "tag": "direct" } ],
    });
    let mixed_tun = json!({
        "inbounds": [
            { "type": "mixed", "tag": "mixed-in" },
            { "type": "tun", "tag": "tun-in" }
        ],
        "outbounds": [ { "type": "direct", "tag": "direct" } ],
    });
    let tun_only = json!({
        "inbounds": [ { "type": "tun", "tag": "tun-in" } ],
        "outbounds": [ { "type": "direct", "tag": "direct" } ],
    });
    let two_tun = json!({
        "inbounds": [
            { "type": "mixed", "tag": "mixed-in" },
            { "type": "tun", "tag": "tun-in" },
            { "type": "tun", "tag": "tun-in-2" }
        ],
        "outbounds": [ { "type": "direct", "tag": "direct" } ],
    });

    validate_config_for_intent(&mixed_only, CaptureIntent::Diagnostic).expect("mixed-only");
    validate_config_for_intent(&mixed_tun, CaptureIntent::Tun).expect("mixed+tun");

    assert!(
        matches!(
            validate_config_for_intent(&mixed_tun, CaptureIntent::Diagnostic),
            Err(ConfigError::Invalid(_))
        ),
        "Diagnostic must never carry a tun inbound"
    );
    assert!(
        matches!(
            validate_config_for_intent(&mixed_only, CaptureIntent::Tun),
            Err(ConfigError::Invalid(_))
        ),
        "a Mixed-only config must never be handed to a TUN activation"
    );
    assert!(
        matches!(
            validate_config_for_intent(&tun_only, CaptureIntent::Tun),
            Err(ConfigError::Invalid(_))
        ),
        "Tun config must keep the mixed inbound"
    );
    assert!(
        matches!(
            validate_config_for_intent(&two_tun, CaptureIntent::Tun),
            Err(ConfigError::Invalid(_))
        ),
        "exactly one tun inbound"
    );
}

#[test]
fn build_input_serde_preserves_capture_intent_and_defaults_to_diagnostic() {
    let value = serde_json::to_value(BuildInput {
        template: LocalTemplate::default(),
        profile: Arc::new(NormalizedProfile::from_nodes_only(vec![socks("a")])),
        selected_tag: None,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Tun,
        tun_exclude_package: None,
        shared_root: None,
        platform: HostPlatform::MacOs,
    })
    .expect("serialize");
    assert_eq!(value["capture_intent"], "tun");

    let legacy = serde_json::json!({
        "template": LocalTemplate::default(),
        "profile": NormalizedProfile::from_nodes_only(vec![socks("a")]),
        "selected_tag": null,
        "geoip_rule_set_dir": null,
        "group_selections": {},
        "rule_overrides": {},
    });
    let parsed: BuildInput = serde_json::from_value(legacy).expect("legacy input");
    assert_eq!(parsed.capture_intent, CaptureIntent::Diagnostic);
    assert_eq!(parsed.platform, HostPlatform::MacOs);
    assert_eq!(parsed.template.tun, TunSettings::default());
}

fn guard_ctx() -> ice_config_guard::GuardContext {
    ice_config_guard::GuardContext {
        data_dir: std::env::temp_dir(),
        resources_dir: std::env::temp_dir(),
        log_output: None,
        cache_file_path: None,
        rule_set_staging_dir: None,
    }
}

fn assert_guard_unchanged(cfg: Value) {
    let before = cfg.clone();
    let mut after = cfg;
    ice_config_guard::sanitize_for_elevated_core(&mut after, &guard_ctx())
        .unwrap_or_else(|err| panic!("generated config rejected: {err}"));
    assert_eq!(before, after, "guard must not rewrite generated configs");
}

#[test]
fn generated_diagnostic_configs_pass_elevated_guard_unchanged() {
    let direct = build_direct_only_config(
        &LocalTemplate::default(),
        CaptureIntent::Diagnostic,
        HostPlatform::MacOs,
    )
    .expect("direct");
    assert_guard_unchanged(direct);

    let runtime = build_runtime_json(&build_input_from_nodes(
        LocalTemplate::default(),
        vec![socks("a"), socks("b")],
        None,
    ))
    .expect("runtime");
    assert!(runtime.get("endpoints").is_none());
    assert_guard_unchanged(runtime);
}

#[test]
fn mobile_ignores_diagnostic_intent_and_emits_tun() {
    for platform in [HostPlatform::Android, HostPlatform::Ios] {
        let cfg = build_direct_only_config(
            &LocalTemplate::default(),
            CaptureIntent::Diagnostic,
            platform,
        )
        .expect("mobile direct");
        let inbounds = cfg["inbounds"].as_array().expect("inbounds");
        assert_eq!(inbounds.len(), 1);
        assert_eq!(inbounds[0]["type"], "tun");
        assert_eq!(inbounds[0]["tag"], "tun-in");
        assert!(inbounds[0].get("interface_name").is_none());
        let excludes: Vec<&str> = inbounds[0]["route_exclude_address"]
            .as_array()
            .expect("route_exclude_address")
            .iter()
            .filter_map(|value| value.as_str())
            .collect();
        assert_eq!(
            excludes,
            [
                "192.168.0.0/16",
                "10.0.0.0/8",
                "172.16.0.0/12",
                "127.0.0.0/8",
                "169.254.0.0/16",
                "224.0.0.0/4",
                "fe80::/10",
                "fc00::/7",
            ]
        );
        assert!(inbounds[0].get("loopback_address").is_none());
        assert!(inbounds[0].get("exclude_package").is_none());
        assert_eq!(inbounds[0]["stack"], "gvisor");
        let rules = cfg["route"]["rules"].as_array().expect("rules");
        assert_eq!(rules[0]["action"], "sniff");
        assert_eq!(rules[1]["ip_is_private"], true);
        assert_eq!(rules[1]["outbound"], "direct");
        assert_eq!(rules[2]["ip_cidr"][0], "127.0.0.0/8");
        assert_eq!(rules[2]["outbound"], "direct");
        assert_eq!(rules[3]["protocol"], "dns");
        assert_eq!(rules[3]["action"], "hijack-dns");
        assert!(rules.iter().all(|rule| rule.get("process_name").is_none()));
        assert_eq!(cfg["dns"]["final"], "local");
        assert!(cfg.get("endpoints").is_none());
        assert!(cfg["log"].get("output").is_none());
    }
}

#[test]
fn android_exclude_package_is_emitted_and_ios_ignores_it() {
    let mut input = build_input_from_nodes(LocalTemplate::default(), vec![socks("a")], None);
    input.platform = HostPlatform::Android;
    input.tun_exclude_package = Some("com.yilongmusk.icebox".into());
    let cfg = build_runtime_json(&input).expect("android");
    assert_eq!(
        cfg["inbounds"][0]["exclude_package"][0],
        "com.yilongmusk.icebox"
    );
    assert!(cfg["inbounds"]
        .as_array()
        .unwrap()
        .iter()
        .all(|inbound| inbound["type"] != "mixed"));

    input.platform = HostPlatform::Ios;
    input.tun_exclude_package = Some("com.yilong-musk.icebox".into());
    let cfg = build_runtime_json(&input).expect("ios ignores a desktop-illegal id");
    assert!(cfg["inbounds"][0].get("exclude_package").is_none());

    input.platform = HostPlatform::Android;
    input.tun_exclude_package = Some("com.yilong-musk.icebox".into());
    assert!(build_runtime_config(&input).is_err());
}

#[test]
fn mobile_shared_root_sets_cache_and_log_and_desktop_ignores_it() {
    let root = PathBuf::from("/data/user/0/com.yilongmusk.icebox/files");
    let mut input = build_input_from_nodes(LocalTemplate::default(), vec![socks("a")], None);
    input.platform = HostPlatform::Android;
    input.shared_root = Some(root.clone());
    let cfg = build_runtime_json(&input).expect("android shared root");
    assert_eq!(
        cfg["log"]["output"],
        root.join("sing-box.log").to_string_lossy().as_ref()
    );
    assert_eq!(cfg["experimental"]["cache_file"]["enabled"], true);
    assert_eq!(
        cfg["experimental"]["cache_file"]["path"],
        root.join("cache.db").to_string_lossy().as_ref()
    );

    input.platform = HostPlatform::MacOs;
    let cfg = build_runtime_json(&input).expect("desktop ignores shared root");
    assert!(cfg["log"].get("output").is_none());
    assert!(cfg["experimental"].get("cache_file").is_none());
    assert_eq!(cfg["inbounds"][0]["type"], "mixed");
}

#[test]
fn wireguard_node_becomes_an_endpoint() {
    let node = NormalizedOutbound {
        tag: "WG-01".into(),
        outbound: Arc::new(json!({
            "type": "wireguard",
            "tag": "WG-01",
            "server": "wg.example.xyz",
            "server_port": 443,
            "private_key": "privkey",
            "peer_public_key": "pubkey",
            "preshared_key": "psk",
            "reserved": [0, 1, 2],
            "local_address": ["10.0.0.1/32"],
            "mtu": 1420
        })),
    };
    let cfg = build_runtime_json(&build_input_from_nodes(
        LocalTemplate::default(),
        vec![node],
        None,
    ))
    .expect("wireguard");
    assert!(cfg["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .all(|outbound| outbound["type"] != "wireguard"));
    let endpoint = &cfg["endpoints"][0];
    assert_eq!(endpoint["type"], "wireguard");
    assert_eq!(endpoint["tag"], "WG-01");
    assert_eq!(endpoint["address"][0], "10.0.0.1/32");
    assert_eq!(endpoint["private_key"], "privkey");
    assert_eq!(endpoint["mtu"], 1420);
    assert!(endpoint.get("local_address").is_none());
    assert!(endpoint.get("server").is_none());
    assert_eq!(endpoint["peers"][0]["address"], "wg.example.xyz");
    assert_eq!(endpoint["peers"][0]["port"], 443);
    assert_eq!(endpoint["peers"][0]["public_key"], "pubkey");
    assert_eq!(endpoint["peers"][0]["pre_shared_key"], "psk");
    assert_eq!(endpoint["peers"][0]["reserved"], json!([0, 1, 2]));
    assert_eq!(
        endpoint["peers"][0]["allowed_ips"],
        json!(["0.0.0.0/0", "::/0"])
    );
    let proxy = cfg["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|outbound| outbound["tag"] == "proxy")
        .expect("proxy selector");
    assert!(proxy["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tag| tag == "WG-01"));
    assert_guard_unchanged(cfg);

    let bare = NormalizedOutbound {
        tag: "WG-bare".into(),
        outbound: Arc::new(json!({
            "type": "wireguard",
            "tag": "WG-bare",
            "server": "1.2.3.4",
            "server_port": 51820,
            "private_key": "priv",
            "peer_public_key": "pub"
        })),
    };
    let cfg = build_runtime_json(&build_input_from_nodes(
        LocalTemplate::default(),
        vec![bare],
        None,
    ))
    .expect("default address");
    assert_eq!(cfg["endpoints"][0]["address"][0], "10.0.0.2/32");
    assert!(cfg["endpoints"][0].get("mtu").is_none());
}

#[test]
fn build_mobile_config_rejects_desktop_platforms() {
    let input = build_input_from_nodes(LocalTemplate::default(), vec![socks("a")], None);
    let err = build_mobile_config(&input).expect_err("desktop");
    assert!(matches!(err, ConfigError::Invalid(_)));
}

#[test]
fn share_singbox_config_is_a_portable_document() {
    let shared = share_singbox_config(&NormalizedProfile {
        nodes: vec![NormalizedOutbound::new(
            "n1",
            json!({
                "type": "vmess",
                "tag": "n1",
                "server": "1.2.3.4",
                "uuid": "real-uuid"
            }),
        )],
        groups: Vec::new(),
        route: NormalizedRoute {
            rules: vec![
                json!({"domain": ["example.com"], "outbound": "proxy"}),
                json!({"geoip": ["cn"], "outbound": "direct"}),
                json!({"geosite": ["cn"], "outbound": "direct"}),
                json!({"domain": ["missing.example"], "outbound": "gone"}),
            ],
            final_outbound: "proxy".into(),
            rule_sets: Vec::new(),
        },
        dns: Some(json!({
            "__ice_dns_listen": "127.0.0.1:53",
            "servers": [{"type": "local", "tag": "local"}],
            "final": "local"
        })),
        default_outbound: None,
        parse_stats: ProfileParseStats::default(),
    })
    .expect("share");
    let value: Value = serde_json::from_str(&shared).expect("json");
    assert!(value.get("inbounds").is_none());
    assert!(value.get("experimental").is_none());
    assert!(value.get("log").is_none());
    assert!(value.get("dns").unwrap().get("__ice_dns_listen").is_none());
    assert_eq!(value["route"]["default_domain_resolver"], "local");
    assert_eq!(value["route"]["final"], "proxy");
    let rules = value["route"]["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["domain"][0], "example.com");
    let tags: Vec<&str> = value["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|outbound| outbound["tag"].as_str())
        .collect();
    assert!(tags.contains(&"n1"));
    assert!(tags.contains(&"proxy"));
    assert!(tags.contains(&"direct"));
    assert!(tags.contains(&"block"));
    let node = value["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|outbound| outbound["tag"] == "n1")
        .unwrap();
    assert_eq!(node["uuid"], "real-uuid");

    let grouped = share_singbox_config(&NormalizedProfile {
        nodes: vec![socks("n1")],
        groups: vec![NormalizedOutbound::new(
            "G",
            json!({"type": "selector", "tag": "G", "outbounds": ["n1"]}),
        )],
        route: NormalizedRoute {
            rules: Vec::new(),
            final_outbound: "G".into(),
            rule_sets: Vec::new(),
        },
        dns: None,
        default_outbound: Some("G".into()),
        parse_stats: ProfileParseStats::default(),
    })
    .expect("grouped");
    let grouped: Value = serde_json::from_str(&grouped).unwrap();
    assert!(grouped["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .all(|outbound| outbound["tag"] != "proxy"));
    assert_eq!(grouped["route"]["final"], "G");

    let wireguard = share_singbox_config(&NormalizedProfile::from_nodes_only(vec![
        NormalizedOutbound::new(
            "wg",
            json!({
                "type": "wireguard",
                "tag": "wg",
                "server": "1.2.3.4",
                "server_port": 51820,
                "private_key": "priv",
                "peer_public_key": "pub"
            }),
        ),
    ]))
    .expect("wireguard");
    let wireguard: Value = serde_json::from_str(&wireguard).unwrap();
    assert!(wireguard["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .all(|outbound| outbound["type"] != "wireguard"));
    assert_eq!(wireguard["endpoints"][0]["type"], "wireguard");
    assert_eq!(wireguard["endpoints"][0]["private_key"], "priv");

    let empty = share_singbox_config(&NormalizedProfile::from_nodes_only(vec![]));
    assert!(matches!(empty, Err(ConfigError::EmptyOutbounds)));
}

#[test]
fn example_minimal_direct_passes_elevated_guard() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let raw = std::fs::read_to_string(repo.join("configs/examples/minimal-direct.json"))
        .expect("example");
    let cfg: Value = serde_json::from_str(&raw).expect("json");
    assert_guard_unchanged(cfg);
}
