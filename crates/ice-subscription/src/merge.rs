// SPDX-License-Identifier: GPL-3.0-or-later

//! Load the single active subscription profile.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use ice_config::{HostPlatform, NormalizedOutbound, NormalizedProfile};

use crate::clash::normalize_dns_on;
use crate::error::SubscriptionError;
use crate::store::{read_profile, SubscriptionPaths};
use crate::uri::apply_builtin_default_rules;
use crate::{SubscriptionIndex, SubscriptionMeta};

/// mtime/len of a file; `None` when the path is missing. Atomic writes change
/// at least one of these, so a matching signature is enough to reuse a parse.
fn file_sig(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

struct ProfileLoadCache {
    profile_path: PathBuf,
    profile_sig: Option<(SystemTime, u64)>,
    nodes_sig: Option<(SystemTime, u64)>,
    auto_default_rules: bool,
    platform: HostPlatform,
    profile: Arc<NormalizedProfile>,
}

/// Parsed-active-profile cache owned by the host (`AppState`), not a process
/// static, so tests and multiple app instances do not share state (SUB-6).
#[derive(Default)]
pub struct ProfileCache {
    inner: Mutex<Option<ProfileLoadCache>>,
}

impl ProfileCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load profile for the active subscription, attaching the built-in
    /// split-routing defaults (they are not baked into the cached profile).
    pub fn load_active(
        &self,
        paths: &SubscriptionPaths,
        index: &SubscriptionIndex,
        platform: HostPlatform,
    ) -> Result<Arc<NormalizedProfile>, SubscriptionError> {
        self.load_active_with_default_rules(paths, index, true, platform)
    }

    /// Like [`Self::load_active`], honoring the app's `auto_default_rules`
    /// setting: when enabled, rule-less profiles get the built-in defaults at
    /// load time so both the Rules page and the generated config stay consistent.
    ///
    /// The cached `profile.json` may predate the Windows DNS emission (parsed by
    /// an older binary), so the platform DNS shape is re-applied at load time:
    /// [`normalize_dns_on`] drops fakeip / `local` / UDP upstreams and pins the
    /// anchor (no-op off-Windows).
    ///
    /// Cache hits return a cloned `Arc` (SUB-6); the profile body is not copied.
    pub fn load_active_with_default_rules(
        &self,
        paths: &SubscriptionPaths,
        index: &SubscriptionIndex,
        auto_default_rules: bool,
        platform: HostPlatform,
    ) -> Result<Arc<NormalizedProfile>, SubscriptionError> {
        let meta = active_subscription(index).ok_or(SubscriptionError::NoActiveSubscription)?;
        if !paths.sub_dir(meta.id).exists() {
            return Err(SubscriptionError::ParseFailed(format!(
                "active subscription {} ({}) is missing on disk",
                meta.name, meta.id
            )));
        }
        let profile_path = paths.profile(meta.id);
        let nodes_path = paths.nodes(meta.id);
        let profile_sig = file_sig(&profile_path);
        let nodes_sig = file_sig(&nodes_path);

        let mut cache = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = cache.as_ref() {
            if entry.profile_path == profile_path
                && entry.profile_sig == profile_sig
                && entry.nodes_sig == nodes_sig
                && entry.auto_default_rules == auto_default_rules
                && entry.platform == platform
            {
                return Ok(Arc::clone(&entry.profile));
            }
        }

        let mut profile = read_profile(paths, meta.id)?;
        normalize_dns_on(&mut profile, platform.is_windows());
        if auto_default_rules {
            apply_builtin_default_rules(&mut profile, platform);
        }
        let profile = Arc::new(profile);
        *cache = Some(ProfileLoadCache {
            profile_path,
            profile_sig,
            nodes_sig,
            auto_default_rules,
            platform,
            profile: profile.clone(),
        });
        Ok(profile)
    }
}

/// Returns the active subscription meta, if any.
pub fn active_subscription(index: &SubscriptionIndex) -> Option<&SubscriptionMeta> {
    index.items.iter().find(|m| m.active)
}

/// Uncached load (each call parses). Prefer [`ProfileCache`] in long-lived hosts.
pub fn load_active_profile(
    paths: &SubscriptionPaths,
    index: &SubscriptionIndex,
    platform: HostPlatform,
) -> Result<Arc<NormalizedProfile>, SubscriptionError> {
    ProfileCache::new().load_active(paths, index, platform)
}

/// Uncached load (each call parses). Prefer [`ProfileCache`] in long-lived hosts.
pub fn load_active_profile_with_default_rules(
    paths: &SubscriptionPaths,
    index: &SubscriptionIndex,
    auto_default_rules: bool,
    platform: HostPlatform,
) -> Result<Arc<NormalizedProfile>, SubscriptionError> {
    ProfileCache::new().load_active_with_default_rules(paths, index, auto_default_rules, platform)
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
                outbound: std::sync::Arc::new(
                    serde_json::json!({"type":"selector","tag":"Proxies"}),
                ),
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

    #[test]
    fn load_active_profile_cache_invalidates_when_profile_changes() {
        use crate::store::{load_index, write_subscription_success};
        use crate::{SubscriptionFormat, SubscriptionMeta};
        use std::time::{SystemTime, UNIX_EPOCH};
        use uuid::Uuid;

        let dir = std::env::temp_dir().join(format!(
            "ice-box-profile-cache-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let paths = SubscriptionPaths::from_root(&dir);
        let id = Uuid::new_v4();
        let meta = SubscriptionMeta {
            id,
            name: "t".into(),
            url: "https://example.com/s".into(),
            active: true,
            format: SubscriptionFormat::SingBox,
            node_count: 1,
            group_count: 0,
            rule_count: 0,
            has_dns: false,
            parse_warnings: vec![],
            last_updated: None,
            last_error: None,
            etag: None,
            last_modified: None,
            auto_update: false,
            auto_update_interval: None,
        };
        let node = |tag: &str| NormalizedOutbound {
            tag: tag.into(),
            outbound: std::sync::Arc::new(
                serde_json::json!({"type":"socks","tag":tag,"server":"1.1.1.1","server_port":1}),
            ),
        };
        write_subscription_success(
            &paths,
            &meta,
            "{}",
            &NormalizedProfile::from_nodes_only(vec![node("n1")]),
        )
        .expect("seed");
        let index = load_index(&paths).expect("index");
        let cache = ProfileCache::new();
        let first = cache
            .load_active_with_default_rules(&paths, &index, false, HostPlatform::MacOs)
            .expect("first");
        assert_eq!(first.nodes[0].tag, "n1");
        let again = cache
            .load_active_with_default_rules(&paths, &index, false, HostPlatform::MacOs)
            .expect("cache");
        assert_eq!(again.nodes[0].tag, "n1");
        assert!(
            std::sync::Arc::ptr_eq(&first, &again),
            "cache hit must reuse the Arc instead of cloning the profile"
        );

        write_subscription_success(
            &paths,
            &meta,
            "{}",
            &NormalizedProfile::from_nodes_only(vec![node("n2-longer-tag")]),
        )
        .expect("rewrite");
        let index = load_index(&paths).expect("index");
        let updated = cache
            .load_active_with_default_rules(&paths, &index, false, HostPlatform::MacOs)
            .expect("invalidated");
        assert_eq!(updated.nodes[0].tag, "n2-longer-tag");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
