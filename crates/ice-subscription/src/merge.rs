// SPDX-License-Identifier: GPL-3.0-or-later

//! Load the single active subscription profile.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use ice_config::{HostPlatform, NormalizedOutbound, NormalizedProfile, NormalizedRoute};
use serde_json::{Map, Value};

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
    /// `route.rules` and `dns` were dropped. A later full load re-reads disk.
    rules_released: bool,
}

impl ProfileLoadCache {
    fn matches(
        &self,
        profile_path: &Path,
        profile_sig: Option<(SystemTime, u64)>,
        nodes_sig: Option<(SystemTime, u64)>,
        auto_default_rules: bool,
        platform: HostPlatform,
    ) -> bool {
        self.profile_path == profile_path
            && self.profile_sig == profile_sig
            && self.nodes_sig == nodes_sig
            && self.auto_default_rules == auto_default_rules
            && self.platform == platform
    }
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
            // A released entry has no rules or DNS. Config generation must
            // re-read the file instead of reusing that resident copy.
            if !entry.rules_released
                && entry.matches(
                    &profile_path,
                    profile_sig,
                    nodes_sig,
                    auto_default_rules,
                    platform,
                )
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
            rules_released: false,
        });
        Ok(profile)
    }

    /// Resident profile for UI reads: same identity as a full load, without
    /// `route.rules`, `dns`, import warnings, or outbound connection fields.
    /// Those stay on disk until a full load. The Rules page loads rules on
    /// its own. Leaf nodes share one `{"type"}` value per protocol.
    ///
    /// When this process already holds the full profile alone, the rules and
    /// DNS allocations are cleared in place. A caller that still holds the
    /// full `Arc` keeps it; the cache then stores a separate resident copy.
    pub fn load_resident(
        &self,
        paths: &SubscriptionPaths,
        index: &SubscriptionIndex,
        auto_default_rules: bool,
        platform: HostPlatform,
    ) -> Result<Arc<NormalizedProfile>, SubscriptionError> {
        if let Some(profile) = self.resident_if_current(paths, index, auto_default_rules, platform)
        {
            return Ok(profile);
        }
        let full =
            self.load_active_with_default_rules(paths, index, auto_default_rules, platform)?;
        drop(full);
        // Return the slim `Arc` from the same critical section that publishes
        // it. A config build can replace the cache with a full profile before
        // a later lookup, and that lookup must not surface as "no subscription".
        self.release_rule_bodies()
            .ok_or(SubscriptionError::NoActiveSubscription)
    }

    /// Drop rule, DNS, and outbound connection payloads from the cached
    /// profile so they are not retained between config rebuilds. Leaf nodes
    /// keep a shared `{"type"}` value. Groups keep `type`, `outbounds`, and
    /// `default`.
    ///
    /// Returns the resident profile left in the cache. `None` when the cache
    /// is empty. Already-resident entries are returned as they are.
    pub fn release_rule_bodies(&self) -> Option<Arc<NormalizedProfile>> {
        let mut cache = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = cache.as_mut()?;
        if !entry.rules_released {
            if let Some(profile) = Arc::get_mut(&mut entry.profile) {
                retain_resident(profile);
            } else {
                let mut slim = without_rule_bodies(&entry.profile);
                retain_resident(&mut slim);
                entry.profile = Arc::new(slim);
            }
            entry.rules_released = true;
        }
        Some(Arc::clone(&entry.profile))
    }

    /// The resident `Arc` when the cache matches `index` and rules were
    /// already released. `None` while a full profile is cached: callers must
    /// not adopt that allocation into a long-lived UI cache.
    pub fn resident_if_current(
        &self,
        paths: &SubscriptionPaths,
        index: &SubscriptionIndex,
        auto_default_rules: bool,
        platform: HostPlatform,
    ) -> Option<Arc<NormalizedProfile>> {
        let meta = active_subscription(index)?;
        let profile_path = paths.profile(meta.id);
        let cache = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = cache.as_ref()?;
        if entry.rules_released
            && entry.matches(
                &profile_path,
                file_sig(&profile_path),
                file_sig(&paths.nodes(meta.id)),
                auto_default_rules,
                platform,
            )
        {
            Some(Arc::clone(&entry.profile))
        } else {
            None
        }
    }
}

/// Copy used when another `Arc` still owns the full profile. Rules, DNS, and
/// parse warnings are not cloned into the resident copy. Outbound JSON is
/// still shared until [`retain_resident`] replaces it.
fn without_rule_bodies(full: &NormalizedProfile) -> NormalizedProfile {
    NormalizedProfile {
        nodes: full.nodes.clone(),
        groups: full.groups.clone(),
        route: NormalizedRoute {
            rules: Vec::new(),
            final_outbound: full.route.final_outbound.clone(),
            rule_sets: full.route.rule_sets.clone(),
        },
        dns: None,
        default_outbound: full.default_outbound.clone(),
        parse_stats: ice_config::ProfileParseStats {
            skipped_proxies: full.parse_stats.skipped_proxies,
            skipped_rules: full.parse_stats.skipped_rules,
            skipped_groups: full.parse_stats.skipped_groups,
            unsupported_rule_types: Vec::new(),
            geoip_codes: full.parse_stats.geoip_codes.clone(),
            warnings: Vec::new(),
        },
    }
}

/// Keep the fields UI reads (tags, types, group members, defaults) and drop
/// connection payloads, rules, DNS, and import warnings.
fn retain_resident(profile: &mut NormalizedProfile) {
    profile.route.rules = Vec::new();
    profile.dns = None;
    profile.parse_stats.warnings = Vec::new();
    profile.parse_stats.unsupported_rule_types = Vec::new();
    for outbound in profile.nodes.iter_mut().chain(profile.groups.iter_mut()) {
        shrink_outbound(outbound);
    }
}

fn shrink_outbound(outbound: &mut NormalizedOutbound) {
    let mut owned = std::mem::replace(&mut outbound.outbound, shared_null());
    let (ty, members, default) = if let Some(value) = Arc::get_mut(&mut owned) {
        let ty = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let members = value
            .as_object_mut()
            .and_then(|obj| obj.remove("outbounds"));
        let default = value.as_object_mut().and_then(|obj| obj.remove("default"));
        drop(owned);
        (ty, members, default)
    } else {
        let ty = owned
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let members = owned.get("outbounds").cloned();
        let default = owned.get("default").cloned();
        drop(owned);
        (ty, members, default)
    };
    outbound.outbound = slim_outbound(&ty, members, default);
}

fn slim_outbound(ty: &str, members: Option<Value>, default: Option<Value>) -> Arc<Value> {
    if members.is_none() && default.is_none() {
        return intern_type_value(ty);
    }
    let mut map = Map::new();
    map.insert("type".into(), Value::String(ty.to_string()));
    if let Some(members) = members {
        map.insert("outbounds".into(), members);
    }
    if let Some(default) = default {
        map.insert("default".into(), default);
    }
    Arc::new(Value::Object(map))
}

fn shared_null() -> Arc<Value> {
    static NULL: OnceLock<Arc<Value>> = OnceLock::new();
    NULL.get_or_init(|| Arc::new(Value::Null)).clone()
}

/// One `{"type"}` object per common outbound type, shared by every leaf that
/// uses it. Unknown types stay private so a profile of unique type strings
/// cannot grow a process-lifetime map.
fn intern_type_value(ty: &str) -> Arc<Value> {
    macro_rules! known {
        ($($name:literal),* $(,)?) => {
            match ty {
                $($name => {
                    static SLOT: OnceLock<Arc<Value>> = OnceLock::new();
                    SLOT.get_or_init(|| Arc::new(serde_json::json!({ "type": $name }))).clone()
                })*
                _ => Arc::new(serde_json::json!({ "type": ty })),
            }
        };
    }
    known!(
        "anytls",
        "block",
        "direct",
        "dns",
        "fallback",
        "http",
        "hysteria",
        "hysteria2",
        "loadbalance",
        "mieru",
        "naive",
        "selector",
        "shadowsocks",
        "shadowtls",
        "socks",
        "ssh",
        "trojan",
        "tuic",
        "unknown",
        "urltest",
        "vless",
        "vmess",
        "wireguard",
    )
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
            userinfo: None,
            provider_info: vec![],
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

    #[test]
    fn release_rule_bodies_keeps_nodes_and_reloads_rules() {
        use crate::store::{load_index, write_subscription_success};
        use crate::{SubscriptionFormat, SubscriptionMeta};
        use std::time::{SystemTime, UNIX_EPOCH};
        use uuid::Uuid;

        let dir = std::env::temp_dir().join(format!(
            "ice-box-profile-release-{}",
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
            rule_count: 1,
            has_dns: true,
            parse_warnings: vec![],
            last_updated: None,
            last_error: None,
            etag: None,
            last_modified: None,
            userinfo: None,
            provider_info: vec![],
            auto_update: false,
            auto_update_interval: None,
        };
        let mut profile = NormalizedProfile::from_nodes_only(vec![NormalizedOutbound {
            tag: "n1".into(),
            outbound: std::sync::Arc::new(
                serde_json::json!({"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}),
            ),
        }]);
        profile.route.rules = vec![serde_json::json!({
            "domain": "example.com",
            "outbound": "direct"
        })];
        profile.dns = Some(serde_json::json!({
            "servers": [{"type": "udp", "server": "1.1.1.1"}]
        }));
        write_subscription_success(&paths, &meta, "{}", &profile).expect("seed");
        let index = load_index(&paths).expect("index");
        let cache = ProfileCache::new();
        let full = cache
            .load_active_with_default_rules(&paths, &index, false, HostPlatform::MacOs)
            .expect("full");
        assert_eq!(full.route.rules.len(), 1);
        assert!(full.dns.is_some());
        drop(full);
        cache.release_rule_bodies();

        let resident = cache
            .load_resident(&paths, &index, false, HostPlatform::MacOs)
            .expect("resident");
        assert_eq!(resident.nodes[0].tag, "n1");
        assert!(resident.route.rules.is_empty());
        assert!(resident.dns.is_none());
        assert!(resident.nodes[0].outbound.get("server").is_none());
        assert_eq!(
            resident.nodes[0]
                .outbound
                .get("type")
                .and_then(|v| v.as_str()),
            Some("socks")
        );
        assert!(cache
            .resident_if_current(&paths, &index, false, HostPlatform::MacOs)
            .is_some());

        let again = cache
            .load_active_with_default_rules(&paths, &index, false, HostPlatform::MacOs)
            .expect("reloaded");
        assert_eq!(again.route.rules.len(), 1);
        assert!(again.dns.is_some());
        assert_eq!(again.nodes[0].outbound["server"], "1.1.1.1");
        assert!(!Arc::ptr_eq(&resident, &again));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn release_rule_bodies_shares_leaf_types_and_keeps_group_members() {
        use crate::store::{load_index, write_subscription_success};
        use crate::{SubscriptionFormat, SubscriptionMeta};
        use ice_config::{ProfileParseStats, UiMessage};
        use std::time::{SystemTime, UNIX_EPOCH};
        use uuid::Uuid;

        let dir = std::env::temp_dir().join(format!(
            "ice-box-profile-shrink-{}",
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
            node_count: 2,
            group_count: 1,
            rule_count: 0,
            has_dns: false,
            parse_warnings: vec![],
            last_updated: None,
            last_error: None,
            etag: None,
            last_modified: None,
            userinfo: None,
            provider_info: vec![],
            auto_update: false,
            auto_update_interval: None,
        };
        let profile = NormalizedProfile {
            nodes: vec![
                NormalizedOutbound::new(
                    "a",
                    serde_json::json!({"type":"vmess","tag":"a","server":"1.1.1.1","uuid":"u"}),
                ),
                NormalizedOutbound::new(
                    "b",
                    serde_json::json!({"type":"vmess","tag":"b","server":"2.2.2.2","uuid":"v"}),
                ),
            ],
            groups: vec![NormalizedOutbound::new(
                "Proxies",
                serde_json::json!({
                    "type": "selector",
                    "tag": "Proxies",
                    "outbounds": ["a", "b"],
                    "default": "a",
                    "interrupt_exist_connections": true
                }),
            )],
            route: NormalizedRoute::default(),
            dns: None,
            default_outbound: Some("Proxies".into()),
            parse_stats: ProfileParseStats {
                warnings: vec![UiMessage::raw("skipped one proxy")],
                ..ProfileParseStats::default()
            },
        };
        write_subscription_success(&paths, &meta, "{}", &profile).expect("seed");
        let index = load_index(&paths).expect("index");
        let cache = ProfileCache::new();
        let full = cache
            .load_active_with_default_rules(&paths, &index, false, HostPlatform::MacOs)
            .expect("full");
        assert!(full.nodes[0].outbound.get("server").is_some());
        assert_eq!(full.parse_stats.warnings.len(), 1);
        drop(full);
        cache.release_rule_bodies();
        let resident = cache
            .load_resident(&paths, &index, false, HostPlatform::MacOs)
            .expect("resident");
        assert!(Arc::ptr_eq(
            &resident.nodes[0].outbound,
            &resident.nodes[1].outbound
        ));
        assert!(resident.nodes[0].outbound.get("uuid").is_none());
        assert_eq!(resident.parse_stats.warnings.len(), 0);
        let group = &resident.groups[0].outbound;
        assert_eq!(group["outbounds"], serde_json::json!(["a", "b"]));
        assert_eq!(group["default"], "a");
        assert!(group.get("interrupt_exist_connections").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_resident_stays_slim_while_a_full_load_replaces_the_cache() {
        use crate::store::{load_index, write_subscription_success};
        use crate::{SubscriptionFormat, SubscriptionMeta};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        use uuid::Uuid;

        let dir = std::env::temp_dir().join(format!(
            "ice-box-profile-resident-race-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let paths = SubscriptionPaths::from_root(&dir);
        let meta = SubscriptionMeta {
            id: Uuid::new_v4(),
            name: "t".into(),
            url: "https://example.com/s".into(),
            active: true,
            format: SubscriptionFormat::SingBox,
            node_count: 1,
            group_count: 0,
            rule_count: 1,
            has_dns: false,
            parse_warnings: vec![],
            last_updated: None,
            last_error: None,
            etag: None,
            last_modified: None,
            userinfo: None,
            provider_info: vec![],
            auto_update: false,
            auto_update_interval: None,
        };
        let mut profile = NormalizedProfile::from_nodes_only(vec![NormalizedOutbound::new(
            "n1",
            serde_json::json!({"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}),
        )]);
        profile.route.rules = vec![serde_json::json!({"domain":"example.com","outbound":"direct"})];
        write_subscription_success(&paths, &meta, "{}", &profile).expect("seed");
        let index = load_index(&paths).expect("index");
        let cache = Arc::new(ProfileCache::new());
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let cache = Arc::clone(&cache);
            let paths = SubscriptionPaths::from_root(&dir);
            let index = index.clone();
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let _ = cache.load_active_with_default_rules(
                        &paths,
                        &index,
                        false,
                        HostPlatform::MacOs,
                    );
                }
            })
        };

        for _ in 0..40 {
            let resident = cache
                .load_resident(&paths, &index, false, HostPlatform::MacOs)
                .expect("a full reload must not look like a missing subscription");
            assert!(resident.route.rules.is_empty());
            assert!(resident.nodes[0].outbound.get("server").is_none());
            assert_eq!(resident.nodes[0].tag, "n1");
        }
        stop.store(true, Ordering::Relaxed);
        worker.join().expect("worker");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
