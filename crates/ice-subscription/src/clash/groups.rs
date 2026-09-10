// SPDX-License-Identifier: GPL-3.0-or-later

//! Clash `proxy-groups` → sing-box selector / urltest / fallback / loadbalance outbounds.

use std::collections::HashSet;

use ice_config::{NormalizedOutbound, UiMessage};
use serde_json::{json, Value};

use super::names::resolve_member;
use crate::limits::Limits;

#[derive(Debug, Clone)]
pub struct GroupParseResult {
    pub groups: Vec<NormalizedOutbound>,
    pub skipped: usize,
    pub warnings: Vec<UiMessage>,
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

    let max_groups = Limits::default().max_groups;
    if items.len() > max_groups {
        warnings.push(Limits::warning("groups", items.len() - max_groups));
    }

    for (idx, group) in items.iter().enumerate().take(max_groups) {
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
    warnings: &mut Vec<UiMessage>,
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
                        warnings.push(
                            UiMessage::new("parse.groupUnknownMember")
                                .with("name", name.clone())
                                .with("member", m),
                        );
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    if members.is_empty() {
        warnings.push(UiMessage::new("parse.groupNoMembers").with("name", name.clone()));
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
            if !ice_config_guard::health_check_url_is_allowed(url) {
                warnings.push(
                    UiMessage::new("parse.groupRestrictedUrl")
                        .with("name", name)
                        .with("url", url),
                );
                return None;
            }
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
            if !ice_config_guard::health_check_url_is_allowed(url) {
                warnings.push(
                    UiMessage::new("parse.groupRestrictedUrl")
                        .with("name", name)
                        .with("url", url),
                );
                return None;
            }
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
            warnings.push(
                UiMessage::new("parse.groupUnsupportedType")
                    .with("name", name)
                    .with("type", other),
            );
            return None;
        }
    };

    Some(NormalizedOutbound::new(name, outbound))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn urltest_and_fallback_drop_restricted_health_urls() {
        let mut known = HashSet::new();
        known.insert("n".into());
        let doc = json!({
            "proxy-groups": [
                {
                    "name": "auto",
                    "type": "url-test",
                    "proxies": ["n"],
                    "url": "http://169.254.169.254/latest/meta-data"
                },
                {
                    "name": "ok",
                    "type": "url-test",
                    "proxies": ["n"],
                    "url": "http://www.gstatic.com/generate_204"
                },
                {
                    "name": "fb",
                    "type": "fallback",
                    "proxies": ["n"],
                    "url": "http://127.0.0.1/"
                }
            ]
        });
        let result = parse_groups(&doc, &known);
        let tags: Vec<&str> = result.groups.iter().map(|g| g.tag.as_str()).collect();
        assert_eq!(tags, ["ok"]);
        assert_eq!(result.skipped, 2);
        assert!(result
            .warnings
            .iter()
            .any(|w| w.key == "parse.groupRestrictedUrl"));
    }
}
