// SPDX-License-Identifier: GPL-3.0-or-later

//! Provider-reported subscription info: the `subscription-userinfo` response
//! header, and the usage / expiry entries a provider embeds in the proxy list.

use std::collections::HashSet;

use ice_config::{NormalizedOutbound, NormalizedProfile};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Traffic and expiry metadata a provider reports for a subscription:
/// `subscription-userinfo: upload=1; download=2; total=3; expire=1735689600`.
///
/// `total == 0` means the provider reports no quota (unlimited); the same
/// convention is used by other clients reading this header. Counters are byte
/// values. `expire` is `None` when the header omits it, sends it empty, or
/// sends `0` — panels that use `0` mean "no expiry", not the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SubscriptionUserInfo {
    #[serde(default)]
    pub upload: u64,
    #[serde(default)]
    pub download: u64,
    #[serde(default)]
    pub total: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expire: Option<i64>,
}

impl SubscriptionUserInfo {
    /// Bytes counted against the quota: `upload + download`.
    pub fn used(&self) -> u64 {
        self.upload.saturating_add(self.download)
    }

    /// Remaining quota, `None` when the provider reports no quota.
    pub fn remaining(&self) -> Option<u64> {
        (self.total > 0).then(|| self.total.saturating_sub(self.used()))
    }
}

/// Expiry values above this are read as milliseconds: a Unix timestamp in
/// seconds would be a date past year 5138, so a panel sending milliseconds
/// (13 digits instead of 10) is normalized instead of shown as a year-57000
/// expiry.
const MILLISECOND_EPOCH_THRESHOLD: u64 = 100_000_000_000;

/// Parse the `subscription-userinfo` header. Unknown keys, malformed pairs and
/// unusable values are ignored; `None` when no field carries a usable value
/// (an empty header, or only `expire=0`).
pub fn parse_userinfo_header(raw: &str) -> Option<SubscriptionUserInfo> {
    let mut info = SubscriptionUserInfo::default();
    let mut seen = false;

    for pair in raw.split(';') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "upload" => seen |= assign_counter(&mut info.upload, value),
            "download" => seen |= assign_counter(&mut info.download, value),
            "total" => seen |= assign_counter(&mut info.total, value),
            "expire" => {
                if let Some(expire) = parse_expire(value) {
                    info.expire = Some(expire);
                    seen = true;
                }
            }
            _ => {}
        }
    }

    seen.then_some(info)
}

fn assign_counter(slot: &mut u64, raw: &str) -> bool {
    match raw.parse::<u64>() {
        Ok(value) => {
            *slot = value;
            true
        }
        Err(_) => false,
    }
}

fn parse_expire(raw: &str) -> Option<i64> {
    let value = raw.parse::<u64>().ok()?;
    if value == 0 {
        return None;
    }
    let seconds = if value > MILLISECOND_EPOCH_THRESHOLD {
        value / 1000
    } else {
        value
    };
    i64::try_from(seconds).ok()
}

/// Longest name kept as provider info; longer ones are treated as node names.
const MAX_INFO_LINE_CHARS: usize = 160;

/// Most info entries kept per subscription.
const MAX_INFO_LINES: usize = 6;

/// Label/value separators providers use in info entries.
const INFO_SEPARATORS: [char; 4] = [':', '：', '|', '｜'];

/// Labels that mark an entry as provider info rather than a proxy. The text
/// before the first separator must be exactly these words (joined, case
/// insensitive), so a node name that merely contains one (`香港-流量专线|IEPL`,
/// `Unused: HK-01`) stays a proxy. `🇺🇸美国01|流媒体` has the keyword after the
/// separator and is not info either.
const INFO_LABELS: [&str; 26] = [
    "剩余流量",
    "流量",
    "剩余",
    "到期",
    "过期",
    "套餐",
    "重置",
    "距离下次",
    "官网",
    "客服",
    "订阅",
    "续费",
    "traffic",
    "expire",
    "expires",
    "expiry",
    "expired",
    "used",
    "remaining",
    "reset",
    "quota",
    "available",
    "website",
    "official",
    "subscribe",
    "subscription",
];

/// Allowed after a keyword label: `到期时间`, `套餐到期` is two keywords and
/// does not need a suffix.
const INFO_LABEL_SUFFIXES: [&str; 2] = ["时间", "日期"];

/// Info entries a provider embeds in the proxy list, in list order.
///
/// Panels publish quota and expiry as extra "proxies" whose names carry the
/// text (`Traffic: 11.84 GB | 150 GB`, `剩余流量：1023.64 GB`). Each panel words
/// and scales them its own way, so the names are stored as provider input for
/// consumers to parse into a usage / expiry readout.
/// [`detach_provider_info_nodes`] then drops those proxies from the profile.
pub fn provider_info_lines(nodes: &[NormalizedOutbound]) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for node in nodes {
        if lines.len() == MAX_INFO_LINES {
            break;
        }
        let name = node.tag.trim();
        if name.is_empty()
            || name.chars().count() > MAX_INFO_LINE_CHARS
            || !is_provider_info_name(name)
            || lines.iter().any(|line| line == name)
        {
            continue;
        }
        lines.push(name.to_string());
    }
    lines
}

pub(crate) fn is_provider_info_name(name: &str) -> bool {
    let mut parts = name.split(INFO_SEPARATORS);
    let label = parts.next().unwrap_or_default().trim();
    // Info entries are `label: value`; a name without a separator is a proxy
    // that merely mentions a keyword ("美国-流量专线 1").
    if parts.next().is_none() {
        return false;
    }
    // A label is one short word ("剩余流量", "Expire") or a few of those words
    // joined (`套餐到期`). A longer name that only contains a keyword is a proxy.
    if label.is_empty() || label.chars().count() > 24 {
        return false;
    }
    let label = label.to_lowercase();
    label_is_info_keywords(&label)
        || INFO_LABEL_SUFFIXES.iter().any(|suffix| {
            label
                .strip_suffix(suffix)
                .is_some_and(|rest| !rest.is_empty() && label_is_info_keywords(rest))
        })
}

/// True when `label` is one or more [`INFO_LABELS`] concatenated, and nothing else.
fn label_is_info_keywords(label: &str) -> bool {
    fn covers(label: &str, keywords: &[&str]) -> bool {
        if label.is_empty() {
            return true;
        }
        keywords.iter().any(|keyword| {
            label
                .strip_prefix(keyword)
                .is_some_and(|rest| covers(rest, keywords))
        })
    }

    !label.is_empty() && covers(label, info_keywords_longest_first())
}

fn info_keywords_longest_first() -> &'static [&'static str] {
    use std::sync::OnceLock;
    static SORTED: OnceLock<Vec<&'static str>> = OnceLock::new();
    SORTED
        .get_or_init(|| {
            let mut keys = INFO_LABELS.to_vec();
            keys.sort_by_key(|keyword| std::cmp::Reverse(keyword.chars().count()));
            keys
        })
        .as_slice()
}

/// Remove provider info proxies from `profile`.
///
/// Returns the info lines in list order (capped, for the subscription card).
/// Matching leaf nodes are dropped, and selector groups that listed them fall
/// through to the next real member so a freshly enabled subscription does not
/// start on a quota or expiry entry. No parse warning is recorded: these
/// entries are provider metadata, not broken proxies.
pub fn detach_provider_info_nodes(profile: &mut NormalizedProfile) -> Vec<String> {
    let lines = provider_info_lines(&profile.nodes);
    let dropped: HashSet<String> = profile
        .nodes
        .iter()
        .filter(|node| is_provider_info_name(node.tag.trim()))
        .map(|node| node.tag.clone())
        .collect();
    if dropped.is_empty() {
        return lines;
    }

    profile
        .nodes
        .retain(|node| !dropped.contains(node.tag.as_str()));
    for group in &mut profile.groups {
        remove_dropped_members(group, &dropped);
        retarget_selector_default(group);
    }

    let warning_len = profile.parse_stats.warnings.len();
    crate::prune::prune_dangling_refs(profile);
    profile.parse_stats.warnings.truncate(warning_len);
    lines
}

/// Drop quota / expiry proxies from a sing-box document before it is copied.
///
/// The node list hides these entries. A share built from the original body
/// would otherwise carry them, including as a selector default.
pub(crate) fn strip_provider_info_singbox(root: &mut Value) {
    let Some(obj) = root.as_object_mut() else {
        return;
    };
    let mut dropped = HashSet::new();
    for key in ["outbounds", "endpoints"] {
        let Some(items) = obj.get(key).and_then(|value| value.as_array()) else {
            continue;
        };
        for item in items {
            let ty = item
                .get("type")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if share_group_type(ty) {
                continue;
            }
            let Some(tag) = item.get("tag").and_then(|value| value.as_str()) else {
                continue;
            };
            if is_provider_info_name(tag.trim()) {
                dropped.insert(tag.to_string());
            }
        }
    }
    if dropped.is_empty() {
        return;
    }
    for key in ["outbounds", "endpoints"] {
        let Some(items) = obj.get_mut(key).and_then(|value| value.as_array_mut()) else {
            continue;
        };
        items.retain(|item| {
            let tag = item
                .get("tag")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            !dropped.contains(tag)
        });
        for item in items.iter_mut() {
            if !share_group_type(
                item.get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or(""),
            ) {
                continue;
            }
            if let Some(members) = item
                .get_mut("outbounds")
                .and_then(|value| value.as_array_mut())
            {
                members.retain(|member| member.as_str().is_none_or(|tag| !dropped.contains(tag)));
            }
            if item.get("type").and_then(|value| value.as_str()) == Some("selector") {
                retarget_json_selector(item);
            }
        }
    }
    if let Some(route) = obj.get_mut("route") {
        if let Some(final_tag) = route.get("final").and_then(|value| value.as_str()) {
            if dropped.contains(final_tag) {
                route["final"] = json!("direct");
            }
        }
        if let Some(rules) = route
            .get_mut("rules")
            .and_then(|value| value.as_array_mut())
        {
            for rule in rules {
                rewrite_dropped_outbound(rule, &dropped);
            }
        }
    }
}

fn share_group_type(ty: &str) -> bool {
    matches!(
        ty,
        "selector" | "urltest" | "fallback" | "loadbalance" | "load-balance"
    )
}

fn retarget_json_selector(outbound: &mut Value) {
    let Some(obj) = outbound.as_object_mut() else {
        return;
    };
    let members: Vec<String> = obj
        .get("outbounds")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let current_ok = obj
        .get("default")
        .and_then(|value| value.as_str())
        .is_some_and(|tag| members.iter().any(|member| member == tag));
    if current_ok {
        return;
    }
    let Some(first) = members.first().cloned() else {
        return;
    };
    obj.insert("default".into(), json!(first));
}

fn rewrite_dropped_outbound(rule: &mut Value, dropped: &HashSet<String>) {
    if rule.get("type").and_then(|value| value.as_str()) == Some("logical") {
        if let Some(nested) = rule.get_mut("rules").and_then(|value| value.as_array_mut()) {
            for child in nested {
                rewrite_dropped_outbound(child, dropped);
            }
        }
    }
    let points_at_info = rule
        .get("outbound")
        .and_then(|value| value.as_str())
        .is_some_and(|tag| dropped.contains(tag));
    if points_at_info {
        rule["outbound"] = json!("direct");
    }
}

fn remove_dropped_members(group: &mut NormalizedOutbound, dropped: &HashSet<String>) {
    let Some(members) = group
        .outbound_mut()
        .get_mut("outbounds")
        .and_then(|value| value.as_array_mut())
    else {
        return;
    };
    members.retain(|member| member.as_str().is_none_or(|tag| !dropped.contains(tag)));
}

/// Point a selector at its first remaining member when the current default was
/// an info entry (or is otherwise no longer listed).
fn retarget_selector_default(group: &mut NormalizedOutbound) {
    let outbound = group.outbound_mut();
    if outbound.get("type").and_then(|value| value.as_str()) != Some("selector") {
        return;
    }
    let members: Vec<String> = outbound
        .get("outbounds")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let current_ok = outbound
        .get("default")
        .and_then(|value| value.as_str())
        .is_some_and(|tag| members.iter().any(|member| member == tag));
    if current_ok {
        return;
    }
    let Some(first) = members.first().cloned() else {
        return;
    };
    if let Some(object) = outbound.as_object_mut() {
        object.insert("default".into(), json!(first));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_fields() {
        let info = parse_userinfo_header(
            "upload=7118713; download=383477544; total=1099511627776; expire=1790000000",
        )
        .expect("header with counters");
        assert_eq!(info.upload, 7_118_713);
        assert_eq!(info.download, 383_477_544);
        assert_eq!(info.total, 1_099_511_627_776);
        assert_eq!(info.expire, Some(1_790_000_000));
        assert_eq!(info.used(), 390_596_257);
        assert_eq!(info.remaining(), Some(1_099_511_627_776 - 390_596_257));
    }

    #[test]
    fn empty_expire_means_no_expiry() {
        let info = parse_userinfo_header("upload=1; download=2; total=3; expire=")
            .expect("counters are usable");
        assert_eq!(info.expire, None);
    }

    #[test]
    fn zero_expire_means_no_expiry() {
        let info = parse_userinfo_header("upload=1; total=0; expire=0").expect("upload=1");
        assert_eq!(info.expire, None);
        assert_eq!(info.total, 0);
        assert_eq!(
            info.remaining(),
            None,
            "total=0 is unlimited, not a 0 quota"
        );
    }

    #[test]
    fn tolerates_spacing_case_and_unknown_keys() {
        let info =
            parse_userinfo_header(" Upload = 10 ;foo=bar; DOWNLOAD=20 ;total = 30 ;expire=40")
                .expect("pairs");
        assert_eq!(info.upload, 10);
        assert_eq!(info.download, 20);
        assert_eq!(info.total, 30);
        assert_eq!(info.expire, Some(40));
    }

    #[test]
    fn malformed_values_are_ignored() {
        let info = parse_userinfo_header("upload=abc; download=7; total=-5; expire=soon")
            .expect("download is usable");
        assert_eq!(info.upload, 0);
        assert_eq!(info.download, 7);
        assert_eq!(info.total, 0);
        assert_eq!(info.expire, None);
    }

    #[test]
    fn unusable_headers_are_none() {
        assert!(parse_userinfo_header("").is_none());
        assert!(parse_userinfo_header("upload=abc; download=xyz").is_none());
        assert!(
            parse_userinfo_header("expire=0").is_none(),
            "expire=0 carries no usable field"
        );
    }

    #[test]
    fn millisecond_expire_is_normalized() {
        let info = parse_userinfo_header("expire=1790000000000").expect("expire in ms");
        assert_eq!(info.expire, Some(1_790_000_000));
    }

    fn node(tag: &str) -> NormalizedOutbound {
        NormalizedOutbound {
            tag: tag.into(),
            outbound: std::sync::Arc::new(serde_json::json!({
                "type": "trojan",
                "tag": tag,
                "server": "example.com",
                "server_port": 443,
            })),
        }
    }

    #[test]
    fn collects_embedded_info_entries_in_order() {
        let nodes = vec![
            node("Traffic: 11.84 GB | 150 GB"),
            node("Expire: 2026-09-26"),
            node("🇭🇰 香港实验性 IEPL 专线 1"),
            node("剩余流量：1023.64 GB"),
            node("套餐到期：长期有效"),
        ];
        assert_eq!(
            provider_info_lines(&nodes),
            vec![
                "Traffic: 11.84 GB | 150 GB",
                "Expire: 2026-09-26",
                "剩余流量：1023.64 GB",
                "套餐到期：长期有效",
            ]
        );
    }

    #[test]
    fn keeps_ordinary_node_names_out_of_provider_info() {
        let nodes = vec![
            node("🇺🇸美国01|流媒体|0.1x"),
            node("美国-流量专线 1"),
            node("🇯🇵 日本标准 IEPL 专线 1"),
            node("Expire-Me-Not"),
            node("香港-流量专线|IEPL"),
            node("Unused: HK-01"),
            node("Unofficial: backup"),
        ];
        assert!(provider_info_lines(&nodes).is_empty());
    }

    #[test]
    fn compound_info_labels_match_without_substring_false_positives() {
        assert!(is_provider_info_name("套餐流量：100 GB"));
        assert!(is_provider_info_name("套餐到期：长期有效"));
        assert!(is_provider_info_name("到期时间：2026-01-01"));
        assert!(is_provider_info_name("Expired: 2026-01-01"));
        assert!(is_provider_info_name("距离下次重置剩余：19 天"));
        assert!(!is_provider_info_name("香港-流量专线|IEPL"));
        assert!(!is_provider_info_name("Unused: HK-01"));
        assert!(!is_provider_info_name("Unofficial: backup"));
    }

    #[test]
    fn provider_info_is_deduped_and_capped() {
        let nodes: Vec<_> = (0..MAX_INFO_LINES + 2)
            .map(|i| node(&format!("剩余流量：{i} GB")))
            .chain(std::iter::once(node("剩余流量：0 GB")))
            .collect();
        let lines = provider_info_lines(&nodes);
        assert_eq!(lines.len(), MAX_INFO_LINES);
        assert_eq!(lines[0], "剩余流量：0 GB");
        let long = node(&format!("Traffic: {}", "9".repeat(MAX_INFO_LINE_CHARS)));
        assert!(provider_info_lines(&[long]).is_empty());
    }

    fn profile_with(nodes: Vec<NormalizedOutbound>) -> NormalizedProfile {
        NormalizedProfile::from_nodes_only(nodes)
    }

    #[test]
    fn detach_drops_info_nodes_and_skips_them_as_the_default() {
        let mut profile = profile_with(vec![
            node("剩余流量：1023.64 GB"),
            node("套餐到期：长期有效"),
            node("🇭🇰 香港 1"),
        ]);
        let lines = detach_provider_info_nodes(&mut profile);
        assert_eq!(lines, vec!["剩余流量：1023.64 GB", "套餐到期：长期有效"]);
        assert_eq!(
            profile
                .nodes
                .iter()
                .map(|n| n.tag.as_str())
                .collect::<Vec<_>>(),
            ["🇭🇰 香港 1"]
        );
        assert_eq!(profile.default_outbound.as_deref(), Some("🇭🇰 香港 1"));
        assert!(profile.parse_stats.warnings.is_empty());
        assert_eq!(
            crate::merge::resolve_selected_tag(Some("剩余流量：1023.64 GB"), &profile).as_deref(),
            Some("🇭🇰 香港 1")
        );
    }

    #[test]
    fn detach_retargets_selector_default_past_info_members() {
        let mut profile = profile_with(vec![
            node("剩余流量：1023.64 GB"),
            node("套餐到期：长期有效"),
            node("🇭🇰 香港 1"),
            node("🇯🇵 日本 1"),
        ]);
        profile.groups.push(NormalizedOutbound::new(
            "Proxies",
            json!({
                "type": "selector",
                "tag": "Proxies",
                "outbounds": [
                    "剩余流量：1023.64 GB",
                    "套餐到期：长期有效",
                    "🇭🇰 香港 1",
                    "🇯🇵 日本 1",
                ],
                "default": "剩余流量：1023.64 GB",
            }),
        ));
        profile.default_outbound = Some("Proxies".into());
        detach_provider_info_nodes(&mut profile);
        assert_eq!(
            profile.groups[0].outbound["outbounds"],
            json!(["🇭🇰 香港 1", "🇯🇵 日本 1"])
        );
        assert_eq!(profile.groups[0].outbound["default"], "🇭🇰 香港 1");
        assert_eq!(profile.default_outbound.as_deref(), Some("Proxies"));
    }

    #[test]
    fn strip_singbox_document_drops_info_nodes_and_retargets_the_selector() {
        let mut doc = json!({
            "outbounds": [
                {"type": "socks", "tag": "剩余流量：1 GB", "server": "1.1.1.1", "server_port": 1},
                {"type": "socks", "tag": "套餐到期：2026-01-01", "server": "1.1.1.1", "server_port": 1},
                {"type": "socks", "tag": "香港-流量专线|IEPL", "server": "1.1.1.1", "server_port": 1},
                {"type": "socks", "tag": "hk", "server": "1.1.1.1", "server_port": 1},
                {
                    "type": "selector",
                    "tag": "proxy",
                    "outbounds": ["剩余流量：1 GB", "套餐到期：2026-01-01", "香港-流量专线|IEPL", "hk"],
                    "default": "剩余流量：1 GB"
                }
            ],
            "route": {
                "final": "剩余流量：1 GB",
                "rules": [{"domain": ["example.com"], "outbound": "套餐到期：2026-01-01"}]
            }
        });
        strip_provider_info_singbox(&mut doc);
        let tags: Vec<&str> = doc["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|outbound| outbound["tag"].as_str())
            .collect();
        assert_eq!(tags, ["香港-流量专线|IEPL", "hk", "proxy"]);
        assert_eq!(
            doc["outbounds"][2]["outbounds"],
            json!(["香港-流量专线|IEPL", "hk"])
        );
        assert_eq!(doc["outbounds"][2]["default"], "香港-流量专线|IEPL");
        assert_eq!(doc["route"]["final"], "direct");
        assert_eq!(doc["route"]["rules"][0]["outbound"], "direct");
    }

    #[test]
    fn detach_keeps_an_explicit_real_selector_default() {
        let mut profile = profile_with(vec![
            node("Expire: 2026-09-26"),
            node("🇭🇰 香港 1"),
            node("🇯🇵 日本 1"),
        ]);
        profile.groups.push(NormalizedOutbound::new(
            "Proxies",
            json!({
                "type": "selector",
                "tag": "Proxies",
                "outbounds": ["Expire: 2026-09-26", "🇭🇰 香港 1", "🇯🇵 日本 1"],
                "default": "🇯🇵 日本 1",
            }),
        ));
        detach_provider_info_nodes(&mut profile);
        assert_eq!(
            profile.groups[0].outbound["outbounds"],
            json!(["🇭🇰 香港 1", "🇯🇵 日本 1"])
        );
        assert_eq!(profile.groups[0].outbound["default"], "🇯🇵 日本 1");
    }
}
