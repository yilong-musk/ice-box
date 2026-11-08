// SPDX-License-Identifier: GPL-3.0-or-later

//! Normalized subscription routing profile (nodes + groups + route + dns).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::NormalizedOutbound;
use ice_types::UiMessage;

/// Parse-time statistics and non-fatal warnings.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileParseStats {
    pub skipped_proxies: usize,
    pub skipped_rules: usize,
    pub skipped_groups: usize,
    #[serde(default)]
    pub unsupported_rule_types: Vec<String>,
    /// Lowercase country codes used by GEOIP rules (resolved to bundled rule-sets at build).
    #[serde(default)]
    pub geoip_codes: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<UiMessage>,
}

/// sing-box route block derived from Clash rules or sing-box route.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedRoute {
    pub rules: Vec<Value>,
    #[serde(rename = "final")]
    pub final_outbound: String,
    #[serde(default)]
    pub rule_sets: Vec<Value>,
}

impl Default for NormalizedRoute {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            final_outbound: "direct".into(),
            rule_sets: Vec::new(),
        }
    }
}

/// Full normalized subscription profile for config generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedProfile {
    pub nodes: Vec<NormalizedOutbound>,
    #[serde(default)]
    pub groups: Vec<NormalizedOutbound>,
    pub route: NormalizedRoute,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_outbound: Option<String>,
    #[serde(default)]
    pub parse_stats: ProfileParseStats,
}

impl NormalizedProfile {
    /// Leaf + group outbounds for tag validation and UI listing.
    pub fn all_outbounds(&self) -> impl Iterator<Item = &NormalizedOutbound> {
        self.nodes.iter().chain(self.groups.iter())
    }

    pub fn all_tags(&self) -> Vec<String> {
        self.all_outbounds().map(|o| o.tag.clone()).collect()
    }

    /// Outermost group whose direct members include `tag`; prefers the
    /// profile's main group (`default_outbound`). `None` when the tag belongs
    /// to no group, or the profile has no groups.
    pub fn selection_group_for(&self, tag: &str) -> Option<String> {
        if self.groups.is_empty() {
            return None;
        }
        if let Some(top) = self.default_outbound.as_deref() {
            if self
                .groups
                .iter()
                .any(|g| g.tag == top && group_contains(g, tag))
            {
                return Some(top.to_string());
            }
        }
        self.groups
            .iter()
            .find(|g| group_contains(g, tag))
            .map(|g| g.tag.clone())
    }

    /// Whether picking `member` in `group` is also a node pick, i.e. moves
    /// `selected_tag`. Only a pick in the main group (`default_outbound`) of a
    /// member the profile defines does: Home's outbound line then follows a
    /// switch made on the Nodes page or in the tray. A pick in any other group
    /// (e.g. a per-service selector) leaves the selected node alone, whatever
    /// the group order.
    pub fn picks_selected_node(&self, group: &str, member: &str) -> bool {
        self.default_outbound.as_deref() == Some(group)
            && self
                .groups
                .iter()
                .any(|g| g.tag == group && group_contains(g, member))
            && self.all_outbounds().any(|o| o.tag == member)
    }

    /// Whether `tag` is a strategy group that no other group contains (e.g.
    /// the main group). Picking it is a live no-op for the Clash API.
    pub fn is_unselectable_group(&self, tag: &str) -> bool {
        !self.groups.is_empty()
            && self.groups.iter().any(|g| g.tag == tag)
            && self.selection_group_for(tag).is_none()
    }

    /// Minimal profile: nodes only, global proxy selector behavior (v1 fallback).
    pub fn from_nodes_only(nodes: Vec<NormalizedOutbound>) -> Self {
        let default = nodes.first().map(|n| n.tag.clone());
        Self {
            nodes,
            groups: Vec::new(),
            route: NormalizedRoute {
                rules: Vec::new(),
                final_outbound: "proxy".into(),
                rule_sets: Vec::new(),
            },
            dns: None,
            default_outbound: default,
            parse_stats: ProfileParseStats::default(),
        }
    }
}

/// Whether strategy group `group` lists `tag` among its direct members.
fn group_contains(group: &NormalizedOutbound, tag: &str) -> bool {
    group
        .outbound
        .get("outbounds")
        .and_then(|v| v.as_array())
        .is_some_and(|members| members.iter().filter_map(|v| v.as_str()).any(|m| m == tag))
}
