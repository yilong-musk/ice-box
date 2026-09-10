// SPDX-License-Identifier: GPL-3.0-or-later

//! Subscription CRUD, format detection, normalization, fetch, store, merge.
//!
//! Format priority: sing-box JSON first, then Clash-compatible YAML/text (slice 6).

mod clash;
mod decode;
mod error;
mod fetch;
mod limits;
mod merge;
mod prune;
mod store;
mod tls_fetch;
mod uri;
mod url;

/// Upper bound for simultaneous subscription network fetches.
pub(crate) const MAX_FETCH_CONCURRENCY: usize = 8;

mod manager;
#[cfg(test)]
mod tests_g5;

pub use clash::{
    normalize_dns_on, parse_clash_profile, parse_clash_with_stats, ClashParseResult,
    CLASH_SUPPORTED_TYPES, MAX_CLASH_PROXIES,
};
pub use decode::maybe_decode_base64;
pub use error::SubscriptionError;
pub use fetch::{
    DirectFetcher, FetchResponse, HttpFetcher, MockFetchMode, MockFetcher, PanicOnceMode,
    FETCH_TIMEOUT, MAX_BODY_BYTES,
};
pub use manager::{FetchedAdd, FetchedUpdate, MemorySubscriptionManager, SubscriptionManager};
pub use merge::{
    active_subscription, list_profile_outbounds, load_active_profile,
    load_active_profile_with_default_rules, resolve_selected_tag, short_id, ProfileCache,
};
pub use store::{
    apply_error_to_index, apply_success_to_index, clear_error_in_index, clear_subscription_error,
    commit_subscription_success, load_index, mark_refreshed_in_index, mark_subscription_refreshed,
    read_index, read_nodes, read_profile, recover_subscription_dirs, remove_subscription,
    save_index, set_active, set_auto_update, set_enabled, write_subscription_error,
    write_subscription_success, SubscriptionPaths,
};
pub use uri::{
    apply_builtin_default_rules, looks_like_uri_list, parse_uri_list_profile, MAX_URI_LINES,
};
pub use url::{
    redact_subscription_url_for_log, redact_subscription_url_for_ui, redact_urls_in_text,
};

use chrono::{DateTime, Utc};
use ice_config::{
    HostPlatform, NormalizedOutbound, NormalizedProfile, NormalizedRoute, ProfileParseStats,
    UiMessage,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::decode::maybe_decode_base64 as decode_body;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionFormat {
    SingBox,
    Clash,
    /// Proxy share-link list (`vless://`, `trojan://`, `hysteria2://`, ...).
    UriList,
    Unknown,
}

/// Refresh cadence offered for per-subscription auto-update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoUpdateInterval {
    OneHour,
    ThreeHours,
    SixHours,
    TwelveHours,
    TwentyFourHours,
}

impl AutoUpdateInterval {
    pub const ALL: [AutoUpdateInterval; 5] = [
        AutoUpdateInterval::OneHour,
        AutoUpdateInterval::ThreeHours,
        AutoUpdateInterval::SixHours,
        AutoUpdateInterval::TwelveHours,
        AutoUpdateInterval::TwentyFourHours,
    ];

    pub fn hours(self) -> u32 {
        match self {
            AutoUpdateInterval::OneHour => 1,
            AutoUpdateInterval::ThreeHours => 3,
            AutoUpdateInterval::SixHours => 6,
            AutoUpdateInterval::TwelveHours => 12,
            AutoUpdateInterval::TwentyFourHours => 24,
        }
    }

    pub fn duration(self) -> std::time::Duration {
        std::time::Duration::from_secs(self.hours() as u64 * 3600)
    }

    /// Used when a subscription predates interval selection (auto_update on,
    /// no interval stored): the closest cadence to the original 30-minute tick.
    pub fn default_duration() -> std::time::Duration {
        AutoUpdateInterval::OneHour.duration()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionMeta {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    #[serde(default, alias = "enabled")]
    pub active: bool,
    pub format: SubscriptionFormat,
    pub node_count: usize,
    #[serde(default)]
    pub group_count: usize,
    #[serde(default)]
    pub rule_count: usize,
    #[serde(default)]
    pub has_dns: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parse_warnings: Vec<UiMessage>,
    pub last_updated: Option<DateTime<Utc>>,
    pub last_error: Option<UiMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    /// Refresh this subscription on a background schedule when enabled.
    #[serde(default)]
    pub auto_update: bool,
    /// Cadence for the background refresh; `None` falls back to
    /// [`AutoUpdateInterval::default_duration`] for legacy entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_update_interval: Option<AutoUpdateInterval>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SubscriptionIndex {
    pub items: Vec<SubscriptionMeta>,
}

/// Detect format from raw subscription body.
pub fn detect_format(raw: &str) -> SubscriptionFormat {
    let trimmed = raw.trim_start();
    // Cheap structural sniff instead of a full JSON parse (bodies can be up to
    // 8 MiB): sing-box bodies are objects carrying a quoted `outbounds` /
    // `endpoints` key. The real parse happens later in `parse_singbox_profile`,
    // so a classification here loses no information or error detail.
    if trimmed.starts_with('{')
        && (trimmed.contains("\"outbounds\"") || trimmed.contains("\"endpoints\""))
    {
        return SubscriptionFormat::SingBox;
    }

    // Zero-allocation case-insensitive scan (no `to_ascii_lowercase` copy).
    if crate::decode::contains_ascii_case_insensitive(trimmed, "proxies:")
        || crate::decode::contains_ascii_case_insensitive(trimmed, "proxy-groups:")
        || crate::decode::contains_ascii_case_insensitive(trimmed, "mixed-port:")
    {
        return SubscriptionFormat::Clash;
    }

    if uri::looks_like_uri_list(trimmed) {
        return SubscriptionFormat::UriList;
    }

    SubscriptionFormat::Unknown
}

/// Parse raw content into normalized sing-box outbounds.
pub fn parse_subscription(
    raw: &str,
    format: SubscriptionFormat,
) -> Result<Vec<NormalizedOutbound>, SubscriptionError> {
    match format {
        SubscriptionFormat::SingBox => parse_singbox(raw),
        SubscriptionFormat::Clash => parse_clash(raw),
        SubscriptionFormat::UriList => uri::parse_uri_list_profile(raw).map(|p| p.nodes),
        SubscriptionFormat::Unknown => {
            let detected = detect_format(raw);
            match detected {
                SubscriptionFormat::Unknown => Err(SubscriptionError::UnknownFormat),
                other => parse_subscription(raw, other),
            }
        }
    }
}

/// Decode optional base64 wrapper, detect format, parse full profile.
pub fn normalize_raw_body(
    raw: &str,
    platform: HostPlatform,
) -> Result<(SubscriptionFormat, NormalizedProfile), SubscriptionError> {
    let decoded = decode_body(raw)?;
    let format = detect_format(&decoded);
    if format == SubscriptionFormat::Unknown {
        return Err(SubscriptionError::UnknownFormat);
    }
    let profile = parse_profile(&decoded, format, platform)?;
    Ok((format, profile))
}

pub fn parse_profile(
    raw: &str,
    format: SubscriptionFormat,
    platform: HostPlatform,
) -> Result<NormalizedProfile, SubscriptionError> {
    match format {
        SubscriptionFormat::SingBox => parse_singbox_profile(raw),
        SubscriptionFormat::Clash => parse_clash_profile(raw, platform),
        SubscriptionFormat::UriList => uri::parse_uri_list_profile(raw),
        SubscriptionFormat::Unknown => {
            let detected = detect_format(raw);
            match detected {
                SubscriptionFormat::Unknown => Err(SubscriptionError::UnknownFormat),
                other => parse_profile(raw, other, platform),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn meta_from_profile(
    id: Uuid,
    name: String,
    url: String,
    format: SubscriptionFormat,
    profile: &NormalizedProfile,
    active: bool,
    etag: Option<String>,
    last_modified: Option<String>,
    auto_update: bool,
    auto_update_interval: Option<AutoUpdateInterval>,
) -> SubscriptionMeta {
    SubscriptionMeta {
        id,
        name,
        url,
        active,
        format,
        node_count: profile.nodes.len(),
        group_count: profile.groups.len(),
        rule_count: profile.route.rules.len(),
        has_dns: profile.dns.is_some(),
        parse_warnings: profile.parse_stats.warnings.clone(),
        last_updated: Some(Utc::now()),
        last_error: None,
        etag,
        last_modified,
        auto_update,
        auto_update_interval,
    }
}

pub(crate) fn meta_from_fetched_profile(
    current: &SubscriptionMeta,
    fetched: &FetchResponse,
    format: SubscriptionFormat,
    profile: &NormalizedProfile,
) -> SubscriptionMeta {
    meta_from_profile(
        current.id,
        current.name.clone(),
        current.url.clone(),
        format,
        profile,
        current.active,
        fetched.etag.clone().or_else(|| current.etag.clone()),
        fetched
            .last_modified
            .clone()
            .or_else(|| current.last_modified.clone()),
        current.auto_update,
        current.auto_update_interval,
    )
}

pub fn parse_singbox_profile(raw: &str) -> Result<NormalizedProfile, SubscriptionError> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| SubscriptionError::ParseFailed(format!("sing-box json: {e}")))?;

    let mut nodes = Vec::new();
    let mut groups = Vec::new();
    let mut parse_stats = ProfileParseStats::default();

    if let Some(outbounds) = value
        .get("outbounds")
        .or_else(|| value.get("endpoints"))
        .and_then(|v| v.as_array())
    {
        for (idx, item) in outbounds.iter().enumerate() {
            let ty = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let tag = item
                .get("tag")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("outbound-{idx}"));
            if let Err(err) = ice_config_guard::check_outbound(item) {
                let is_group = matches!(
                    ty,
                    "selector" | "urltest" | "fallback" | "loadbalance" | "load-balance"
                );
                if is_group {
                    parse_stats.skipped_groups += 1;
                } else {
                    parse_stats.skipped_proxies += 1;
                }
                parse_stats.warnings.push(
                    UiMessage::new("parse.skippedOutbound")
                        .with("tag", tag)
                        .with("detail", err.to_string()),
                );
                continue;
            }
            let entry = NormalizedOutbound {
                tag: tag.clone(),
                outbound: std::sync::Arc::new(item.clone()),
            };
            match ty {
                "selector" | "urltest" => {
                    groups.push(entry);
                }
                "direct" | "block" | "dns" => {}
                _ => nodes.push(entry),
            }
        }
    }

    if nodes.is_empty() {
        return Err(SubscriptionError::EmptyNodes);
    }

    let limits = crate::limits::Limits::default();
    if nodes.len() > limits.max_nodes {
        let dropped = nodes.len() - limits.max_nodes;
        nodes.truncate(limits.max_nodes);
        parse_stats
            .warnings
            .push(crate::limits::Limits::warning("nodes", dropped));
    }
    if groups.len() > limits.max_groups {
        let dropped = groups.len() - limits.max_groups;
        groups.truncate(limits.max_groups);
        parse_stats
            .warnings
            .push(crate::limits::Limits::warning("groups", dropped));
    }

    let default_outbound = groups.first().map(|g| g.tag.clone());

    let route = if let Some(r) = value.get("route") {
        let mut rules = r
            .get("rules")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if rules.len() > limits.max_rules {
            let dropped = rules.len() - limits.max_rules;
            rules.truncate(limits.max_rules);
            parse_stats
                .warnings
                .push(crate::limits::Limits::warning("rules", dropped));
        }
        NormalizedRoute {
            rules,
            final_outbound: r
                .get("final")
                .and_then(|v| v.as_str())
                .unwrap_or("direct")
                .to_string(),
            rule_sets: {
                let mut sets = r
                    .get("rule_set")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                ice_config_guard::retain_local_rule_sets(&mut sets);
                sets
            },
        }
    } else {
        NormalizedRoute {
            final_outbound: groups
                .first()
                .map(|g| g.tag.clone())
                .unwrap_or_else(|| "proxy".into()),
            ..Default::default()
        }
    };

    Ok({
        let mut profile = NormalizedProfile {
            nodes,
            groups,
            route,
            dns: value.get("dns").cloned().map(|mut dns| {
                ice_config_guard::retain_allowed_dns_servers(&mut dns);
                dns
            }),
            default_outbound,
            parse_stats,
        };
        crate::prune::prune_dangling_refs(&mut profile);
        profile
    })
}

pub fn parse_singbox(raw: &str) -> Result<Vec<NormalizedOutbound>, SubscriptionError> {
    Ok(parse_singbox_profile(raw)?.nodes)
}

fn parse_clash(raw: &str) -> Result<Vec<NormalizedOutbound>, SubscriptionError> {
    Ok(parse_clash_profile(raw, HostPlatform::MacOs)?.nodes)
}

fn name_from_disposition(cd: Option<&str>) -> Option<String> {
    let cd = cd?;
    // filename="foo.json" or filename=foo.json
    for part in cd.split(';') {
        let part = part.trim();
        if let Some(rest) = part
            .strip_prefix("filename=")
            .or_else(|| part.strip_prefix("filename*="))
        {
            let name = rest.trim().trim_matches('"');
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn name_from_url(url: &str) -> Option<String> {
    let path = url.split('?').next().unwrap_or(url);
    let seg = path.rsplit('/').next().unwrap_or("");
    if seg.is_empty() || seg == path {
        return None;
    }
    Some(seg.to_string())
}

pub fn resolve_subscription_name(
    user_name: Option<&str>,
    content_disposition: Option<&str>,
    url: &str,
    id: Uuid,
) -> String {
    if let Some(n) = user_name.map(str::trim).filter(|s| !s.is_empty()) {
        return n.to_string();
    }
    if let Some(n) = name_from_disposition(content_disposition) {
        return n;
    }
    if let Some(n) = name_from_url(url) {
        return n;
    }
    format!("Subscription-{}", short_id(&id))
}
