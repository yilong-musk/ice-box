//! Normalized subscription routing profile (nodes + groups + route + dns).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::NormalizedOutbound;

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
    pub warnings: Vec<String>,
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
