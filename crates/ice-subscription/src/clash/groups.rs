//! Clash `proxy-groups` → sing-box selector / urltest / fallback / loadbalance outbounds.

use std::collections::HashSet;

use ice_config::NormalizedOutbound;
use serde_json::{json, Value};

use super::names::resolve_member;

pub const MAX_CLASH_GROUPS: usize = 128;

#[derive(Debug, Clone)]
pub struct GroupParseResult {
    pub groups: Vec<NormalizedOutbound>,
    pub skipped: usize,
    pub warnings: Vec<String>,
}

pub fn parse_groups(doc: &Value, known: &HashSet<String>) -> GroupParseResult {
    let mut groups = Vec::new();
    let mut skipped = 0usize;
    let mut warnings = Vec::new();

    let Some(items) = doc.get("proxy-groups").and_then(|v| v.as_array()) else {
        return GroupParseResult {
            groups,
            skipped,
            warnings,
        };
    };

    if items.len() > MAX_CLASH_GROUPS {
        warnings.push(format!(
            "proxy-groups count {} exceeds limit {MAX_CLASH_GROUPS}",
            items.len()
        ));
    }

    for (idx, group) in items.iter().enumerate().take(MAX_CLASH_GROUPS) {
        match map_group(group, idx, known, &mut warnings) {
            Some(node) => groups.push(node),
            None => skipped += 1,
        }
    }

    GroupParseResult {
        groups,
        skipped,
        warnings,
    }
}

fn map_group(
    group: &Value,
    idx: usize,
    known: &HashSet<String>,
    warnings: &mut Vec<String>,
) -> Option<NormalizedOutbound> {
    let obj = group.as_object()?;
    let name = obj
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("group-{idx}"));
    let ty = obj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("select")
        .to_ascii_lowercase();

    let members: Vec<String> = obj
        .get("proxies")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .filter_map(|m| match resolve_member(m, known) {
                    Some(tag) => Some(tag),
                    None => {
                        warnings.push(format!("group {name}: unknown member {m}"));
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    if members.is_empty() {
        warnings.push(format!("group {name}: no resolvable members"));
        return None;
    }

    // Clash `select` groups have no default member: the first listed member is the
    // initial selection (subscriptions order nodes so the first is the recommended one).
    let default = members.first().cloned();

    let outbound = match ty.as_str() {
        "select" => json!({
            "type": "selector",
            "tag": name,
            "outbounds": members,
            "default": default,
        }),
        "url-test" => {
            let url = obj
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("http://www.gstatic.com/generate_204");
            let interval = obj.get("interval").and_then(|v| v.as_u64()).unwrap_or(300);
            json!({
                "type": "urltest",
                "tag": name,
                "outbounds": members,
                "url": url,
                "interval": format!("{interval}s"),
                "tolerance": obj.get("tolerance").and_then(|v| v.as_u64()).unwrap_or(50),
            })
        }
        "fallback" => {
            let url = obj
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("http://www.gstatic.com/generate_204");
            let interval = obj.get("interval").and_then(|v| v.as_u64()).unwrap_or(300);
            json!({
                "type": "fallback",
                "tag": name,
                "outbounds": members,
                "url": url,
                "interval": format!("{interval}s"),
            })
        }
        "load-balance" => {
            let strategy = obj
                .get("strategy")
                .and_then(|v| v.as_str())
                .unwrap_or("round-robin");
            json!({
                "type": "loadbalance",
                "tag": name,
                "outbounds": members,
                "strategy": strategy,
            })
        }
        other => {
            warnings.push(format!("group {name}: unsupported type {other}"));
            return None;
        }
    };

    Some(NormalizedOutbound {
        tag: name,
        outbound,
    })
}
