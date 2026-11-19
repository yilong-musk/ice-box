// SPDX-License-Identifier: GPL-3.0-or-later

//! Outbound tags the app owns, and the rename that keeps imported
//! subscriptions from claiming them.
//!
//! `direct` / `block` are emitted by the config builder itself, `proxy` names
//! the selector injected for a group-less profile (and the tag the Clash API
//! switches, see `ice_core::SELECTOR_TAG`), and `dns` is treated as always
//! available by profile pruning. A subscription outbound that reuses one of
//! those names either fails the build or silently hijacks the target:
//!
//! * a node tagged `proxy` on a group-less profile duplicates the injected
//!   selector, and validation rejects the config (`duplicate tag proxy`);
//! * a node tagged `direct` / `block` shadows the built-in outbound, so every
//!   `direct` rule — including the reserved TUN bypass rules — routes through
//!   the subscription server instead of leaving the machine.
//!
//! Parsers rename a colliding subscription outbound to `name-2`, `name-3`, ...
//! and keep the reserved name for the app, so the hard-coded names elsewhere
//! (`ice_core::SELECTOR_TAG`, the desktop node pick, the URI split-routing
//! detour) stay valid.
//!
//! Only the runtime profile is renamed. The exported sing-box copy
//! ([`crate::share_singbox_config`]) keeps subscription node names verbatim so
//! the document matches the provider and imports elsewhere unchanged; its
//! injected selector just takes the next free tag.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::{NormalizedOutbound, NormalizedProfile, UiMessage};

/// Tags the app reserves for its own outbounds.
pub const RESERVED_OUTBOUND_TAGS: &[&str] = &["direct", "block", "dns", "proxy"];

/// Whether `tag` belongs to the app rather than to a subscription.
pub fn is_reserved_outbound_tag(tag: &str) -> bool {
    RESERVED_OUTBOUND_TAGS.contains(&tag)
}

/// First free tag among `preferred`, `preferred-2`, `preferred-3`, ...
pub fn fresh_outbound_tag(taken: &HashSet<String>, preferred: &str) -> String {
    if !taken.contains(preferred) {
        return preferred.to_string();
    }
    for n in 2..u32::MAX {
        let candidate = format!("{preferred}-{n}");
        if !taken.contains(&candidate) {
            return candidate;
        }
    }
    format!("{preferred}-fallback")
}

/// Rename subscription outbounds whose tag collides with a reserved one,
/// rewriting the profile's references so it stays self-consistent.
///
/// Returns one warning per renamed outbound (`parse.renamedReservedTag`), so
/// the change shows up in subscription details instead of being silent.
/// Idempotent: a renamed tag (`proxy-2`) is no longer reserved, so a second
/// call is a no-op. That is what lets profile snapshots saved before this
/// rename existed be migrated on every read.
pub fn rename_reserved_outbounds(profile: &mut NormalizedProfile) -> Vec<UiMessage> {
    let mut taken: HashSet<String> = profile.all_tags().into_iter().collect();
    let mut renamed: HashMap<String, String> = HashMap::new();
    let mut warnings = Vec::new();

    for outbound in profile.nodes.iter_mut().chain(profile.groups.iter_mut()) {
        rename_outbound(outbound, &mut taken, &mut renamed, &mut warnings);
    }
    if renamed.is_empty() {
        return warnings;
    }

    let targets = reference_targets(&renamed, profile.groups.is_empty());
    if let Some(new) = targets.get(&profile.route.final_outbound) {
        profile.route.final_outbound.clone_from(new);
    }
    for rule in &mut profile.route.rules {
        rewrite_rule(rule, &targets);
    }
    for outbound in profile.nodes.iter_mut().chain(profile.groups.iter_mut()) {
        rewrite_outbound_refs(outbound, &targets);
    }
    rewrite_dns_detours(profile, &targets);
    // `default_outbound` always names one of the profile's own outbounds (the
    // main group or the first node), so it follows every rename.
    if let Some(default) = profile.default_outbound.as_mut() {
        if let Some(new) = renamed.get(default.as_str()) {
            *default = new.clone();
        }
    }
    warnings
}

fn rename_outbound(
    outbound: &mut NormalizedOutbound,
    taken: &mut HashSet<String>,
    renamed: &mut HashMap<String, String>,
    warnings: &mut Vec<UiMessage>,
) {
    let old = outbound.tag.clone();
    if !is_reserved_outbound_tag(&old) {
        return;
    }
    let new = fresh_outbound_tag(taken, &old);
    taken.insert(new.clone());
    // A second collision keeps its own new name; references can only follow
    // one of them, and the first outbound in declaration order wins.
    renamed.entry(old.clone()).or_insert_with(|| new.clone());
    outbound.tag.clone_from(&new);
    if let Some(obj) = outbound.outbound_mut().as_object_mut() {
        obj.insert("tag".into(), Value::String(new.clone()));
    }
    warnings.push(
        UiMessage::new("parse.renamedReservedTag")
            .with("tag", old)
            .with("new_tag", new),
    );
}

/// References that must follow a renamed outbound.
///
/// `direct` / `block` are always emitted by the builder, and a group-less
/// profile always gets the `proxy` selector, so a reference to those names
/// keeps meaning the app-owned outbound — the fail-safe direction (traffic
/// that a rule sends `direct` stops being proxied rather than the reverse).
/// Every other reserved name can only have referred to the subscription's own
/// outbound, so its references move along.
fn reference_targets(renamed: &HashMap<String, String>, flat: bool) -> HashMap<String, String> {
    renamed
        .iter()
        .filter(|(old, _)| match old.as_str() {
            "direct" | "block" => false,
            "proxy" => !flat,
            _ => true,
        })
        .map(|(old, new)| (old.clone(), new.clone()))
        .collect()
}

fn rewrite_rule(rule: &mut Value, targets: &HashMap<String, String>) {
    let Some(obj) = rule.as_object_mut() else {
        return;
    };
    let outbound = obj
        .get("outbound")
        .and_then(Value::as_str)
        .map(str::to_string);
    if let Some(new) = outbound.and_then(|tag| targets.get(&tag).cloned()) {
        obj.insert("outbound".into(), Value::String(new));
    }
    if let Some(nested) = obj.get_mut("rules").and_then(Value::as_array_mut) {
        for child in nested {
            rewrite_rule(child, targets);
        }
    }
}

/// Rewrite references inside an outbound: a strategy group's member list and
/// default pick, plus a leaf's `detour`.
fn rewrite_outbound_refs(outbound: &mut NormalizedOutbound, targets: &HashMap<String, String>) {
    let Some(obj) = outbound.outbound_mut().as_object_mut() else {
        return;
    };
    for key in ["detour", "default"] {
        let current = obj.get(key).and_then(Value::as_str).map(str::to_string);
        if let Some(new) = current.and_then(|tag| targets.get(&tag).cloned()) {
            obj.insert(key.into(), Value::String(new));
        }
    }
    if let Some(members) = obj.get_mut("outbounds").and_then(Value::as_array_mut) {
        for member in members.iter_mut() {
            let current = member.as_str().map(str::to_string);
            if let Some(new) = current.and_then(|tag| targets.get(&tag).cloned()) {
                *member = Value::String(new);
            }
        }
    }
}

fn rewrite_dns_detours(profile: &mut NormalizedProfile, targets: &HashMap<String, String>) {
    let Some(servers) = profile
        .dns
        .as_mut()
        .and_then(|dns| dns.get_mut("servers"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for server in servers {
        let Some(obj) = server.as_object_mut() else {
            continue;
        };
        let current = obj
            .get("detour")
            .and_then(Value::as_str)
            .map(str::to_string);
        if let Some(new) = current.and_then(|tag| targets.get(&tag).cloned()) {
            obj.insert("detour".into(), Value::String(new));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use serde_json::json;

    use super::{
        fresh_outbound_tag, is_reserved_outbound_tag, rename_reserved_outbounds,
        RESERVED_OUTBOUND_TAGS,
    };
    use crate::{NormalizedOutbound, NormalizedProfile, NormalizedRoute, ProfileParseStats};

    fn socks(tag: &str) -> NormalizedOutbound {
        NormalizedOutbound {
            tag: tag.into(),
            outbound: Arc::new(json!({
                "type": "socks",
                "tag": tag,
                "server": "127.0.0.1",
                "server_port": 1080
            })),
        }
    }

    fn selector(tag: &str, members: &[&str]) -> NormalizedOutbound {
        NormalizedOutbound {
            tag: tag.into(),
            outbound: Arc::new(json!({
                "type": "selector",
                "tag": tag,
                "outbounds": members,
                "default": members.first().copied().unwrap_or_default()
            })),
        }
    }

    fn grouped(
        nodes: Vec<NormalizedOutbound>,
        groups: Vec<NormalizedOutbound>,
        final_outbound: &str,
    ) -> NormalizedProfile {
        NormalizedProfile {
            default_outbound: groups.first().map(|g| g.tag.clone()),
            nodes,
            groups,
            route: NormalizedRoute {
                rules: Vec::new(),
                final_outbound: final_outbound.into(),
                rule_sets: Vec::new(),
            },
            dns: None,
            parse_stats: ProfileParseStats::default(),
        }
    }

    fn tags(profile: &NormalizedProfile) -> Vec<&str> {
        profile.nodes.iter().map(|n| n.tag.as_str()).collect()
    }

    #[test]
    fn reserved_list_matches_the_builders_owned_tags() {
        assert_eq!(RESERVED_OUTBOUND_TAGS.len(), 4);
        assert!(is_reserved_outbound_tag("proxy"));
        assert!(is_reserved_outbound_tag("direct"));
        assert!(is_reserved_outbound_tag("block"));
        assert!(is_reserved_outbound_tag("dns"));
        assert!(!is_reserved_outbound_tag("proxy-2"));
        assert!(!is_reserved_outbound_tag("Proxies"));
    }

    #[test]
    fn fresh_tag_skips_taken_names() {
        let mut taken: HashSet<String> = HashSet::new();
        assert_eq!(fresh_outbound_tag(&taken, "free"), "free");
        taken.insert("proxy".into());
        assert_eq!(fresh_outbound_tag(&taken, "proxy"), "proxy-2");
        taken.insert("proxy-2".into());
        assert_eq!(fresh_outbound_tag(&taken, "proxy"), "proxy-3");
    }

    #[test]
    fn flat_node_named_proxy_leaves_the_injected_selector_free() {
        let mut profile = NormalizedProfile::from_nodes_only(vec![socks("proxy"), socks("jp")]);
        profile.route.final_outbound = "proxy".into();
        let warnings = rename_reserved_outbounds(&mut profile);
        assert_eq!(tags(&profile), ["proxy-2", "jp"]);
        assert_eq!(profile.nodes[0].outbound["tag"], "proxy-2");
        // The flat profile's `final` still points at the injected selector.
        assert_eq!(profile.route.final_outbound, "proxy");
        // `default_outbound` names a profile outbound, so it follows the rename.
        assert_eq!(profile.default_outbound.as_deref(), Some("proxy-2"));
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].key, "parse.renamedReservedTag");
        assert_eq!(warnings[0].params["tag"], "proxy");
        assert_eq!(warnings[0].params["new_tag"], "proxy-2");
    }

    #[test]
    fn direct_and_block_references_keep_pointing_at_the_builtins() {
        let mut profile = NormalizedProfile::from_nodes_only(vec![socks("direct"), socks("jp")]);
        profile.route = NormalizedRoute {
            rules: vec![json!({"domain_suffix": ["a.example"], "outbound": "direct"})],
            final_outbound: "direct".into(),
            rule_sets: Vec::new(),
        };
        rename_reserved_outbounds(&mut profile);
        assert_eq!(profile.nodes[0].tag, "direct-2");
        assert_eq!(profile.nodes[0].outbound["tag"], "direct-2");
        assert_eq!(profile.route.rules[0]["outbound"], "direct");
        assert_eq!(profile.route.final_outbound, "direct");
    }

    #[test]
    fn group_member_lists_keep_direct_as_the_builtin_too() {
        let mut profile = grouped(
            vec![socks("direct"), socks("jp")],
            vec![selector("main", &["direct", "jp"])],
            "main",
        );
        rename_reserved_outbounds(&mut profile);
        assert_eq!(profile.nodes[0].tag, "direct-2");
        // A group that listed `direct` keeps the built-in rather than being
        // silently pointed at the subscription server.
        assert_eq!(
            profile.groups[0].outbound["outbounds"],
            json!(["direct", "jp"])
        );
    }

    #[test]
    fn grouped_group_named_proxy_takes_its_references_along() {
        let mut profile = grouped(
            vec![socks("n1")],
            vec![selector("proxy", &["n1"]), selector("main", &["proxy"])],
            "proxy",
        );
        profile.route.rules = vec![json!({"domain": ["a.example"], "outbound": "proxy"})];
        profile.default_outbound = Some("proxy".into());
        rename_reserved_outbounds(&mut profile);
        assert_eq!(profile.groups[0].tag, "proxy-2");
        assert_eq!(profile.groups[0].outbound["tag"], "proxy-2");
        // A grouped profile injects no `proxy` selector, so the references
        // move with the group.
        assert_eq!(profile.route.final_outbound, "proxy-2");
        assert_eq!(profile.route.rules[0]["outbound"], "proxy-2");
        assert_eq!(profile.groups[1].outbound["outbounds"], json!(["proxy-2"]));
        assert_eq!(profile.groups[1].outbound["default"], "proxy-2");
        assert_eq!(profile.default_outbound.as_deref(), Some("proxy-2"));
    }

    #[test]
    fn nested_rule_and_dns_detour_follow_a_renamed_dns_outbound() {
        let mut profile = NormalizedProfile::from_nodes_only(vec![socks("dns"), socks("jp")]);
        profile.route.rules = vec![json!({
            "type": "logical",
            "mode": "and",
            "rules": [
                {"domain": ["a.example"]},
                {"outbound": "dns"},
            ],
        })];
        profile.dns = Some(json!({
            "servers": [
                {"type": "local", "tag": "local"},
                {"type": "https", "tag": "remote", "server": "1.1.1.1", "detour": "dns"},
            ],
            "final": "local",
        }));
        rename_reserved_outbounds(&mut profile);
        assert_eq!(profile.nodes[0].tag, "dns-2");
        assert_eq!(profile.route.rules[0]["rules"][1]["outbound"], "dns-2");
        assert_eq!(
            profile.dns.as_ref().unwrap()["servers"][1]["detour"],
            "dns-2"
        );
    }

    #[test]
    fn leaf_detour_follows_a_renamed_outbound() {
        let mut leaf = socks("jp");
        leaf.outbound_mut()
            .as_object_mut()
            .unwrap()
            .insert("detour".into(), json!("dns"));
        let mut profile = NormalizedProfile::from_nodes_only(vec![socks("dns"), leaf]);
        rename_reserved_outbounds(&mut profile);
        assert_eq!(profile.nodes[1].outbound["detour"], "dns-2");
    }

    #[test]
    fn rename_is_idempotent() {
        let mut profile = NormalizedProfile::from_nodes_only(vec![socks("proxy"), socks("jp")]);
        rename_reserved_outbounds(&mut profile);
        let before: Vec<String> = profile.nodes.iter().map(|n| n.tag.clone()).collect();
        assert!(rename_reserved_outbounds(&mut profile).is_empty());
        let after: Vec<String> = profile.nodes.iter().map(|n| n.tag.clone()).collect();
        assert_eq!(before, after);
    }

    #[test]
    fn two_colliding_outbounds_get_distinct_names() {
        let mut profile = grouped(
            vec![socks("proxy"), socks("jp")],
            vec![selector("proxy", &["jp"])],
            "proxy",
        );
        rename_reserved_outbounds(&mut profile);
        assert_eq!(profile.nodes[0].tag, "proxy-2");
        assert_eq!(profile.groups[0].tag, "proxy-3");
    }

    #[test]
    fn an_existing_suffixed_tag_is_not_reused() {
        let mut profile =
            NormalizedProfile::from_nodes_only(vec![socks("proxy"), socks("proxy-2")]);
        rename_reserved_outbounds(&mut profile);
        assert_eq!(tags(&profile), ["proxy-3", "proxy-2"]);
    }
}
