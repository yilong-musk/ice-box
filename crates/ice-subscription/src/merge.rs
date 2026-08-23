//! Load the single active subscription profile.

use ice_config::{NormalizedOutbound, NormalizedProfile};

use crate::error::SubscriptionError;
use crate::store::{read_profile, SubscriptionPaths};
use crate::{SubscriptionIndex, SubscriptionMeta};

/// Returns the active subscription meta, if any.
pub fn active_subscription(index: &SubscriptionIndex) -> Option<&SubscriptionMeta> {
    index.items.iter().find(|m| m.active)
}

/// Load profile for the active subscription.
pub fn load_active_profile(
    paths: &SubscriptionPaths,
    index: &SubscriptionIndex,
) -> Result<NormalizedProfile, SubscriptionError> {
    let meta = active_subscription(index).ok_or(SubscriptionError::NoActiveSubscription)?;
    if !paths.sub_dir(meta.id).exists() {
        return Err(SubscriptionError::ParseFailed(format!(
            "active subscription {} ({}) is missing on disk",
            meta.name, meta.id
        )));
    }
    read_profile(paths, meta.id)
}

/// Resolve `selected_tag`: keep if present in outbounds/groups, else default_outbound or first tag.
pub fn resolve_selected_tag(selected: Option<&str>, profile: &NormalizedProfile) -> Option<String> {
    let tags: Vec<String> = profile.all_tags();
    if tags.is_empty() {
        return None;
    }
    if let Some(sel) = selected {
        if tags.iter().any(|t| t == sel) {
            return Some(sel.to_string());
        }
    }
    if let Some(def) = &profile.default_outbound {
        if tags.iter().any(|t| t == def) {
            return Some(def.clone());
        }
    }
    profile
        .groups
        .first()
        .map(|g| g.tag.clone())
        .or_else(|| profile.nodes.first().map(|n| n.tag.clone()))
}

/// List outbounds for UI: groups first, then leaf nodes.
pub fn list_profile_outbounds(profile: &NormalizedProfile) -> Vec<NormalizedOutbound> {
    let mut out = profile.groups.clone();
    out.extend(profile.nodes.clone());
    out
}

/// Short 8-char uuid prefix for display names.
pub fn short_id(id: &uuid::Uuid) -> String {
    id.to_string()[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ice_config::ProfileParseStats;

    #[test]
    fn resolve_selected_prefers_existing_group() {
        let profile = NormalizedProfile {
            nodes: vec![],
            groups: vec![NormalizedOutbound {
                tag: "Proxies".into(),
                outbound: serde_json::json!({"type":"selector","tag":"Proxies"}),
            }],
            route: Default::default(),
            dns: None,
            default_outbound: Some("Proxies".into()),
            parse_stats: ProfileParseStats::default(),
        };
        assert_eq!(
            resolve_selected_tag(Some("Proxies"), &profile).as_deref(),
            Some("Proxies")
        );
    }
}
