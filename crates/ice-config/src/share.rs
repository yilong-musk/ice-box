// SPDX-License-Identifier: GPL-3.0-or-later

//! Portable sing-box documents for subscription copy and export.
//!
//! The copy keeps proxies, groups, WireGuard endpoints, DNS, route rules, and
//! remote rule-sets. Inbounds, logs, the Clash API, and on-disk rule-set files
//! stay on this machine. Legacy `geoip` / `geosite` matchers are rewritten to
//! remote rule-sets because sing-box 1.13 rejects those fields.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::build::{
    dns_block_is_usable, dns_has_local_server, ensure_builtin_outbound, rule_set_refs_are_known,
    split_wireguard_endpoints, validate_route_refs, wireguard_endpoint,
};
use crate::runtime_config::tagged_outbound;
use crate::{
    ConfigError, NormalizedOutbound, NormalizedProfile, NormalizedRoute, ProfileParseStats,
};

const GEOIP_RULE_SET_BASE: &str = "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set";
const GEOSITE_RULE_SET_BASE: &str =
    "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set";

/// Action / DNS keys that are not match conditions.
const RULE_META_KEYS: &[&str] = &[
    "outbound",
    "action",
    "server",
    "disable_cache",
    "rewrite_ttl",
    "client_subnet",
    "method",
    "no_drop",
    "timeout",
    "override_address",
    "override_port",
    "udp_disable_domain_unmapping",
    "udp_timeout",
    "tls_fragment",
    "tls_fragment_fallback_delay",
    "tls_record_fragment",
    "invert",
];

/// Portable sing-box document for one subscription profile.
///
/// Outbounds, WireGuard endpoints, route, remote rule-sets, and DNS travel
/// with the copy. Local inbounds, the Clash API, and log paths stay behind.
/// Credentials are left intact so the copy can be imported elsewhere.
pub fn share_singbox_config(profile: &NormalizedProfile) -> Result<String, ConfigError> {
    if !profile.nodes.iter().any(share_node_is_proxy) {
        return Err(ConfigError::EmptyOutbounds);
    }

    let mut tag_set: HashSet<String> = HashSet::new();
    let mut outbounds: Vec<Arc<Value>> = Vec::new();
    for node in &profile.nodes {
        if node.outbound.get("type").and_then(|v| v.as_str()) == Some("wireguard")
            && wireguard_endpoint(&node.outbound).is_err()
        {
            continue;
        }
        push_unique_outbound(
            &mut outbounds,
            &mut tag_set,
            tagged_outbound(&node.outbound, &node.tag),
        );
    }
    for group in &profile.groups {
        push_unique_outbound(
            &mut outbounds,
            &mut tag_set,
            tagged_outbound(&group.outbound, &group.tag),
        );
    }
    // Groups often list `direct` / `block` before those builtins are copied
    // onto a flat profile. Insert them before member trimming.
    ensure_builtin_outbound(
        &mut outbounds,
        &mut tag_set,
        "direct",
        json!({"type": "direct", "tag": "direct"}),
    );
    ensure_builtin_outbound(
        &mut outbounds,
        &mut tag_set,
        "block",
        json!({"type": "block", "tag": "block"}),
    );
    trim_share_groups(&mut outbounds, &mut tag_set);
    if !outbounds
        .iter()
        .any(|outbound| share_value_is_proxy(outbound))
    {
        return Err(ConfigError::EmptyOutbounds);
    }

    // A flat node list gets a selector. `proxy` is the usual tag; a node that
    // already uses it keeps that name and the selector takes the next free one.
    let has_group = outbounds
        .iter()
        .any(|outbound| share_type_is_group(outbound_type(outbound)));
    let selector_tag = if has_group {
        None
    } else {
        let tag = fresh_share_tag(&tag_set, "proxy");
        let node_tags: Vec<String> = outbounds
            .iter()
            .filter(|outbound| share_value_is_proxy(outbound))
            .filter_map(|outbound| {
                outbound
                    .get("tag")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .collect();
        if node_tags.is_empty() {
            return Err(ConfigError::EmptyOutbounds);
        }
        let proxy_default = node_tags[0].clone();
        outbounds.push(Arc::new(json!({
            "type": "selector",
            "tag": tag.clone(),
            "outbounds": node_tags,
            "default": proxy_default,
        })));
        tag_set.insert(tag.clone());
        Some(tag)
    };

    let final_outbound = {
        let candidate = profile.route.final_outbound.as_str();
        if candidate == "direct" || candidate == "block" || tag_set.contains(candidate) {
            candidate.to_string()
        } else if let Some(tag) = selector_tag {
            tag
        } else if tag_set.contains("proxy") {
            "proxy".to_string()
        } else {
            "direct".to_string()
        }
    };

    let mut rule_sets = Vec::new();
    let mut set_tags = HashSet::new();
    for set in &profile.route.rule_sets {
        push_portable_rule_set(set, &mut rule_sets, &mut set_tags);
    }
    let rules: Vec<Value> = profile
        .route
        .rules
        .iter()
        .flat_map(|rule| expand_share_rules(rule, &tag_set, &mut rule_sets, &mut set_tags))
        .collect();
    scrub_rule_set_detours(&mut rule_sets, &tag_set);

    let mut route = json!({
        "final": final_outbound,
        "auto_detect_interface": true,
    });
    if !rules.is_empty() {
        route
            .as_object_mut()
            .expect("route object")
            .insert("rules".into(), Value::Array(rules));
    }
    if !rule_sets.is_empty() {
        route
            .as_object_mut()
            .expect("route object")
            .insert("rule_set".into(), Value::Array(rule_sets));
    }

    let dns = if let Some(raw) = &profile.dns {
        let mut dns = raw.clone();
        if prepare_share_dns(&mut dns, &tag_set, &set_tags) && dns_block_is_usable(&dns) {
            if dns_has_local_server(&dns) {
                route
                    .as_object_mut()
                    .expect("route object")
                    .insert("default_domain_resolver".into(), json!("local"));
            }
            Some(dns)
        } else {
            None
        }
    } else {
        None
    };

    validate_route_refs(&route, &tag_set)?;
    let endpoints = split_wireguard_endpoints(&mut outbounds)?;

    let mut root = Map::new();
    if let Some(dns) = dns {
        root.insert("dns".into(), dns);
    }
    root.insert(
        "outbounds".into(),
        Value::Array(
            outbounds
                .into_iter()
                .map(|item| item.as_ref().clone())
                .collect(),
        ),
    );
    if !endpoints.is_empty() {
        root.insert("endpoints".into(), Value::Array(endpoints));
    }
    root.insert("route".into(), route);
    Ok(serde_json::to_string_pretty(&Value::Object(root))?)
}

/// Portable document from an original sing-box subscription body.
///
/// Parsing a subscription drops remote rule-sets, DNS outbounds, and an
/// `endpoints` array that sits beside `outbounds`. The raw body still has
/// them, so copy and export start here when the subscription is sing-box JSON.
pub fn share_singbox_document(raw: &str) -> Result<String, ConfigError> {
    let raw = raw.trim().trim_start_matches('\u{feff}');
    let value: Value = serde_json::from_str(raw)
        .map_err(|err| ConfigError::invalid(format!("sing-box json: {err}")))?;
    let root = value
        .as_object()
        .ok_or_else(|| ConfigError::invalid("sing-box config must be an object"))?;

    let mut tags = HashSet::new();
    let mut nodes = Vec::new();
    let mut groups = Vec::new();
    if let Some(items) = root.get("outbounds").and_then(|v| v.as_array()) {
        for item in items {
            push_share_item(item, &mut nodes, &mut groups, &mut tags);
        }
    }
    if let Some(items) = root.get("endpoints").and_then(|v| v.as_array()) {
        for item in items {
            push_share_item(item, &mut nodes, &mut groups, &mut tags);
        }
    }
    trim_profile_groups(&mut groups, &mut tags);
    if !nodes.iter().any(share_node_is_proxy) {
        return Err(ConfigError::EmptyOutbounds);
    }

    let route_value = root.get("route");
    let rules = route_value
        .and_then(|route| route.get("rules"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let rule_sets = route_value
        .and_then(|route| route.get("rule_set"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let final_outbound = route_value
        .and_then(|route| route.get("final"))
        .and_then(|v| v.as_str())
        .unwrap_or("direct")
        .to_string();

    let profile = NormalizedProfile {
        nodes,
        groups,
        route: NormalizedRoute {
            rules,
            final_outbound,
            rule_sets,
        },
        dns: root.get("dns").filter(|dns| dns.is_object()).cloned(),
        default_outbound: None,
        parse_stats: ProfileParseStats::default(),
    };
    let rendered = share_singbox_config(&profile)?;
    let mut doc: Value = serde_json::from_str(&rendered)?;
    apply_share_route_extras(&mut doc, route_value);
    Ok(serde_json::to_string_pretty(&doc)?)
}

fn push_share_item(
    item: &Value,
    nodes: &mut Vec<NormalizedOutbound>,
    groups: &mut Vec<NormalizedOutbound>,
    tags: &mut HashSet<String>,
) {
    if ice_config_guard::check_outbound(item).is_err() {
        return;
    }
    let ty = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let Some(tag) = item
        .get("tag")
        .and_then(|v| v.as_str())
        .filter(|tag| share_tag_ok(tag))
    else {
        return;
    };
    if !tags.insert(tag.to_string()) {
        // The endpoints array is the sing-box 1.13 WireGuard form. Prefer it
        // when an outbound of the same tag was already recorded.
        if ty == "wireguard" {
            if let Some(node) = nodes.iter_mut().find(|node| node.tag == tag) {
                *node = NormalizedOutbound::new(tag, item.clone());
            }
        }
        return;
    }
    let entry = NormalizedOutbound::new(tag, item.clone());
    if share_type_is_group(ty) {
        groups.push(entry);
    } else {
        nodes.push(entry);
    }
}

fn trim_profile_groups(groups: &mut Vec<NormalizedOutbound>, tags: &mut HashSet<String>) {
    loop {
        let before = groups.len();
        for group in groups.iter_mut() {
            let Some(members) = group
                .outbound_mut()
                .get_mut("outbounds")
                .and_then(|value| value.as_array_mut())
            else {
                continue;
            };
            members.retain(|member| member.as_str().is_some_and(|tag| tags.contains(tag)));
        }
        groups.retain(|group| {
            let empty = group
                .outbound
                .get("outbounds")
                .and_then(|value| value.as_array())
                .is_none_or(|members| members.is_empty());
            if empty {
                tags.remove(&group.tag);
                false
            } else {
                true
            }
        });
        if groups.len() == before {
            break;
        }
    }
}

fn apply_share_route_extras(doc: &mut Value, route: Option<&Value>) {
    let Some(route) = route.and_then(|value| value.as_object()) else {
        return;
    };
    let auto_detect = route.get("auto_detect_interface").and_then(|v| v.as_bool());
    let extras: Vec<(String, Value)> = [
        "find_process",
        "override_android_vpn",
        "default_network_strategy",
        "default_network_type",
    ]
    .into_iter()
    .filter_map(|key| {
        route
            .get(key)
            .filter(|value| portable_route_extra(value))
            .map(|value| (key.to_string(), value.clone()))
    })
    .collect();
    let resolver = route
        .get("default_domain_resolver")
        .and_then(|v| v.as_str())
        .filter(|tag| share_tag_ok(tag))
        .map(str::to_string)
        .filter(|tag| {
            doc.get("dns")
                .and_then(|dns| dns.get("servers"))
                .and_then(|v| v.as_array())
                .is_some_and(|servers| {
                    servers
                        .iter()
                        .any(|server| server.get("tag").and_then(|v| v.as_str()) == Some(tag))
                })
        });
    let Some(dst) = doc.get_mut("route").and_then(|value| value.as_object_mut()) else {
        return;
    };
    if let Some(flag) = auto_detect {
        dst.insert("auto_detect_interface".into(), json!(flag));
    }
    for (key, value) in extras {
        dst.insert(key, value);
    }
    if let Some(tag) = resolver {
        dst.insert("default_domain_resolver".into(), json!(tag));
    }
}

fn portable_route_extra(value: &Value) -> bool {
    match value {
        Value::Bool(_) => true,
        Value::String(text) => {
            !text.is_empty()
                && text.len() <= 64
                && text
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        }
        Value::Array(items) => {
            items.len() <= 8
                && items
                    .iter()
                    .all(|item| item.is_string() && portable_route_extra(item))
        }
        _ => false,
    }
}

fn push_unique_outbound(
    outbounds: &mut Vec<Arc<Value>>,
    tags: &mut HashSet<String>,
    outbound: Arc<Value>,
) {
    let Some(tag) = outbound.get("tag").and_then(|v| v.as_str()) else {
        return;
    };
    if tags.insert(tag.to_string()) {
        outbounds.push(outbound);
    }
}

fn trim_share_groups(outbounds: &mut Vec<Arc<Value>>, tags: &mut HashSet<String>) {
    loop {
        let before = outbounds.len();
        for outbound in outbounds.iter_mut() {
            if !share_type_is_group(outbound_type(outbound)) {
                continue;
            }
            let mut cloned = (**outbound).clone();
            if let Some(members) = cloned.get_mut("outbounds").and_then(|v| v.as_array_mut()) {
                members.retain(|member| member.as_str().is_some_and(|tag| tags.contains(tag)));
            }
            *outbound = Arc::new(cloned);
        }
        outbounds.retain(|outbound| {
            if !share_type_is_group(outbound_type(outbound)) {
                return true;
            }
            let empty = outbound
                .get("outbounds")
                .and_then(|value| value.as_array())
                .is_none_or(|members| members.is_empty());
            if empty {
                if let Some(tag) = outbound.get("tag").and_then(|v| v.as_str()) {
                    tags.remove(tag);
                }
                false
            } else {
                true
            }
        });
        if outbounds.len() == before {
            break;
        }
    }
}

fn share_node_is_proxy(node: &NormalizedOutbound) -> bool {
    share_value_is_proxy(&node.outbound)
}

fn share_value_is_proxy(value: &Value) -> bool {
    let ty = outbound_type(value);
    !ty.is_empty() && !share_type_is_builtin(ty) && !share_type_is_group(ty)
}

fn outbound_type(value: &Value) -> &str {
    value.get("type").and_then(|v| v.as_str()).unwrap_or("")
}

fn share_type_is_builtin(ty: &str) -> bool {
    matches!(ty, "direct" | "block" | "dns")
}

fn share_type_is_group(ty: &str) -> bool {
    matches!(
        ty,
        "selector" | "urltest" | "fallback" | "loadbalance" | "load-balance"
    )
}

fn fresh_share_tag(tags: &HashSet<String>, preferred: &str) -> String {
    if !tags.contains(preferred) {
        return preferred.to_string();
    }
    for n in 2..u32::MAX {
        let candidate = format!("{preferred}-{n}");
        if !tags.contains(&candidate) {
            return candidate;
        }
    }
    format!("{preferred}-fallback")
}

fn push_portable_rule_set(set: &Value, sets: &mut Vec<Value>, tags: &mut HashSet<String>) {
    let Some(portable) = portable_rule_set(set) else {
        return;
    };
    let Some(tag) = portable.get("tag").and_then(|v| v.as_str()) else {
        return;
    };
    if tags.insert(tag.to_string()) {
        sets.push(portable);
    }
}

fn portable_rule_set(set: &Value) -> Option<Value> {
    let obj = set.as_object()?;
    if obj.get("type").and_then(|v| v.as_str()) == Some("local") {
        return None;
    }
    let tag = obj.get("tag").and_then(|v| v.as_str()).map(str::trim)?;
    if !share_tag_ok(tag) {
        return None;
    }
    let mut copy = set.clone();
    let map = copy.as_object_mut()?;
    map.remove("path");
    for key in ["url", "download_url"] {
        let ok = map
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|url| is_share_http_url(url.trim()));
        if map.contains_key(key) && !ok {
            map.remove(key);
        }
    }
    let url = map
        .get("url")
        .or_else(|| map.get("download_url"))
        .and_then(|v| v.as_str())?;
    if !is_share_http_url(url) {
        return None;
    }
    if map.get("type").and_then(|v| v.as_str()) != Some("remote") {
        map.insert("type".into(), json!("remote"));
    }
    Some(copy)
}

fn is_share_http_url(url: &str) -> bool {
    let url = url.trim();
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    if rest.is_empty() || rest.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return false;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    !host.is_empty() && !host.eq_ignore_ascii_case("localhost")
}

fn share_tag_ok(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 256
        && !tag
            .chars()
            .any(|ch| ch.is_control() || ch == '/' || ch == '\\')
}

fn scrub_rule_set_detours(sets: &mut [Value], outbound_tags: &HashSet<String>) {
    for set in sets {
        let Some(obj) = set.as_object_mut() else {
            continue;
        };
        let drop_detour = obj
            .get("download_detour")
            .and_then(|v| v.as_str())
            .is_some_and(|tag| tag != "direct" && !outbound_tags.contains(tag));
        if drop_detour {
            obj.remove("download_detour");
        }
    }
}

fn expand_share_rules(
    rule: &Value,
    outbound_tags: &HashSet<String>,
    rule_sets: &mut Vec<Value>,
    set_tags: &mut HashSet<String>,
) -> Vec<Value> {
    if !rule.is_object() || rule.get("inbound").is_some() || !share_outbound_ok(rule, outbound_tags)
    {
        return Vec::new();
    }
    if rule.get("type").and_then(|v| v.as_str()) == Some("logical") {
        return expand_logical_share_rule(rule, outbound_tags, rule_sets, set_tags);
    }
    rewrite_geo_share_rule(rule, rule_sets, set_tags)
}

fn expand_logical_share_rule(
    rule: &Value,
    outbound_tags: &HashSet<String>,
    rule_sets: &mut Vec<Value>,
    set_tags: &mut HashSet<String>,
) -> Vec<Value> {
    let mode = rule.get("mode").and_then(|v| v.as_str()).unwrap_or("and");
    let Some(children) = rule.get("rules").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let expanded: Vec<Vec<Value>> = children
        .iter()
        .map(|child| expand_share_rules(child, outbound_tags, rule_sets, set_tags))
        .collect();
    if mode == "or" {
        let kept: Vec<Value> = expanded.into_iter().flatten().collect();
        if kept.is_empty() {
            return Vec::new();
        }
        return vec![rebuild_logical(rule, "or", kept)];
    }
    if expanded.iter().any(|child| child.len() != 1) {
        return Vec::new();
    }
    let kept: Vec<Value> = expanded
        .into_iter()
        .filter_map(|mut child| child.pop())
        .collect();
    if kept.is_empty() {
        return Vec::new();
    }
    vec![rebuild_logical(rule, "and", kept)]
}

fn rebuild_logical(parent: &Value, mode: &str, children: Vec<Value>) -> Value {
    let mut obj = Map::new();
    if let Some(parent_obj) = parent.as_object() {
        for (key, value) in parent_obj {
            if share_rule_meta_key(key) {
                obj.insert(key.clone(), value.clone());
            }
        }
    }
    obj.insert("type".into(), json!("logical"));
    obj.insert("mode".into(), json!(mode));
    obj.insert("rules".into(), Value::Array(children));
    Value::Object(obj)
}

fn rewrite_geo_share_rule(
    rule: &Value,
    rule_sets: &mut Vec<Value>,
    set_tags: &mut HashSet<String>,
) -> Vec<Value> {
    if rule.get("geoip").is_none() && rule.get("geosite").is_none() {
        return if share_rule_sets_known(rule, set_tags) {
            vec![rule.clone()]
        } else {
            Vec::new()
        };
    }

    let mut base = rule.clone();
    let Some(obj) = base.as_object_mut() else {
        return Vec::new();
    };
    let geoip = obj.remove("geoip");
    let geosite = obj.remove("geosite");
    if !share_rule_sets_known(&base, set_tags) {
        return Vec::new();
    }

    let mut private = false;
    let mut geoip_tags = Vec::new();
    if let Some(geoip) = geoip.as_ref() {
        for code in geo_codes(geoip) {
            if code == "private" || code == "lan" {
                private = true;
            } else if let Some(tag) = ensure_share_geo_set("geoip", &code, rule_sets, set_tags) {
                geoip_tags.push(tag);
            }
        }
    }
    let mut geosite_tags = Vec::new();
    if let Some(geosite) = geosite.as_ref() {
        for code in geo_codes(geosite) {
            if let Some(tag) = ensure_share_geo_set("geosite", &code, rule_sets, set_tags) {
                geosite_tags.push(tag);
            }
        }
    }

    if !private && geoip_tags.is_empty() && geosite_tags.is_empty() {
        return if share_rule_has_matcher(&base) {
            vec![base]
        } else {
            Vec::new()
        };
    }

    let mut outputs = Vec::new();
    if private {
        let mut private_rule = base.clone();
        private_rule["ip_is_private"] = json!(true);
        outputs.push(and_rule_groups(
            private_rule,
            geosite_tag_groups(&geosite_tags),
        ));
    }
    if !geoip_tags.is_empty() {
        let mut groups = vec![geoip_tags];
        if !geosite_tags.is_empty() {
            groups.push(geosite_tags);
        }
        outputs.push(and_rule_groups(base, groups));
    } else if !private && !geosite_tags.is_empty() {
        outputs.push(and_rule_groups(base, vec![geosite_tags]));
    }
    outputs
        .into_iter()
        .filter(|rule| share_rule_sets_known(rule, set_tags) && share_rule_has_matcher(rule))
        .collect()
}

fn geosite_tag_groups(tags: &[String]) -> Vec<Vec<String>> {
    if tags.is_empty() {
        Vec::new()
    } else {
        vec![tags.to_vec()]
    }
}

fn and_rule_groups(base: Value, groups: Vec<Vec<String>>) -> Value {
    let Some(obj) = base.as_object() else {
        return base;
    };
    let mut meta = Map::new();
    let mut matchers = Map::new();
    for (key, value) in obj {
        if share_rule_meta_key(key) {
            meta.insert(key.clone(), value.clone());
        } else {
            matchers.insert(key.clone(), value.clone());
        }
    }
    let existing = matchers
        .remove("rule_set")
        .map(|value| rule_set_list(&value))
        .unwrap_or_default();
    let mut children = Vec::new();
    if !matchers.is_empty() {
        children.push(Value::Object(matchers));
    }
    if !existing.is_empty() {
        children.push(json!({ "rule_set": rule_set_value(&existing) }));
    }
    for group in groups {
        if !group.is_empty() {
            children.push(json!({ "rule_set": rule_set_value(&group) }));
        }
    }
    if children.len() == 1 {
        if let Value::Object(child) = &children[0] {
            for (key, value) in child {
                meta.insert(key.clone(), value.clone());
            }
            return Value::Object(meta);
        }
    }
    if children.is_empty() {
        return Value::Object(meta);
    }
    meta.insert("type".into(), json!("logical"));
    meta.insert("mode".into(), json!("and"));
    meta.insert("rules".into(), Value::Array(children));
    Value::Object(meta)
}

fn rule_set_list(value: &Value) -> Vec<String> {
    match value {
        Value::String(tag) => vec![tag.clone()],
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn rule_set_value(tags: &[String]) -> Value {
    if tags.len() == 1 {
        Value::String(tags[0].clone())
    } else {
        Value::Array(tags.iter().cloned().map(Value::String).collect())
    }
}

fn ensure_share_geo_set(
    kind: &str,
    code: &str,
    rule_sets: &mut Vec<Value>,
    set_tags: &mut HashSet<String>,
) -> Option<String> {
    if !geo_code_ok(code) {
        return None;
    }
    let tag = format!("{kind}-{code}");
    if set_tags.contains(&tag) {
        return Some(tag);
    }
    let base = match kind {
        "geoip" => GEOIP_RULE_SET_BASE,
        "geosite" => GEOSITE_RULE_SET_BASE,
        _ => return None,
    };
    rule_sets.push(json!({
        "type": "remote",
        "tag": tag,
        "format": "binary",
        "url": format!("{base}/{kind}-{code}.srs"),
    }));
    set_tags.insert(tag.clone());
    Some(tag)
}

fn geo_codes(value: &Value) -> Vec<String> {
    match value {
        Value::String(code) => vec![code.trim().to_ascii_lowercase()],
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(|code| code.trim().to_ascii_lowercase()))
            .filter(|code| !code.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn geo_code_ok(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '!'))
}

fn share_outbound_ok(rule: &Value, tags: &HashSet<String>) -> bool {
    match rule.get("outbound").and_then(|v| v.as_str()) {
        None => true,
        Some("direct" | "block") => true,
        Some(tag) => tags.contains(tag),
    }
}

fn share_rule_sets_known(rule: &Value, set_tags: &HashSet<String>) -> bool {
    let refs: HashSet<&str> = set_tags.iter().map(String::as_str).collect();
    if !rule_set_refs_are_known(rule, &refs) {
        return false;
    }
    true
}

fn share_rule_has_matcher(rule: &Value) -> bool {
    rule.as_object().is_some_and(|obj| {
        obj.keys().any(|key| !share_rule_meta_key(key))
            || obj.get("type").and_then(|v| v.as_str()) == Some("logical")
    })
}

fn share_rule_meta_key(key: &str) -> bool {
    RULE_META_KEYS.contains(&key)
}

fn prepare_share_dns(
    dns: &mut Value,
    outbound_tags: &HashSet<String>,
    set_tags: &HashSet<String>,
) -> bool {
    let Some(obj) = dns.as_object_mut() else {
        return false;
    };
    obj.remove("__ice_dns_listen");
    let mut servers = obj
        .get("servers")
        .and_then(|v| v.as_array())
        .map(|servers| {
            servers
                .iter()
                .filter(|server| dns_server_is_portable(server))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if servers.is_empty() {
        return false;
    }
    let server_tags: HashSet<String> = servers
        .iter()
        .filter_map(|server| {
            server
                .get("tag")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .collect();
    for server in &mut servers {
        let Some(map) = server.as_object_mut() else {
            continue;
        };
        let drop_resolver = map
            .get("domain_resolver")
            .and_then(|v| v.as_str())
            .is_some_and(|tag| !server_tags.contains(tag));
        if drop_resolver {
            map.remove("domain_resolver");
        }
        let drop_detour = map
            .get("detour")
            .and_then(|v| v.as_str())
            .is_some_and(|tag| tag != "direct" && !outbound_tags.contains(tag));
        if drop_detour {
            map.remove("detour");
        }
    }
    let final_tag = obj
        .get("final")
        .and_then(|v| v.as_str())
        .filter(|tag| server_tags.contains(*tag))
        .map(str::to_string)
        .or_else(|| {
            servers.iter().find_map(|server| {
                server
                    .get("tag")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
        });
    let set_refs: HashSet<&str> = set_tags.iter().map(String::as_str).collect();
    let rules = obj
        .get("rules")
        .and_then(|v| v.as_array())
        .map(|rules| {
            rules
                .iter()
                .filter(|rule| share_dns_rule_ok(rule, &server_tags, &set_refs))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    obj.insert("servers".into(), Value::Array(servers));
    if rules.is_empty() {
        obj.remove("rules");
    } else {
        obj.insert("rules".into(), Value::Array(rules));
    }
    if let Some(tag) = final_tag {
        obj.insert("final".into(), json!(tag));
    } else {
        obj.remove("final");
    }
    true
}

fn dns_server_is_portable(server: &Value) -> bool {
    let Some(obj) = server.as_object() else {
        return false;
    };
    match obj.get("type").and_then(|v| v.as_str()) {
        Some("hosts") => false,
        Some(ty) if !ice_config_guard::dns_server_type_is_allowed(ty) => false,
        Some("https" | "h3") => true,
        Some(_) => !server_has_filesystem_path(obj),
        None => {
            obj.get("address")
                .and_then(|v| v.as_str())
                .is_some_and(|address| !address.is_empty())
                && !server_has_filesystem_path(obj)
        }
    }
}

fn server_has_filesystem_path(server: &Map<String, Value>) -> bool {
    server.keys().any(|key| {
        key == "path" || key == "executable_path" || key.ends_with("_path") || key == "output"
    })
}

fn share_dns_rule_ok(rule: &Value, servers: &HashSet<String>, sets: &HashSet<&str>) -> bool {
    if rule.get("inbound").is_some() {
        return false;
    }
    if let Some(server) = rule.get("server").and_then(|v| v.as_str()) {
        if !servers.contains(server) {
            return false;
        }
    }
    rule_set_refs_are_known(rule, sets)
}
