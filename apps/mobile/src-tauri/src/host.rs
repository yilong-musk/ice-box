// SPDX-License-Identifier: GPL-3.0-or-later

//! Host-process state: subscriptions, settings, and the config file the
//! tunnel process reads. Plugin calls stay in the commands.

use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

use ice_config::{
    load_group_selections, load_rule_overrides, load_settings, redact_config_str, rule_fingerprint,
    rule_matches_fingerprint, rule_type_of, save_group_selections, save_rule_overrides,
    save_settings_for, set_proxy_service_enabled_for, AppError, AppPaths, AppSettings, ErrorCode,
    NormalizedProfile, ProxyMode, RuleOverrides, SettingsPatch,
};
use ice_core::{
    proxy_delay, proxy_groups, HealthEndpoints, TrafficDelta, TrafficMonitor, TrafficSnapshot,
    DELAY_TEST_URL,
};
use ice_subscription::{
    read_index, redact_subscription_url_for_ui, AutoUpdateInterval, SubscriptionManager,
    SubscriptionMeta, SubscriptionPaths,
};
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::config::{self, PLATFORM};
use crate::status::{self, StatusResponse, TunnelPhase, TunnelView, VpnPermission};

pub struct MobileHost {
    paths: Option<AppPaths>,
    traffic: TrafficMonitor,
}

impl MobileHost {
    pub fn new() -> Self {
        Self {
            paths: None,
            traffic: TrafficMonitor::new(),
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
    ) -> Result<StatusResponse, AppError> {
        let paths = self.paths()?.clone();
        let settings = load_settings(&paths.settings())?;
        self.sync_traffic(&settings, view.phase);
        let count = read_index(&SubscriptionPaths::from_app(&paths))
            .map(|index| index.items.len())
            .unwrap_or(0);
        Ok(status::status_response(view, message, memory_bytes, count))
    }

    pub fn paths_ready(&self) -> bool {
        self.paths.is_some()
    }

    pub fn set_service_enabled(&mut self, enabled: bool) -> Result<(), AppError> {
        let paths = self.paths()?;
        set_proxy_service_enabled_for(&paths.settings(), enabled, PLATFORM)?;
        if !enabled {
            self.traffic.set_endpoints(None);
        }
        Ok(())
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

    pub fn add_subscription(
        &self,
        url: &str,
        name: Option<&str>,
        auto_update: bool,
        interval: Option<AutoUpdateInterval>,
    ) -> Result<Value, AppError> {
        let meta = self.manager()?.add(url, name, auto_update, interval)?;
        public_meta(&meta)
    }

    pub fn update_subscription(&self, id: Uuid) -> Result<Value, AppError> {
        let meta = self.manager()?.update(id)?;
        public_meta(&meta)
    }

    pub fn update_all(&self) -> Result<Value, AppError> {
        let results = self.manager()?.update_all();
        let items: Vec<Value> = results
            .into_iter()
            .map(|(id, result)| match result {
                Ok(_) => json!({ "id": id, "ok": true }),
                Err(err) => json!({ "id": id, "ok": false, "error": err.to_string() }),
            })
            .collect();
        Ok(Value::Array(items))
    }

    pub fn remove_subscription(&self, id: Uuid) -> Result<(), AppError> {
        self.manager()?.remove(id)?;
        Ok(())
    }

    pub fn set_active(&self, id: Uuid, active: bool) -> Result<Value, AppError> {
        let meta = self.manager()?.set_active(id, active)?;
        public_meta(&meta)
    }

    pub fn set_auto_update(
        &self,
        id: Uuid,
        auto_update: bool,
        interval: Option<AutoUpdateInterval>,
    ) -> Result<Value, AppError> {
        let meta = self.manager()?.set_auto_update(id, auto_update, interval)?;
        public_meta(&meta)
    }

    pub fn list_nodes(&self, live: bool) -> Result<Vec<NodeInfo>, AppError> {
        let profile = self.profile()?;
        let paths = self.paths()?;
        let settings = load_settings(&paths.settings())?;
        let selections = load_group_selections(&paths.group_selections());
        let groups = if live {
            self.endpoints(&settings)
                .ok()
                .and_then(|endpoints| proxy_groups(&endpoints).ok())
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
                let live_group = groups
                    .as_ref()
                    .and_then(|groups| groups.iter().find(|group| group.tag == outbound.tag));
                let static_now = if ty == "selector" {
                    selections
                        .get(&outbound.tag)
                        .cloned()
                        .or_else(|| {
                            outbound
                                .outbound
                                .get("default")
                                .and_then(|v| v.as_str())
                                .map(str::to_string)
                        })
                        .or_else(|| members.first().cloned())
                } else {
                    None
                };
                NodeInfo {
                    tag: outbound.tag.clone(),
                    outbound_type: ty,
                    group_now: live_group
                        .map(|group| group.now.clone())
                        .filter(|now| !now.is_empty())
                        .or(static_now)
                        .filter(|_| is_group),
                    group_all: if is_group {
                        Some(live_group.map(|group| group.all.clone()).unwrap_or(members))
                    } else {
                        None
                    },
                }
            })
            .collect())
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
            disabled: rows.iter().filter(|row| row.disabled).count(),
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
        let profile = self.profile()?;
        let mut overrides = load_overrides(&paths, &profile);
        match find_rule(&profile, &overrides, fingerprint) {
            Some(rule) => overrides.set_rule_disabled(&rule, disabled),
            None if !disabled => overrides.set_disabled(fingerprint.to_string(), false),
            None => return Err(AppError::new(ErrorCode::ConfigInvalid, "rule not found")),
        }
        save_rule_overrides(&paths.rule_overrides(), &overrides)?;
        Ok(())
    }

    pub fn add_custom_rule(&mut self, rule: Value) -> Result<String, AppError> {
        if !rule.is_object() {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                "a custom rule must be a JSON object",
            ));
        }
        let paths = self.paths()?.clone();
        let profile = self.profile()?;
        let mut overrides = load_overrides(&paths, &profile);
        let fingerprint = rule_fingerprint(&rule);
        overrides.custom.push(rule);
        save_rule_overrides(&paths.rule_overrides(), &overrides)?;
        Ok(fingerprint)
    }

    pub fn remove_custom_rule(&mut self, fingerprint: &str) -> Result<(), AppError> {
        let paths = self.paths()?.clone();
        let profile = self.profile()?;
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

    fn profile(&self) -> Result<Arc<NormalizedProfile>, AppError> {
        let paths = self.paths()?;
        let settings = load_settings(&paths.settings())?;
        config::active_profile(paths, settings.auto_default_rules)
    }

    fn rules_state(&self) -> Result<(Arc<NormalizedProfile>, RuleOverrides), AppError> {
        let profile = self.profile()?;
        let overrides = load_overrides(self.paths()?, &profile);
        Ok((profile, overrides))
    }
}

fn load_overrides(paths: &AppPaths, profile: &NormalizedProfile) -> RuleOverrides {
    let mut overrides = load_rule_overrides(&paths.rule_overrides());
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

fn find_rule(
    profile: &NormalizedProfile,
    overrides: &RuleOverrides,
    fingerprint: &str,
) -> Option<Value> {
    overrides
        .custom
        .iter()
        .chain(profile.route.rules.iter())
        .find(|rule| rule_matches_fingerprint(rule, fingerprint))
        .cloned()
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
        disabled: overrides.is_rule_disabled(rule),
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
