//! Clash YAML → full `NormalizedProfile`.

mod dns;
mod groups;
mod names;
mod proxies;
mod rules;

use std::collections::HashSet;

use ice_config::{NormalizedProfile, ProfileParseStats};
use serde_json::Value;

use crate::error::SubscriptionError;

pub use proxies::{parse_proxies, CLASH_SUPPORTED_TYPES, MAX_CLASH_PROXIES};

#[derive(Debug, Clone)]
pub struct ClashParseResult {
    pub profile: NormalizedProfile,
}

/// Backward-compatible node-only parse for existing tests.
pub fn parse_clash_with_stats(raw: &str) -> Result<ClashParseResult, SubscriptionError> {
    let profile = parse_clash_profile(raw)?;
    Ok(ClashParseResult { profile })
}

pub fn parse_clash_profile(raw: &str) -> Result<NormalizedProfile, SubscriptionError> {
    let doc: Value = serde_yaml::from_str(raw)
        .map_err(|e| SubscriptionError::ParseFailed(format!("clash yaml: {e}")))?;

    let proxy_result = parse_proxies(&doc).map_err(|e| match e {
        proxies::SkipReason::TooMany => SubscriptionError::ParseFailed(format!(
            "proxies count exceeds limit {MAX_CLASH_PROXIES}"
        )),
        _ => SubscriptionError::EmptyNodes,
    })?;

    let mut known: HashSet<String> = proxy_result.nodes.iter().map(|n| n.tag.clone()).collect();
    known.insert("direct".into());
    known.insert("block".into());

    // Group references resolve groups-first (plan §3.1): pre-register group names so
    // groups may reference other groups (e.g. Proxies → HK/JP/US sub-groups).
    if let Some(groups) = doc.get("proxy-groups").and_then(|v| v.as_array()) {
        for g in groups.iter().take(groups::MAX_CLASH_GROUPS) {
            if let Some(name) = g.get("name").and_then(|v| v.as_str()) {
                known.insert(name.to_string());
            }
        }
    }

    let group_result = groups::parse_groups(&doc, &known);
    for g in &group_result.groups {
        known.insert(g.tag.clone());
    }

    // Rules may only reference surviving outbounds: rebuild the known set from parsed
    // nodes and groups (plus builtins) so rules pointing at dropped proxies/groups are
    // dropped at parse time instead of failing the whole config build later.
    let mut rule_known: HashSet<String> =
        proxy_result.nodes.iter().map(|n| n.tag.clone()).collect();
    rule_known.extend(group_result.groups.iter().map(|g| g.tag.clone()));
    rule_known.insert("direct".into());
    rule_known.insert("block".into());

    let mut stats = ProfileParseStats {
        skipped_proxies: proxy_result.skipped,
        skipped_groups: group_result.skipped,
        warnings: group_result.warnings,
        ..Default::default()
    };

    let rule_known_vec: Vec<String> = rule_known.iter().cloned().collect();
    let rule_result = rules::parse_rules(&doc, &rule_known_vec);
    stats.skipped_rules += rule_result.stats.skipped_rules;
    stats
        .unsupported_rule_types
        .extend(rule_result.stats.unsupported_rule_types);
    stats.warnings.extend(rule_result.stats.warnings);
    for code in rule_result.stats.geoip_codes {
        if !stats.geoip_codes.contains(&code) {
            stats.geoip_codes.push(code);
        }
    }

    let (dns, dns_warnings) = dns::parse_dns(&doc);
    stats.warnings.extend(dns_warnings);

    let group_names: Vec<String> = group_result.groups.iter().map(|g| g.tag.clone()).collect();
    let default_outbound = names::detect_default_outbound(&group_names);

    let mut route = rule_result.route;
    if route.rules.is_empty() && default_outbound.is_some() {
        route.final_outbound = default_outbound.clone().unwrap();
    } else if default_outbound.is_some() && route.final_outbound == "direct" {
        if let Some(def) = default_outbound.clone() {
            if known.contains(&def) {
                route.final_outbound = def;
            }
        }
    }

    Ok(NormalizedProfile {
        nodes: proxy_result.nodes,
        groups: group_result.groups,
        route,
        dns,
        default_outbound,
        parse_stats: stats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../configs/examples")
    }

    #[test]
    fn g6_1_clash_ss() {
        let raw =
            std::fs::read_to_string(fixtures_dir().join("subscription-clash-ss.yaml")).unwrap();
        let profile = parse_clash_profile(&raw).unwrap();
        assert_eq!(profile.nodes.len(), 1);
        assert_eq!(profile.nodes[0].outbound["type"], "shadowsocks");
    }

    #[test]
    fn g6_6_clash_with_proxy_groups() {
        let raw =
            std::fs::read_to_string(fixtures_dir().join("subscription-clash-mixed.yaml")).unwrap();
        let profile = parse_clash_profile(&raw).unwrap();
        assert_eq!(profile.nodes.len(), 5);
        assert!(!profile.groups.is_empty());
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Instant;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../configs/examples")
    }

    #[test]
    fn s2_s3_full_profile_groups_rules_and_perf() {
        let raw =
            std::fs::read_to_string(fixtures_dir().join("subscription-clash-profile-full.yaml"))
                .unwrap();
        let start = Instant::now();
        let profile = parse_clash_profile(&raw).unwrap();
        let elapsed = start.elapsed();
        assert_eq!(profile.nodes.len(), 90);
        assert_eq!(profile.groups.len(), 21);
        assert_eq!(profile.route.rules.len(), 4260);
        assert_eq!(profile.parse_stats.skipped_rules, 0);
        assert_eq!(profile.default_outbound.as_deref(), Some("G01"));
        assert!(profile.parse_stats.geoip_codes.contains(&"cn".to_string()));
        assert!(
            profile
                .parse_stats
                .warnings
                .iter()
                .all(|w| !w.contains("GEOIP")),
            "GEOIP is supported via bundled rule-sets, no warning expected"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "parse took {elapsed:?}"
        );
    }

    #[test]
    fn s3_rules_min_covers_all_types() {
        let raw = std::fs::read_to_string(fixtures_dir().join("subscription-clash-rules-min.yaml"))
            .unwrap();
        let profile = parse_clash_profile(&raw).unwrap();
        let rule_types: Vec<String> = profile
            .route
            .rules
            .iter()
            .filter_map(|r| {
                r.as_object()
                    .and_then(|o| o.keys().find(|k| *k != "outbound").cloned())
            })
            .collect();
        for t in [
            "domain",
            "domain_suffix",
            "domain_keyword",
            "ip_cidr",
            "process_name",
            "geoip",
        ] {
            assert!(rule_types.contains(&t.to_string()), "missing {t}");
        }
        assert_eq!(profile.route.final_outbound, "Proxies");
        assert_eq!(profile.parse_stats.geoip_codes.as_slice(), ["cn"]);
        assert!(
            profile
                .parse_stats
                .unsupported_rule_types
                .iter()
                .all(|t| t != "GEOIP"),
            "GEOIP parses to rule_set references, not skipped"
        );
        assert!(profile
            .parse_stats
            .unsupported_rule_types
            .iter()
            .any(|t| t == "RULE-SET"));
        assert_eq!(profile.groups.len(), 4);
        let types: Vec<&str> = profile
            .groups
            .iter()
            .map(|g| g.outbound["type"].as_str().unwrap())
            .collect();
        assert!(types.contains(&"selector"));
        assert!(types.contains(&"urltest"));
        assert!(types.contains(&"fallback"));
        assert!(types.contains(&"loadbalance"));
    }

    #[test]
    fn s4_dns_fakeip_local_and_domain_resolver() {
        let raw =
            std::fs::read_to_string(fixtures_dir().join("subscription-clash-dns-fakeip.yaml"))
                .unwrap();
        let profile = parse_clash_profile(&raw).unwrap();
        let dns = profile.dns.expect("dns block");
        assert!(
            dns.get("listen").is_none(),
            "clash listen key must not leak"
        );
        assert!(
            dns.get("__ice_dns_listen").is_none(),
            "no internal listen carrying remains"
        );
        let fakeip = &dns["servers"][1];
        assert_eq!(fakeip["type"], "fakeip");
        assert_eq!(fakeip["inet4_range"], "198.18.0.1/16");
        let servers = dns["servers"].as_array().unwrap();
        assert!(
            servers
                .iter()
                .any(|s| s["tag"] == "local" && s["type"] == "local"),
            "fake-ip-filter rules reference local; local server must exist"
        );
        assert!(servers.iter().any(|s| s["server"] == "223.5.5.5"));
        assert!(servers.iter().any(|s| {
            s["server"] == "dns.alidns.com"
                && s["type"] == "https"
                && s["domain_resolver"] == "local"
        }));
        assert!(servers.iter().any(|s| {
            s["server"] == "1.1.1.1" && s["type"] == "tls" && s.get("domain_resolver").is_none()
        }));
        assert!(dns["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["server"] == "local"));
    }
}
