// SPDX-License-Identifier: GPL-3.0-or-later

//! Host-process state: subscriptions, settings, and the config file the
//! tunnel process reads. Plugin calls stay in the commands.

use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ice_config::{
    is_android_package_name, load_group_selections, load_node_favorites, load_rule_overrides,
    load_settings, redact_config_str, remove_subscription_favorites, rule_fingerprint,
    rule_matches_fingerprint, rule_type_of, save_group_selections, save_rule_overrides,
    save_settings_for, set_node_favorite as persist_node_favorite, set_proxy_service_enabled_for,
    validate_custom_rule_value, AppError, AppPaths, AppProxyMode, AppSettings, ErrorCode,
    NormalizedProfile, ProxyMode, RuleOverrides, SettingsPatch,
};
use ice_core::{
    proxy_delay, proxy_group_heads, proxy_selected_now, GroupHead, HealthEndpoints, TrafficDelta,
    TrafficMonitor, TrafficSnapshot, DELAY_TEST_URL,
};
use ice_subscription::{
    active_subscription, read_index, redact_subscription_url_for_ui, ProfileCache,
    SubscriptionError, SubscriptionManager, SubscriptionMeta, SubscriptionPaths,
};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::config::{self, PLATFORM};
use crate::status::{self, StatusResponse, TunnelPhase, TunnelView, VpnPermission};

pub struct MobileHost {
    paths: Option<AppPaths>,
    traffic: TrafficMonitor,
    /// Parsed active profile without rules, DNS, or connection fields.
    /// Status polls reuse it instead of re-reading the subscription file.
    profile_cache: ProfileCache,
    /// Live group exits (`now` only). Member tags stay on the profile.
    groups_cache: Mutex<Option<(Instant, Vec<GroupHead>)>>,
    /// Serializes `GET /proxies` with [`Self::record_group_now`], so a sample
    /// that started before a selector switch cannot publish the old member
    /// afterwards.
    groups_fetch: Mutex<()>,
    /// Home's selected exit, so a status poll does not fetch the proxy map.
    now_cache: Mutex<Option<(Instant, String, String)>>,
    /// Serializes `GET /proxies/{tag}`. Separate from [`Self::groups_fetch`]
    /// so Home does not queue behind a full proxy-map read.
    now_fetch: Mutex<()>,
}

const PROXY_GROUPS_TTL: Duration = Duration::from_secs(10);

impl MobileHost {
    pub fn new() -> Self {
        Self {
            paths: None,
            traffic: TrafficMonitor::new(),
            profile_cache: ProfileCache::new(),
            groups_cache: Mutex::new(None),
            groups_fetch: Mutex::new(()),
            now_cache: Mutex::new(None),
            now_fetch: Mutex::new(()),
        }
    }

    pub fn ensure_paths(
        &mut self,
        private: AppPaths,
        shared_dir: Option<std::path::PathBuf>,
    ) -> Result<AppPaths, AppError> {
        if let Some(paths) = &self.paths {
            return Ok(paths.clone());
        }
        let shared = shared_dir.unwrap_or_else(|| private.root().to_path_buf());
        let paths = AppPaths::with_shared(private.root(), shared);
        paths.ensure_dirs().map_err(|err| {
            AppError::new(ErrorCode::ConfigInvalid, format!("create data dirs: {err}"))
        })?;
        self.paths = Some(paths.clone());
        // The phone does not remember that the VPN was on, and does not
        // reconnect it the next time the app opens. Clear a flag an older
        // build may have stored.
        set_proxy_service_enabled_for(&paths.settings(), false, PLATFORM)?;
        Ok(paths)
    }

    fn paths(&self) -> Result<&AppPaths, AppError> {
        self.paths.as_ref().ok_or_else(|| {
            AppError::new(
                ErrorCode::ConfigInvalid,
                "mobile data directory is not ready",
            )
        })
    }

    pub fn status(
        &mut self,
        view: TunnelView,
        message: Option<&str>,
        memory_bytes: Option<u64>,
        guidance: Option<status::DeviceGuidance>,
    ) -> Result<StatusResponse, AppError> {
        let paths = self.paths()?.clone();
        let settings = load_settings(&paths.settings())?;
        self.sync_traffic(&settings, view.phase);
        let count = read_index(&SubscriptionPaths::from_app(&paths))
            .map(|index| index.items.len())
            .unwrap_or(0);
        let live = view.phase == TunnelPhase::Connected;
        let (has_nodes, selected_outbound) = self.outbound_summary(live);
        Ok(status::status_response(
            view,
            message,
            memory_bytes,
            count,
            guidance,
            has_nodes,
            selected_outbound,
        ))
    }

    pub fn paths_ready(&self) -> bool {
        self.paths.is_some()
    }

    pub fn app_paths(&self) -> Result<AppPaths, AppError> {
        self.paths().cloned()
    }

    pub fn subscription_paths(&self) -> Result<SubscriptionPaths, AppError> {
        Ok(SubscriptionPaths::from_app(self.paths()?))
    }

    /// Rewrite `config.json` from disk state. Callers reload the tunnel when it is live.
    pub fn rewrite_config(&mut self, package_name: Option<&str>) -> Result<(), AppError> {
        let paths = self.paths()?.clone();
        config::write_config(&paths, package_name)
    }

    pub fn list_subscriptions(&self) -> Result<Vec<Value>, AppError> {
        let metas = self.manager()?.list()?;
        metas.iter().map(public_meta).collect()
    }

    pub fn apply_added(&self, add: ice_subscription::FetchedAdd) -> Result<Value, AppError> {
        let meta = self.manager()?.apply_add(add)?;
        public_meta(&meta)
    }

    pub fn apply_updated(
        &self,
        update: ice_subscription::FetchedUpdate,
    ) -> Result<Value, AppError> {
        let meta = self.manager()?.apply_update(update)?;
        public_meta(&meta)
    }

    pub fn note_subscription_error(
        &self,
        id: Uuid,
        message: ice_config::UiMessage,
    ) -> Result<(), AppError> {
        ice_subscription::write_subscription_error(&self.subscription_paths()?, id, message)?;
        Ok(())
    }

    pub fn remove_subscription(&self, id: Uuid) -> Result<(), AppError> {
        self.manager()?.remove(id)?;
        if let Err(err) = remove_subscription_favorites(&self.paths()?.node_favorites(), id) {
            log::warn!("remove_subscription: purge favorites failed: {err}");
        }
        Ok(())
    }

    pub fn set_active(&self, id: Uuid, active: bool) -> Result<Value, AppError> {
        let meta = self.manager()?.set_active(id, active)?;
        public_meta(&meta)
    }

    pub fn list_nodes(&self, live: bool) -> Result<Vec<NodeInfo>, AppError> {
        let Some((subscription, profile)) = self.profile_with_subscription()? else {
            return Ok(vec![]);
        };
        let paths = self.paths()?;
        let settings = load_settings(&paths.settings())?;
        let selections = load_group_selections(&paths.group_selections());
        let favorites = load_node_favorites(&paths.node_favorites());
        let groups = if live {
            self.cached_group_heads(&settings)
        } else {
            None
        };
        Ok(profile
            .groups
            .iter()
            .chain(profile.nodes.iter())
            .map(|outbound| {
                let ty = outbound
                    .outbound
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let is_group = matches!(
                    ty.as_str(),
                    "selector" | "urltest" | "fallback" | "loadbalance"
                );
                let members = member_tags(&outbound.outbound);
                let live_now = groups
                    .as_ref()
                    .and_then(|groups| groups.iter().find(|group| group.tag == outbound.tag))
                    .map(|group| group.now.clone())
                    .filter(|now| !now.is_empty());
                let static_now = if ty == "selector" {
                    selector_member_now(&selections, &outbound.tag, &outbound.outbound, &members)
                } else {
                    None
                };
                let favorite_rank = favorites.favorite_rank(subscription, &outbound.tag);
                NodeInfo {
                    tag: outbound.tag.clone(),
                    outbound_type: ty,
                    group_now: live_now
                        .filter(|now| members.iter().any(|member| member == now))
                        .or(static_now)
                        .filter(|_| is_group),
                    group_all: if is_group { Some(members) } else { None },
                    favorite: favorite_rank.is_some(),
                    favorite_rank,
                }
            })
            .collect())
    }

    /// Favorite or unfavorite a tag for the active subscription. A UI
    /// preference: no config rewrite and no tunnel reload.
    pub fn set_node_favorite(&self, tag: &str, favorite: bool) -> Result<(), AppError> {
        let Some((subscription, profile)) = self.profile_with_subscription()? else {
            return Err(AppError::new(
                ErrorCode::ConfigEmptyOutbounds,
                "no active subscription",
            ));
        };
        if !profile.all_outbounds().any(|outbound| outbound.tag == tag) {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                format!("unknown node tag: {tag}"),
            ));
        }
        persist_node_favorite(&self.paths()?.node_favorites(), subscription, tag, favorite)
    }

    pub fn set_selected_node(&mut self, tag: &str) -> Result<(), AppError> {
        let profile = self.profile()?;
        if !profile.all_outbounds().any(|outbound| outbound.tag == tag) {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                format!("unknown node tag: {tag}"),
            ));
        }
        if is_unselectable_group(&profile, tag) {
            return Ok(());
        }
        let paths = self.paths()?.clone();
        let mut settings = load_settings(&paths.settings())?;
        settings.selected_tag = Some(tag.to_string());
        save_settings_for(&paths.settings(), &settings, PLATFORM)?;
        if let Some(group) = selection_group_for(&profile, tag) {
            let mut selections = load_group_selections(&paths.group_selections());
            selections.insert(group, tag.to_string());
            save_group_selections(&paths.group_selections(), &selections)?;
        }
        Ok(())
    }

    /// Remember the group exit for a node pick after the tunnel has applied it.
    pub(crate) fn record_selected_exit(&self, tag: &str) {
        let Ok(profile) = self.profile() else {
            return;
        };
        if let Some(group) = selection_group_for(&profile, tag) {
            self.record_group_now(&group, tag);
        }
    }

    /// Remember a selector member after the tunnel has applied it.
    ///
    /// Fetch locks are held across the write. An in-flight `/proxies` read
    /// publishes first, then this patch replaces its `now`. A sample that
    /// starts afterwards still wins, so urltest and fallback keep moving.
    pub(crate) fn record_group_now(&self, tag: &str, now: &str) {
        let _groups = self
            .groups_fetch
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let _selected = self.now_fetch.lock().unwrap_or_else(|err| err.into_inner());
        *self.now_cache.lock().unwrap_or_else(|err| err.into_inner()) =
            Some((Instant::now(), tag.to_string(), now.to_string()));
        let mut groups = self
            .groups_cache
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if let Some((at, heads)) = groups.as_mut() {
            if let Some(head) = heads.iter_mut().find(|head| head.tag == tag) {
                head.now = now.to_string();
                *at = Instant::now();
            }
        }
    }

    pub fn set_group_selection(&mut self, group: &str, member: &str) -> Result<(), AppError> {
        let profile = self.profile()?;
        let group_outbound = profile
            .groups
            .iter()
            .find(|outbound| outbound.tag == group)
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::ConfigInvalid,
                    format!("unknown strategy group: {group}"),
                )
            })?;
        if group_outbound.outbound.get("type").and_then(|v| v.as_str()) != Some("selector") {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                format!("{group} is not a selector group"),
            ));
        }
        let members = member_tags(&group_outbound.outbound);
        if !members.iter().any(|item| item == member) {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                format!("{member} is not a member of group {group}"),
            ));
        }
        let paths = self.paths()?.clone();
        let mut selections = load_group_selections(&paths.group_selections());
        selections.insert(group.to_string(), member.to_string());
        save_group_selections(&paths.group_selections(), &selections)?;
        Ok(())
    }

    pub fn test_delay(&self, tag: &str, live: bool) -> Result<u32, AppError> {
        if !live {
            return Err(AppError::new(
                ErrorCode::CoreInvalidState,
                "start the VPN before testing delay",
            ));
        }
        let profile = self.profile()?;
        if !profile.all_outbounds().any(|outbound| outbound.tag == tag) {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                format!("unknown node tag: {tag}"),
            ));
        }
        let settings = load_settings(&self.paths()?.settings())?;
        let endpoints = self.endpoints(&settings)?;
        proxy_delay(&endpoints, tag, 5000, DELAY_TEST_URL).map_err(AppError::from)
    }

    pub fn traffic_snapshot(&mut self, live: bool) -> Result<TrafficSnapshot, AppError> {
        self.sync_traffic_live(live);
        Ok(self.traffic.snapshot())
    }

    pub fn traffic_since(
        &mut self,
        cursor: Option<u64>,
        live: bool,
    ) -> Result<TrafficDelta, AppError> {
        self.sync_traffic_live(live);
        Ok(self.traffic.snapshot_since(cursor))
    }

    pub fn log_view(&self, n: usize) -> Result<Vec<String>, AppError> {
        let path = config::core_log_path(self.paths()?);
        Ok(tail_lines(&path, n.min(2000)))
    }

    pub fn runtime_config(&self) -> Result<String, AppError> {
        let path = self.paths()?.config();
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok("{}".into()),
            Err(err) => {
                return Err(AppError::new(
                    ErrorCode::ConfigInvalid,
                    format!("read config: {err}"),
                ))
            }
        };
        redact_config_str(&raw)
            .map_err(|err| AppError::new(ErrorCode::ConfigInvalid, format!("redact config: {err}")))
    }

    pub fn settings(&self) -> Result<AppSettings, AppError> {
        load_settings(&self.paths()?.settings())
    }

    pub fn save_settings(&mut self, patch: &SettingsPatch) -> Result<(), AppError> {
        let paths = self.paths()?.clone();
        let current = load_settings(&paths.settings())?;
        let next = current.apply_patch(patch);
        save_settings_for(&paths.settings(), &next, PLATFORM)
    }

    pub fn set_proxy_mode(&mut self, mode: ProxyMode) -> Result<(), AppError> {
        let paths = self.paths()?.clone();
        let mut settings = load_settings(&paths.settings())?;
        settings.proxy_mode = mode;
        save_settings_for(&paths.settings(), &settings, PLATFORM)
    }

    /// Persist one Android app's routing override. `Follow` clears the entry.
    pub fn set_app_proxy(&mut self, package: &str, mode: AppProxyMode) -> Result<(), AppError> {
        if !is_android_package_name(package) {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                format!("not an Android package name: {package}"),
            ));
        }
        let paths = self.paths()?.clone();
        let mut settings = load_settings(&paths.settings())?;
        if mode == AppProxyMode::Follow {
            settings.app_proxies.remove(package);
        } else {
            settings.app_proxies.insert(package.to_string(), mode);
        }
        save_settings_for(&paths.settings(), &settings, PLATFORM)
    }

    pub fn rule_overview(&self) -> Result<RuleOverview, AppError> {
        let (profile, overrides) = self.rules_state()?;
        let rows = rule_rows(&profile, &overrides);
        let mut types: Vec<RuleTypeCount> = Vec::new();
        for row in rows.iter().filter(|row| !row.custom) {
            if let Some(slot) = types
                .iter_mut()
                .find(|slot| slot.rule_type == row.rule_type)
            {
                slot.count += 1;
            } else {
                types.push(RuleTypeCount {
                    rule_type: row.rule_type.clone(),
                    count: 1,
                });
            }
        }
        types.sort_by(|a, b| b.count.cmp(&a.count).then(a.rule_type.cmp(&b.rule_type)));
        Ok(RuleOverview {
            total: rows.len(),
            disabled: rows
                .iter()
                .filter(|row| !row.custom && row.disabled)
                .count(),
            custom: rows.iter().filter(|row| row.custom).count(),
            rule_sets: profile.route.rule_sets.len(),
            types,
        })
    }

    pub fn list_rules(&self, req: &ListRulesRequest) -> Result<ListRulesResponse, AppError> {
        let (profile, overrides) = self.rules_state()?;
        let keyword = req
            .keyword
            .as_deref()
            .map(str::trim)
            .filter(|keyword| !keyword.is_empty())
            .map(|keyword| keyword.to_ascii_lowercase());
        let rows: Vec<RuleRow> = rule_rows(&profile, &overrides)
            .into_iter()
            .filter(|row| match req.custom {
                Some(true) => row.custom,
                Some(false) => !row.custom,
                None => true,
            })
            .filter(|row| match req.disabled.as_deref() {
                Some("disabled") => row.disabled,
                Some("enabled") => !row.disabled,
                _ => true,
            })
            .filter(|row| match req.rule_type.as_deref() {
                Some(ty) if !ty.is_empty() => row.rule_type == ty,
                _ => true,
            })
            .filter(|row| match &keyword {
                Some(keyword) => serde_json::to_string(&row.rule)
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(keyword),
                None => true,
            })
            .collect();
        let total = rows.len();
        let limit = req.limit.clamp(1, 200);
        let items = rows.into_iter().skip(req.offset).take(limit).collect();
        Ok(ListRulesResponse {
            total,
            offset: req.offset,
            limit,
            items,
        })
    }

    pub fn set_rule_disabled(&mut self, fingerprint: &str, disabled: bool) -> Result<(), AppError> {
        let paths = self.paths()?.clone();
        let profile = self.full_profile()?;
        let mut overrides = load_overrides(&paths, &profile);
        let subscription_rule = profile
            .route
            .rules
            .iter()
            .find(|rule| rule_matches_fingerprint(rule, fingerprint))
            .cloned();
        if let Some(rule) = subscription_rule {
            overrides.set_rule_disabled(&rule, disabled);
        } else if overrides
            .custom
            .iter()
            .any(|rule| rule_matches_fingerprint(rule, fingerprint))
        {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                "custom rules cannot be disabled; edit or remove the rule instead",
            ));
        } else if !disabled {
            overrides.set_disabled(fingerprint.to_string(), false);
        } else {
            return Err(AppError::new(ErrorCode::ConfigInvalid, "rule not found"));
        }
        save_rule_overrides(&paths.rule_overrides(), &overrides)?;
        Ok(())
    }

    pub fn add_custom_rule(&mut self, rule: Value) -> Result<String, AppError> {
        let paths = self.paths()?.clone();
        let profile = self.full_profile()?;
        validate_custom_rule_value(&rule, Some(profile.as_ref()))
            .map_err(|message| AppError::new(ErrorCode::ConfigInvalid, message))?;
        let mut overrides = load_overrides(&paths, &profile);
        let fingerprint = rule_fingerprint(&rule);
        if overrides
            .custom
            .iter()
            .any(|candidate| rule_fingerprint(candidate) == fingerprint)
        {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                "custom rule already exists",
            ));
        }
        overrides.custom.push(rule);
        save_rule_overrides(&paths.rule_overrides(), &overrides)?;
        Ok(fingerprint)
    }

    pub fn update_custom_rule(
        &mut self,
        fingerprint: &str,
        rule: Value,
    ) -> Result<String, AppError> {
        let paths = self.paths()?.clone();
        let profile = self.full_profile()?;
        validate_custom_rule_value(&rule, Some(profile.as_ref()))
            .map_err(|message| AppError::new(ErrorCode::ConfigInvalid, message))?;
        let mut overrides = load_overrides(&paths, &profile);
        let Some(index) = overrides
            .custom
            .iter()
            .position(|candidate| rule_matches_fingerprint(candidate, fingerprint))
        else {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                "custom rule not found",
            ));
        };
        let new_fingerprint = rule_fingerprint(&rule);
        if overrides
            .custom
            .iter()
            .enumerate()
            .any(|(idx, candidate)| idx != index && rule_fingerprint(candidate) == new_fingerprint)
        {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                "custom rule already exists",
            ));
        }
        overrides.custom[index] = rule;
        save_rule_overrides(&paths.rule_overrides(), &overrides)?;
        Ok(new_fingerprint)
    }

    pub fn remove_custom_rule(&mut self, fingerprint: &str) -> Result<(), AppError> {
        let paths = self.paths()?.clone();
        let profile = self.full_profile()?;
        let mut overrides = load_overrides(&paths, &profile);
        overrides.remove_custom(fingerprint);
        save_rule_overrides(&paths.rule_overrides(), &overrides)?;
        Ok(())
    }

    fn sync_traffic_live(&mut self, live: bool) {
        if !live {
            self.traffic.set_endpoints(None);
            return;
        }
        let Ok(paths) = self.paths() else {
            return;
        };
        let Ok(settings) = load_settings(&paths.settings()) else {
            return;
        };
        self.sync_traffic(&settings, TunnelPhase::Connected);
    }

    fn sync_traffic(&mut self, settings: &AppSettings, phase: TunnelPhase) {
        if phase != TunnelPhase::Connected {
            self.traffic.set_endpoints(None);
            return;
        }
        let Ok(paths) = self.paths().cloned() else {
            return;
        };
        let Ok(secret) = ice_config::ensure_clash_api_secret(&paths.clash_api_secret()) else {
            return;
        };
        self.traffic.set_endpoints(Some(
            HealthEndpoints::new(settings.clash_api_listen.clone(), settings.clash_api_port)
                .with_secret(secret),
        ));
    }

    fn fresh_groups(&self) -> Option<Vec<GroupHead>> {
        let slot = self
            .groups_cache
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let (at, groups) = slot.as_ref()?;
        (at.elapsed() < PROXY_GROUPS_TTL).then(|| groups.clone())
    }

    fn fresh_group_now(&self, tag: &str) -> Option<String> {
        self.fresh_groups()?
            .into_iter()
            .find(|group| group.tag == tag)
            .map(|group| group.now)
            .filter(|now| !now.is_empty())
    }

    fn fresh_recorded_now(&self, tag: &str) -> Option<String> {
        let slot = self.now_cache.lock().unwrap_or_else(|err| err.into_inner());
        let (at, cached_tag, now) = slot.as_ref()?;
        (cached_tag == tag && at.elapsed() < PROXY_GROUPS_TTL && !now.is_empty())
            .then(|| now.clone())
    }

    /// Group sample first, then the exit recorded for a selector switch.
    fn preferred_cached_now(&self, tag: &str) -> Option<String> {
        self.fresh_group_now(tag)
            .or_else(|| self.fresh_recorded_now(tag))
    }

    fn cached_group_heads(&self, settings: &AppSettings) -> Option<Vec<GroupHead>> {
        if let Some(groups) = self.fresh_groups() {
            return Some(groups);
        }
        let endpoints = self.endpoints(settings).ok()?;
        self.load_group_heads(|| proxy_group_heads(&endpoints).ok())
    }

    /// Fetch group exits when the sample is missing or older than
    /// [`PROXY_GROUPS_TTL`]. The fetch lock covers the request and the write.
    fn load_group_heads(
        &self,
        fetch: impl FnOnce() -> Option<Vec<GroupHead>>,
    ) -> Option<Vec<GroupHead>> {
        if let Some(groups) = self.fresh_groups() {
            return Some(groups);
        }
        let _fetch = self
            .groups_fetch
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if let Some(groups) = self.fresh_groups() {
            return Some(groups);
        }
        let groups = fetch()?;
        *self
            .groups_cache
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = Some((Instant::now(), groups.clone()));
        Some(groups)
    }

    /// One group's live exit. Does not fetch or retain the proxy map.
    ///
    /// A fresh group sample wins over the recorded exit, so an automatic
    /// urltest or fallback change is visible before [`PROXY_GROUPS_TTL`].
    /// The recorded exit still fills the gap when that sample has no row.
    fn cached_selected_now(&self, settings: &AppSettings, tag: &str) -> Option<String> {
        if let Some(now) = self.preferred_cached_now(tag) {
            return Some(now);
        }
        let _fetch = self.now_fetch.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(now) = self.preferred_cached_now(tag) {
            return Some(now);
        }
        let endpoints = self.endpoints(settings).ok()?;
        let now = proxy_selected_now(&endpoints, tag).ok()?;
        if now.is_empty() {
            return None;
        }
        *self.now_cache.lock().unwrap_or_else(|err| err.into_inner()) =
            Some((Instant::now(), tag.to_string(), now.clone()));
        Some(now)
    }

    /// Home's exit line. Member lists stay on `list_nodes`.
    fn outbound_summary(&self, live: bool) -> (bool, Option<status::SelectedOutbound>) {
        let Ok(profile) = self.profile() else {
            return (false, None);
        };
        let Ok(paths) = self.paths() else {
            return (false, None);
        };
        let settings = load_settings(&paths.settings()).ok();
        let selections = load_group_selections(&paths.group_selections());
        let outbounds: Vec<_> = profile.groups.iter().chain(profile.nodes.iter()).collect();
        if outbounds.is_empty() {
            return (false, None);
        }
        let selected = settings
            .as_ref()
            .and_then(|settings| settings.selected_tag.as_deref())
            .and_then(|tag| {
                outbounds
                    .iter()
                    .copied()
                    .find(|outbound| outbound.tag == tag)
            })
            .or_else(|| outbounds.first().copied());
        let Some(outbound) = selected else {
            return (false, None);
        };
        let ty = outbound
            .outbound
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
            .to_string();
        let is_group = matches!(
            ty.as_str(),
            "selector" | "urltest" | "fallback" | "loadbalance"
        );
        let members = member_tags(&outbound.outbound);
        let static_now = if ty == "selector" {
            selector_member_now(&selections, &outbound.tag, &outbound.outbound, &members)
        } else {
            None
        };
        let group_now = if live && is_group {
            settings
                .as_ref()
                .and_then(|settings| self.cached_selected_now(settings, &outbound.tag))
                .filter(|now| members.iter().any(|member| member == now))
        } else {
            None
        }
        .or(static_now)
        .filter(|_| is_group);
        (
            true,
            Some(status::SelectedOutbound {
                tag: outbound.tag.clone(),
                outbound_type: ty,
                group_now,
            }),
        )
    }

    fn endpoints(&self, settings: &AppSettings) -> Result<HealthEndpoints, AppError> {
        let secret = ice_config::ensure_clash_api_secret(&self.paths()?.clash_api_secret())?;
        Ok(
            HealthEndpoints::new(settings.clash_api_listen.clone(), settings.clash_api_port)
                .with_secret(secret),
        )
    }

    fn manager(&self) -> Result<SubscriptionManager, AppError> {
        Ok(SubscriptionManager::open(
            SubscriptionPaths::from_app(self.paths()?),
            PLATFORM,
        ))
    }

    /// Tags, types, and group members. Connection fields stay on disk.
    fn profile(&self) -> Result<Arc<NormalizedProfile>, AppError> {
        Ok(self
            .profile_with_subscription()?
            .map(|(_, profile)| profile)
            .unwrap_or_else(|| Arc::new(NormalizedProfile::from_nodes_only(vec![]))))
    }

    /// Active subscription id paired with its resident profile. `None` means
    /// no subscription is active; [`Self::profile`] cannot be used for that
    /// check because it turns the missing subscription into an empty profile.
    fn profile_with_subscription(
        &self,
    ) -> Result<Option<(Uuid, Arc<NormalizedProfile>)>, AppError> {
        let paths = self.paths()?;
        let settings = load_settings(&paths.settings())?;
        let sub_paths = SubscriptionPaths::from_app(paths);
        let index = read_index(&sub_paths)?;
        let Some(meta) = active_subscription(&index) else {
            return Ok(None);
        };
        let subscription = meta.id;
        match self.profile_cache.load_resident(
            &sub_paths,
            &index,
            settings.auto_default_rules,
            PLATFORM,
        ) {
            Ok(profile) => Ok(Some((subscription, profile))),
            Err(SubscriptionError::NoActiveSubscription) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Rules and connection fields. Not kept in [`Self::profile_cache`].
    fn full_profile(&self) -> Result<Arc<NormalizedProfile>, AppError> {
        let paths = self.paths()?;
        let settings = load_settings(&paths.settings())?;
        config::active_profile(paths, settings.auto_default_rules)
    }

    fn rules_state(&self) -> Result<(Arc<NormalizedProfile>, RuleOverrides), AppError> {
        let profile = self.full_profile()?;
        let overrides = load_overrides(self.paths()?, &profile);
        Ok((profile, overrides))
    }
}

fn load_overrides(paths: &AppPaths, profile: &NormalizedProfile) -> RuleOverrides {
    let mut overrides = load_rule_overrides(&paths.rule_overrides());
    if !overrides.has_legacy_disabled_fingerprints() {
        return overrides;
    }
    let mut rules = profile.route.rules.clone();
    rules.extend(overrides.custom.clone());
    if overrides.migrate_legacy_fingerprints(rules.iter()) {
        let _ = save_rule_overrides(&paths.rule_overrides(), &overrides);
    }
    overrides
}

fn public_meta(meta: &SubscriptionMeta) -> Result<Value, AppError> {
    let mut value = serde_json::to_value(meta).map_err(|err| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("serialize subscription: {err}"),
        )
    })?;
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "url".into(),
            Value::String(redact_subscription_url_for_ui(&meta.url)),
        );
    }
    Ok(value)
}

/// Selector exit that is still a member. A saved pick of a removed info
/// entry falls through to the group's default, then its first real member.
fn selector_member_now(
    selections: &std::collections::HashMap<String, String>,
    tag: &str,
    outbound: &Value,
    members: &[String],
) -> Option<String> {
    let listed = |candidate: &str| members.iter().any(|member| member == candidate);
    selections
        .get(tag)
        .filter(|selected| listed(selected))
        .cloned()
        .or_else(|| {
            outbound
                .get("default")
                .and_then(|value| value.as_str())
                .filter(|value| listed(value))
                .map(str::to_string)
        })
        .or_else(|| members.first().cloned())
}

fn member_tags(outbound: &Value) -> Vec<String> {
    outbound
        .get("outbounds")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn selection_group_for(profile: &NormalizedProfile, tag: &str) -> Option<String> {
    if profile.groups.is_empty() {
        return None;
    }
    let contains = |group: &ice_config::NormalizedOutbound| {
        group
            .outbound
            .get("outbounds")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str())
                    .any(|member| member == tag)
            })
            .unwrap_or(false)
    };
    if let Some(top) = profile.default_outbound.as_deref() {
        if profile
            .groups
            .iter()
            .any(|group| group.tag == top && contains(group))
        {
            return Some(top.to_string());
        }
    }
    profile
        .groups
        .iter()
        .find(|group| contains(group))
        .map(|group| group.tag.clone())
}

fn is_unselectable_group(profile: &NormalizedProfile, tag: &str) -> bool {
    !profile.groups.is_empty()
        && profile.groups.iter().any(|group| group.tag == tag)
        && selection_group_for(profile, tag).is_none()
}

fn rule_rows(profile: &NormalizedProfile, overrides: &RuleOverrides) -> Vec<RuleRow> {
    let mut rows = Vec::new();
    for rule in &overrides.custom {
        rows.push(rule_row(None, rule, true, overrides));
    }
    for (index, rule) in profile.route.rules.iter().enumerate() {
        rows.push(rule_row(Some(index), rule, false, overrides));
    }
    rows
}

fn rule_row(
    index: Option<usize>,
    rule: &Value,
    custom: bool,
    overrides: &RuleOverrides,
) -> RuleRow {
    RuleRow {
        index,
        fingerprint: rule_fingerprint(rule),
        rule: rule.clone(),
        custom,
        disabled: !custom && overrides.is_rule_disabled(rule),
        rule_type: rule_type_of(rule).to_string(),
    }
}

fn tail_lines(path: &std::path::Path, n: usize) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return Vec::new(),
    };
    let len = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let start = len.saturating_sub(1_000_000);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut buf = String::new();
    if file.read_to_string(&mut buf).is_err() {
        return Vec::new();
    }
    let mut lines: Vec<String> = buf.lines().map(str::to_string).collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    if lines.len() > n {
        lines.split_off(lines.len() - n)
    } else {
        lines
    }
}

#[derive(Debug, Serialize)]
pub struct NodeInfo {
    pub tag: String,
    pub outbound_type: String,
    pub group_now: Option<String>,
    pub group_all: Option<Vec<String>>,
    pub favorite: bool,
    pub favorite_rank: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct RuleTypeCount {
    pub rule_type: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct RuleOverview {
    pub total: usize,
    pub disabled: usize,
    pub custom: usize,
    pub rule_sets: usize,
    pub types: Vec<RuleTypeCount>,
}

#[derive(Debug, serde::Deserialize)]
pub struct ListRulesRequest {
    #[serde(default)]
    pub keyword: Option<String>,
    #[serde(default, rename = "type")]
    pub rule_type: Option<String>,
    #[serde(default)]
    pub disabled: Option<String>,
    #[serde(default)]
    pub custom: Option<bool>,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_page")]
    pub limit: usize,
}

fn default_page() -> usize {
    50
}

#[derive(Debug, Serialize)]
pub struct RuleRow {
    pub index: Option<usize>,
    pub fingerprint: String,
    pub rule: Value,
    pub custom: bool,
    pub disabled: bool,
    pub rule_type: String,
}

#[derive(Debug, Serialize)]
pub struct ListRulesResponse {
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub items: Vec<RuleRow>,
}

#[derive(Debug, Serialize)]
pub struct RemoveResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apply_warning: Option<AppError>,
}

#[derive(Debug, Serialize)]
pub struct RuleMutation {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apply_warning: Option<AppError>,
}

pub fn tunnel_view(phase: &str, permission: &str) -> TunnelView {
    TunnelView {
        phase: TunnelPhase::parse(phase),
        permission: VpnPermission::parse(permission),
    }
}

pub fn stopped_view() -> TunnelView {
    TunnelView {
        phase: TunnelPhase::Stopped,
        permission: VpnPermission::Unknown,
    }
}

#[cfg(test)]
mod group_cache_tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn seed_groups(host: &MobileHost, heads: Vec<GroupHead>) {
        *host
            .groups_cache
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = Some((Instant::now(), heads));
    }

    #[test]
    fn record_group_now_patches_a_fresh_group_head() {
        let host = MobileHost::new();
        seed_groups(
            &host,
            vec![
                GroupHead {
                    tag: "auto".into(),
                    now: "a".into(),
                },
                GroupHead {
                    tag: "proxy".into(),
                    now: "hk".into(),
                },
            ],
        );
        host.record_group_now("proxy", "jp");
        assert_eq!(host.preferred_cached_now("proxy").as_deref(), Some("jp"));
        let groups = host.fresh_groups().expect("fresh groups");
        assert_eq!(groups[0].now, "a");
        assert_eq!(groups[1].now, "jp");
    }

    #[test]
    fn a_fresher_group_sample_wins_over_the_recorded_exit() {
        let host = MobileHost::new();
        *host.now_cache.lock().unwrap_or_else(|err| err.into_inner()) =
            Some((Instant::now(), "proxy".into(), "hk".into()));
        seed_groups(
            &host,
            vec![GroupHead {
                tag: "proxy".into(),
                now: "jp".into(),
            }],
        );
        assert_eq!(host.preferred_cached_now("proxy").as_deref(), Some("jp"));
    }

    #[test]
    fn recorded_exit_fills_a_group_sample_that_lacks_the_tag() {
        let host = MobileHost::new();
        *host.now_cache.lock().unwrap_or_else(|err| err.into_inner()) =
            Some((Instant::now(), "proxy".into(), "jp".into()));
        seed_groups(
            &host,
            vec![GroupHead {
                tag: "auto".into(),
                now: "a".into(),
            }],
        );
        assert_eq!(host.preferred_cached_now("proxy").as_deref(), Some("jp"));
    }

    #[test]
    fn an_in_flight_group_sample_cannot_overwrite_the_recorded_exit() {
        let host = Arc::new(MobileHost::new());
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let worker = {
            let host = Arc::clone(&host);
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            thread::spawn(move || {
                host.load_group_heads(|| {
                    entered.wait();
                    release.wait();
                    Some(vec![GroupHead {
                        tag: "proxy".into(),
                        now: "hk".into(),
                    }])
                })
            })
        };
        entered.wait();
        let recorder = {
            let host = Arc::clone(&host);
            thread::spawn(move || host.record_group_now("proxy", "jp"))
        };
        release.wait();
        worker.join().expect("fetch");
        recorder.join().expect("record");
        assert_eq!(host.preferred_cached_now("proxy").as_deref(), Some("jp"));
        let groups = host.fresh_groups().expect("fresh groups");
        assert_eq!(groups[0].now, "jp");
    }
}

#[cfg(test)]
mod favorite_tests {
    use super::*;
    use ice_config::load_node_favorites;
    use ice_subscription::{write_subscription_success, SubscriptionFormat, SubscriptionSource};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn host_with_subscription(label: &str) -> (MobileHost, Uuid, AppPaths) {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-mobile-favorites-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = AppPaths::new(&dir);
        paths.ensure_dirs().unwrap();
        let mut host = MobileHost::new();
        host.ensure_paths(AppPaths::new(&dir), None).unwrap();

        let id = Uuid::new_v4();
        let meta = SubscriptionMeta {
            id,
            name: "test".into(),
            url: "https://example.com/sub".into(),
            active: true,
            format: SubscriptionFormat::SingBox,
            node_count: 2,
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
            source: SubscriptionSource::Remote,
        };
        let profile = ice_config::NormalizedProfile::from_nodes_only(vec![
            ice_config::NormalizedOutbound {
                tag: "n1".into(),
                outbound: Arc::new(
                    serde_json::json!({"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}),
                ),
            },
            ice_config::NormalizedOutbound {
                tag: "n2".into(),
                outbound: Arc::new(
                    serde_json::json!({"type":"socks","tag":"n2","server":"1.1.1.1","server_port":2}),
                ),
            },
        ]);
        write_subscription_success(&SubscriptionPaths::from_app(&paths), &meta, "{}", &profile)
            .unwrap();
        (host, id, paths)
    }

    #[test]
    fn list_nodes_marks_favorites_and_toggles_do_not_rewrite_the_config() {
        let (host, id, paths) = host_with_subscription("mark");
        std::fs::write(paths.config(), b"runtime-config").unwrap();

        let nodes = host.list_nodes(false).unwrap();
        assert!(nodes.iter().all(|node| !node.favorite));

        let err = host.set_node_favorite("missing", true).unwrap_err();
        assert_eq!(err.code, ErrorCode::ConfigInvalid.as_str());

        host.set_node_favorite("n2", true).unwrap();
        let nodes = host.list_nodes(false).unwrap();
        let n2 = nodes.iter().find(|node| node.tag == "n2").unwrap();
        assert!(n2.favorite);
        assert_eq!(n2.favorite_rank, Some(0));

        host.set_node_favorite("n1", true).unwrap();
        let nodes = host.list_nodes(false).unwrap();
        let n1 = nodes.iter().find(|node| node.tag == "n1").unwrap();
        let n2 = nodes.iter().find(|node| node.tag == "n2").unwrap();
        assert_eq!(n1.favorite_rank, Some(0));
        assert_eq!(n2.favorite_rank, Some(1));
        assert!(load_node_favorites(&paths.node_favorites()).contains(id, "n1"));
        assert_eq!(
            std::fs::read_to_string(paths.config()).unwrap(),
            "runtime-config",
            "a favorite must not rewrite the tunnel config"
        );
        let _ = std::fs::remove_dir_all(paths.root());
    }

    #[test]
    fn no_active_subscription_returns_empty_outbounds_and_removal_purges() {
        let (host, id, paths) = host_with_subscription("purge");
        host.set_node_favorite("n1", true).unwrap();
        host.remove_subscription(id).unwrap();
        assert!(load_node_favorites(&paths.node_favorites())
            .tags(id)
            .is_empty());

        let err = host.set_node_favorite("n1", true).unwrap_err();
        assert_eq!(err.code, ErrorCode::ConfigEmptyOutbounds.as_str());
        assert!(host.list_nodes(false).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(paths.root());
    }

    #[test]
    fn custom_rules_use_the_shared_validator() {
        let (mut host, _id, paths) = host_with_subscription("custom-validation");

        let err = host
            .add_custom_rule(serde_json::json!({ "geoip": ["cn"], "outbound": "direct" }))
            .unwrap_err();
        assert!(
            err.message.contains("sing-box 1.13 removed it"),
            "{}",
            err.message
        );

        let custom = serde_json::json!({ "domain": ["a.com"], "outbound": "direct" });
        let fingerprint = host.add_custom_rule(custom.clone()).unwrap();
        let err = host.add_custom_rule(custom).unwrap_err();
        assert!(err.message.contains("already exists"), "{}", err.message);

        let err = host
            .update_custom_rule(
                &fingerprint,
                serde_json::json!({ "rule_set": ["missing"], "outbound": "direct" }),
            )
            .unwrap_err();
        assert!(err.message.contains("unknown rule_set"), "{}", err.message);
        let _ = std::fs::remove_dir_all(paths.root());
    }
}
