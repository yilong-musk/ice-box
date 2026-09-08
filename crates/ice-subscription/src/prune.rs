// SPDX-License-Identifier: GPL-3.0-or-later

//! Drop selector/urltest members, route refs, and `detour`s that point at
//! outbounds the parser skipped or truncated (SUB-2 / SEC-1).

use std::collections::HashSet;

use ice_config::NormalizedProfile;
use serde_json::json;

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
                    dropped_detour
                        .push(format!("dropped outbound {}: detour {d} is missing", n.tag));
                    false
                }
                _ => true,
            },
        );
        profile.parse_stats.warnings.extend(dropped_detour);

        let mut empty_groups = Vec::new();
        let mut trimmed_groups = Vec::new();
        for g in &mut profile.groups {
            if let Some(arr) = g
                .outbound
                .get_mut("outbounds")
                .and_then(|v| v.as_array_mut())
            {
                let before = arr.len();
                arr.retain(|m| m.as_str().is_some_and(|t| known.contains(t)));
                if arr.len() != before {
                    trimmed_groups.push(format!(
                        "trimmed group {}: dropped {} missing members",
                        g.tag,
                        before - arr.len()
                    ));
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
                empty_groups.push(format!("dropped empty group {}", g.tag));
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
                profile.parse_stats.warnings.push(format!(
                    "route rule outbound {out} is missing; falling back to direct"
                ));
                rule["outbound"] = json!("direct");
            }
        }
    }
    if !known.contains(&profile.route.final_outbound) {
        profile.parse_stats.warnings.push(format!(
            "route final {} is missing; falling back to direct",
            profile.route.final_outbound
        ));
        profile.route.final_outbound = "direct".into();
    }
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
            outbound: json!({"type":"shadowsocks","tag":"ss","detour":"st"}),
        };
        let keep = NormalizedOutbound {
            tag: "ok".into(),
            outbound: json!({"type":"shadowsocks","tag":"ok"}),
        };
        let group = NormalizedOutbound {
            tag: "proxy".into(),
            outbound: json!({"type":"selector","tag":"proxy","outbounds":["ss","ok","gone"]}),
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
        assert!(p.parse_stats.warnings.iter().any(|w| w.contains("detour")));
    }
}
