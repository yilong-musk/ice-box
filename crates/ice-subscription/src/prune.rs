// SPDX-License-Identifier: GPL-3.0-or-later

//! Drop selector/urltest members, route refs, and `detour`s that point at
//! outbounds the parser skipped or truncated (SUB-2 / SEC-1).

use std::collections::HashSet;

use ice_config::{NormalizedProfile, UiMessage};
use serde_json::{json, Value};

const BUILTIN_TAGS: &[&str] = &["direct", "block", "dns", "proxy"];

/// Make `profile` self-consistent after allowlist filtering or size caps.
pub fn prune_dangling_refs(profile: &mut NormalizedProfile) {
    loop {
        let known = known_tags(profile);
        let nodes_before = profile.nodes.len();
        let groups_before = profile.groups.len();

        let mut dropped_detour = Vec::new();
        profile.nodes.retain(
            |n| match n.outbound.get("detour").and_then(|v| v.as_str()) {
                Some(d) if !known.contains(d) => {
                    dropped_detour.push(
                        UiMessage::new("parse.droppedDetour")
                            .with("tag", n.tag.clone())
                            .with("detour", d),
                    );
                    false
                }
                _ => true,
            },
        );
        profile.parse_stats.warnings.extend(dropped_detour);

        let mut empty_groups = Vec::new();
        let mut trimmed_groups = Vec::new();
        for g in &mut profile.groups {
            let tag = g.tag.clone();
            if let Some(arr) = g
                .outbound_mut()
                .get_mut("outbounds")
                .and_then(|v| v.as_array_mut())
            {
                let before = arr.len();
                arr.retain(|m| m.as_str().is_some_and(|t| known.contains(t)));
                if arr.len() != before {
                    trimmed_groups.push(
                        UiMessage::new("parse.trimmedGroup")
                            .with("tag", tag.clone())
                            .with("dropped", (before - arr.len()).to_string()),
                    );
                }
            }
        }
        profile.parse_stats.warnings.extend(trimmed_groups);
        profile.groups.retain(|g| {
            let empty = g
                .outbound
                .get("outbounds")
                .and_then(|v| v.as_array())
                .is_none_or(|a| a.is_empty());
            if empty {
                empty_groups
                    .push(UiMessage::new("parse.droppedEmptyGroup").with("tag", g.tag.clone()));
                false
            } else {
                true
            }
        });
        profile.parse_stats.warnings.extend(empty_groups);

        if profile.nodes.len() == nodes_before && profile.groups.len() == groups_before {
            break;
        }
    }

    let known = known_tags(profile);
    for rule in &mut profile.route.rules {
        if let Some(out) = rule.get("outbound").and_then(|v| v.as_str()) {
            if !known.contains(out) {
                profile.parse_stats.warnings.push(
                    UiMessage::new("parse.routeOutboundMissing")
                        .with("outbound", out)
                        .with("fallback", "direct"),
                );
                rule["outbound"] = json!("direct");
            }
        }
    }
    if !known.contains(&profile.route.final_outbound) {
        profile.parse_stats.warnings.push(
            UiMessage::new("parse.routeFinalMissing")
                .with("outbound", profile.route.final_outbound.clone())
                .with("fallback", "direct"),
        );
        profile.route.final_outbound = "direct".into();
    }
    let known_sets: HashSet<String> = profile
        .route
        .rule_sets
        .iter()
        .filter_map(|s| s.get("tag").and_then(|v| v.as_str()).map(str::to_string))
        .collect();
    let mut dropped_rule_sets = Vec::new();
    profile.route.rules.retain(|rule| {
        if rule_set_refs_resolvable(rule, &known_sets) {
            return true;
        }
        dropped_rule_sets.push(
            UiMessage::new("parse.routeRuleSetMissing")
                .with("rule_set", unknown_rule_set_label(rule, &known_sets)),
        );
        false
    });
    profile.parse_stats.warnings.extend(dropped_rule_sets);
    if let Some(tag) = &profile.default_outbound {
        if !known.contains(tag) {
            profile.default_outbound = profile
                .groups
                .first()
                .map(|g| g.tag.clone())
                .or_else(|| profile.nodes.first().map(|n| n.tag.clone()));
        }
    }
}

/// `rule_set` is one tag or a list of tags. A string used to be ignored, so a
/// remote set removed during parse left a dangling reference in the profile.
fn rule_set_refs_resolvable(rule: &Value, known_sets: &HashSet<String>) -> bool {
    match rule.get("rule_set") {
        None => {}
        Some(Value::String(tag)) => {
            if !known_sets.contains(tag) {
                return false;
            }
        }
        Some(Value::Array(items)) => {
            if !items
                .iter()
                .all(|item| item.as_str().is_some_and(|tag| known_sets.contains(tag)))
            {
                return false;
            }
        }
        Some(_) => return false,
    }
    if rule.get("type").and_then(|v| v.as_str()) == Some("logical") {
        let Some(nested) = rule.get("rules").and_then(|v| v.as_array()) else {
            return false;
        };
        return !nested.is_empty()
            && nested
                .iter()
                .all(|child| rule_set_refs_resolvable(child, known_sets));
    }
    true
}

fn unknown_rule_set_label(rule: &Value, known_sets: &HashSet<String>) -> String {
    let mut tags = Vec::new();
    collect_unknown_rule_sets(rule, known_sets, &mut tags);
    if tags.is_empty() {
        "logical".to_string()
    } else {
        tags.join(",")
    }
}

fn collect_unknown_rule_sets(rule: &Value, known_sets: &HashSet<String>, tags: &mut Vec<String>) {
    match rule.get("rule_set") {
        Some(Value::String(tag)) if !known_sets.contains(tag) => tags.push(tag.clone()),
        Some(Value::Array(items)) => {
            for item in items {
                if let Some(tag) = item.as_str() {
                    if !known_sets.contains(tag) {
                        tags.push(tag.to_string());
                    }
                }
            }
        }
        Some(_) => tags.push("invalid".to_string()),
        None => {}
    }
    if rule.get("type").and_then(|v| v.as_str()) == Some("logical") {
        if let Some(nested) = rule.get("rules").and_then(|v| v.as_array()) {
            for child in nested {
                collect_unknown_rule_sets(child, known_sets, tags);
            }
        }
    }
}

fn known_tags(profile: &NormalizedProfile) -> HashSet<String> {
    let mut known: HashSet<String> = BUILTIN_TAGS.iter().map(|s| (*s).to_string()).collect();
    for n in &profile.nodes {
        known.insert(n.tag.clone());
    }
    for g in &profile.groups {
        known.insert(g.tag.clone());
    }
    known
}

#[cfg(test)]
mod tests {
    use super::*;
    use ice_config::{NormalizedOutbound, NormalizedRoute, ProfileParseStats};
    use serde_json::json;

    fn profile(
        nodes: Vec<NormalizedOutbound>,
        groups: Vec<NormalizedOutbound>,
    ) -> NormalizedProfile {
        NormalizedProfile {
            default_outbound: groups.first().map(|g| g.tag.clone()),
            nodes,
            groups,
            route: NormalizedRoute {
                rules: vec![json!({"outbound": "gone"})],
                final_outbound: "gone".into(),
                rule_sets: Vec::new(),
            },
            dns: None,
            parse_stats: ProfileParseStats::default(),
        }
    }

    #[test]
    fn prunes_members_rules_final_and_detour() {
        let leaf = NormalizedOutbound {
            tag: "ss".into(),
            outbound: std::sync::Arc::new(json!({"type":"shadowsocks","tag":"ss","detour":"st"})),
        };
        let keep = NormalizedOutbound {
            tag: "ok".into(),
            outbound: std::sync::Arc::new(json!({"type":"shadowsocks","tag":"ok"})),
        };
        let group = NormalizedOutbound {
            tag: "proxy".into(),
            outbound: std::sync::Arc::new(
                json!({"type":"selector","tag":"proxy","outbounds":["ss","ok","gone"]}),
            ),
        };
        let mut p = profile(vec![leaf, keep], vec![group]);
        prune_dangling_refs(&mut p);
        assert_eq!(
            p.nodes.iter().map(|n| n.tag.as_str()).collect::<Vec<_>>(),
            ["ok"]
        );
        assert_eq!(p.groups.len(), 1);
        assert_eq!(p.groups[0].outbound["outbounds"], json!(["ok"]));
        assert_eq!(p.route.rules[0]["outbound"], "direct");
        assert_eq!(p.route.final_outbound, "direct");
        assert!(p
            .parse_stats
            .warnings
            .iter()
            .any(|w| w.key == "parse.droppedDetour"));
    }

    #[test]
    fn string_rule_set_refs_are_pruned_like_arrays() {
        let mut p = profile(vec![], vec![]);
        p.route.rule_sets = vec![json!({
            "type": "local",
            "tag": "ok",
            "path": "geoip/ok.srs"
        })];
        p.route.rules = vec![
            json!({"rule_set": "missing", "outbound": "direct"}),
            json!({"rule_set": "ok", "outbound": "direct"}),
            json!({"rule_set": ["ok"], "outbound": "direct"}),
            json!({
                "type": "logical",
                "mode": "and",
                "outbound": "direct",
                "rules": [
                    {"domain": ["example.com"]},
                    {"rule_set": "also-missing"}
                ]
            }),
        ];
        prune_dangling_refs(&mut p);
        assert_eq!(p.route.rules.len(), 2);
        assert_eq!(p.route.rules[0]["rule_set"], "ok");
        assert_eq!(p.route.rules[1]["rule_set"], json!(["ok"]));
        assert!(p.parse_stats.warnings.iter().any(|warning| {
            warning.key == "parse.routeRuleSetMissing"
                && warning.params.get("rule_set").map(String::as_str) == Some("missing")
        }));
    }
}
