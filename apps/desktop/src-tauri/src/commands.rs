//! Tauri IPC commands (architecture §14).

use crate::core_watch::reconcile_unexpected_core_exit;
use crate::orchestrate::{
    current_settings, endpoints_from_settings, generate_config, orchestrate_apply,
    orchestrate_start, orchestrate_stop, resolve_binary,
};
use crate::shutdown::graceful_stop;
use crate::AppState;
use ice_config::NormalizedOutbound;
use ice_config::{
    load_group_selections, load_rule_overrides, redact_config_str, rule_fingerprint, rule_type_of,
    save_group_selections, save_rule_overrides, save_settings as persist_settings, AppError,
    AppSettings, ErrorCode, NormalizedProfile, ProxyMode, RuleOverrides,
};
use ice_core::{
    connection_stats, proxy_delay, proxy_groups, select_group, select_outbound, traffic_sample,
    ConnectionStats, CoreState, CoreStatus, HealthEndpoints, TrafficSample, DELAY_TEST_URL,
};
use ice_proxy_sys::is_proxy_live_applied;
use ice_subscription::{
    list_profile_outbounds, load_index, redact_subscription_url_for_ui, SubscriptionError,
    SubscriptionManager, SubscriptionPaths,
};
use serde::{Deserialize, Serialize};
use std::sync::MutexGuard;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

fn lock_poisoned(context: &str) -> AppError {
    AppError::new(
        ErrorCode::ConfigInvalid,
        format!("internal lock poisoned: {context}"),
    )
}

fn lock_orchestrate(state: &AppState) -> Result<MutexGuard<'_, ()>, AppError> {
    state
        .orchestrate
        .lock()
        .map_err(|_| lock_poisoned("orchestrate"))
}

fn resource_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().resource_dir().ok()
}

fn binary_for(app: &AppHandle) -> Result<std::path::PathBuf, AppError> {
    resolve_binary(resource_dir(app).as_deref())
}

fn require_running_core(state: &AppState) -> Result<(), AppError> {
    let core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
    if core.state().status != CoreStatus::Running {
        return Err(AppError::new(
            ErrorCode::CoreInvalidState,
            "operation requires running core",
        ));
    }
    Ok(())
}

fn clash_endpoints(settings: &AppSettings) -> HealthEndpoints {
    HealthEndpoints {
        host: settings.clash_api_listen.clone(),
        port: settings.clash_api_port,
    }
}

/// Join-error mapping for `spawn_blocking` (blocking work must not run on the
/// main thread — sync commands freeze the UI event loop).
fn blocking_join_err<E: std::fmt::Display>(context: &str) -> impl FnOnce(E) -> AppError {
    let context = context.to_string();
    move |e| AppError::new(ErrorCode::ConfigInvalid, format!("{context}: {e}"))
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub core: CoreState,
    pub subscription_count: usize,
    pub proxy_recovery_warning: Option<String>,
    /// When auto system proxy is enabled and core is running: `true` = applied, `false` = pending.
    pub system_proxy_applied: Option<bool>,
}

/// How long a `system_proxy_applied` check result is reused. The check spawns
/// `networksetup` subprocesses (list + 4 gets per service); status is polled every 2s
/// by two components, so caching keeps the subprocess storm away while the result stays
/// fresh enough for the "proxy syncing…" indicator.
const PROXY_APPLIED_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(2);

/// Live check of `is_proxy_live_applied`, memoized per endpoints for `PROXY_APPLIED_CACHE_TTL`.
fn cached_system_proxy_applied(state: &AppState, settings: &AppSettings) -> Option<bool> {
    if !settings.auto_set_system_proxy {
        return None;
    }
    let endpoints = endpoints_from_settings(settings);
    let now = std::time::Instant::now();
    let mut cache = state.proxy_applied_cache.lock().ok()?;
    if let Some((cached_endpoints, at, value)) = cache.as_ref() {
        if cached_endpoints == &endpoints && now.duration_since(*at) < PROXY_APPLIED_CACHE_TTL {
            return Some(*value);
        }
    }
    let proxy = state.proxy.lock().ok()?;
    let value =
        is_proxy_live_applied(proxy.as_ref(), &state.paths.proxy_backup(), &endpoints);
    *cache = Some((endpoints, now, value));
    Some(value)
}

fn active_profile(state: &AppState) -> Result<NormalizedProfile, AppError> {
    let sub_paths = SubscriptionPaths::from_app(&state.paths);
    let index = load_index(&sub_paths).map_err(AppError::from)?;
    match ice_subscription::load_active_profile(&sub_paths, &index) {
        Ok(profile) => Ok(profile),
        Err(SubscriptionError::NoActiveSubscription) => Err(AppError::new(
            ErrorCode::ConfigEmptyOutbounds,
            "no active subscription",
        )),
        Err(err) => Err(AppError::from(err)),
    }
}

fn merged_outbounds(state: &AppState) -> Result<Vec<NormalizedOutbound>, AppError> {
    Ok(list_profile_outbounds(&active_profile(state)?))
}

fn require_known_node_tag(state: &AppState, tag: &str) -> Result<(), AppError> {
    let outbounds = merged_outbounds(state)?;
    if !outbounds.iter().any(|o| o.tag == tag) {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            format!("unknown node tag: {tag}"),
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> Result<StatusResponse, AppError> {
    reconcile_unexpected_core_exit(state.inner());
    let core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
    let paths = SubscriptionPaths::from_app(&state.paths);
    let count = ice_subscription::load_index(&paths)
        .map(|i| i.items.len())
        .unwrap_or(0);
    let proxy_recovery_warning = state
        .proxy_recovery_warning
        .lock()
        .ok()
        .and_then(|g| g.clone());
    let system_proxy_applied = if core.state().status == CoreStatus::Running {
        current_settings(&state.paths)
            .ok()
            .and_then(|settings| cached_system_proxy_applied(state.inner(), &settings))
    } else {
        None
    };
    Ok(StatusResponse {
        core: core.state(),
        subscription_count: count,
        proxy_recovery_warning,
        system_proxy_applied,
    })
}

#[tauri::command]
pub fn list_subscriptions(state: State<'_, AppState>) -> Result<serde_json::Value, AppError> {
    let paths = SubscriptionPaths::from_app(&state.paths);
    let index = ice_subscription::load_index(&paths).map_err(AppError::from)?;
    let public: Vec<serde_json::Value> = index
        .items
        .iter()
        .map(|meta| {
            let mut value = serde_json::to_value(meta).map_err(|e| {
                AppError::new(
                    ErrorCode::ConfigInvalid,
                    format!("serialize subscription: {e}"),
                )
            })?;
            if let Some(obj) = value.as_object_mut() {
                obj.insert(
                    "url".into(),
                    serde_json::Value::String(redact_subscription_url_for_ui(&meta.url)),
                );
            }
            Ok(value)
        })
        .collect::<Result<_, AppError>>()?;
    serde_json::to_value(public).map_err(|e| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("serialize subscriptions: {e}"),
        )
    })
}

#[tauri::command]
pub fn start(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    let _orch = lock_orchestrate(&state)?;
    let settings = current_settings(&state.paths)?;
    let binary = binary_for(&app)?;
    let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
    let proxy = state.proxy.lock().map_err(|_| lock_poisoned("proxy"))?;
    orchestrate_start(
        &state.paths,
        &settings,
        &mut **core,
        proxy.as_ref(),
        binary,
        resource_dir(&app).as_deref(),
    )?;
    if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
        *slot = None;
    }
    if let Ok(mut cache) = state.proxy_applied_cache.lock() {
        *cache = None;
    }
    Ok(())
}

#[tauri::command]
pub fn stop(state: State<'_, AppState>) -> Result<(), AppError> {
    graceful_stop(state.inner())
}

#[derive(Deserialize)]
pub struct LogViewRequest {
    pub n: usize,
}

#[tauri::command]
pub fn get_log_view(
    state: State<'_, AppState>,
    req: LogViewRequest,
) -> Result<Vec<String>, AppError> {
    crate::log_view::read_log_view(&state.paths.app_log(), &state.paths.core_log(), req.n)
}

#[tauri::command]
pub fn get_runtime_config(state: State<'_, AppState>) -> Result<String, AppError> {
    let path = state.paths.config();
    if !path.exists() {
        return Ok(String::new());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("read config: {e}")))?;
    redact_config_str(&raw).map_err(|e| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("redact runtime config: {e}"),
        )
    })
}

#[tauri::command]
pub fn reveal_data_dir(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(state.paths.root().to_string_lossy(), None::<&str>)
        .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("reveal data dir: {e}")))?;
    Ok(())
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, AppError> {
    current_settings(&state.paths)
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<(), AppError> {
    let _orch = lock_orchestrate(&state)?;
    let previous = current_settings(&state.paths).unwrap_or_default();
    persist_settings(&state.paths.settings(), &settings)?;
    apply_after_change(&app, &state, &settings, &previous, false)
}

#[derive(Deserialize)]
pub struct SetProxyModeRequest {
    /// `"rule"` | `"global"` | `"direct"`.
    pub mode: String,
}

fn parse_proxy_mode(mode: &str) -> Result<ProxyMode, AppError> {
    match mode {
        "rule" => Ok(ProxyMode::Rule),
        "global" => Ok(ProxyMode::Global),
        "direct" => Ok(ProxyMode::Direct),
        other => Err(AppError::new(
            ErrorCode::ConfigInvalid,
            format!("unknown proxy mode: {other}"),
        )),
    }
}

/// Switch routing mode: persists `settings.proxy_mode`, regenerates config and hot
/// reloads the core when running (rules are stripped in global / direct mode).
#[tauri::command]
pub fn set_proxy_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    req: SetProxyModeRequest,
) -> Result<(), AppError> {
    let _orch = lock_orchestrate(&state)?;
    let mode = parse_proxy_mode(&req.mode)?;
    let previous = current_settings(&state.paths)?;
    if previous.proxy_mode == mode {
        return Ok(());
    }
    let mut settings = previous.clone();
    settings.proxy_mode = mode;
    persist_settings(&state.paths.settings(), &settings)?;
    apply_after_change(&app, &state, &settings, &previous, false)
}

/// When `soft_empty` is true (subscription mutations), empty outbounds while Stopped is Ok;
/// while Running we Stop. Explicit Apply / save_settings should pass false to surface the error.
fn apply_after_change(
    app: &AppHandle,
    state: &AppState,
    settings: &AppSettings,
    previous_settings: &AppSettings,
    soft_empty: bool,
) -> Result<(), AppError> {
    let binary = binary_for(app)?;
    let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
    let proxy = state.proxy.lock().map_err(|_| lock_poisoned("proxy"))?;
    match orchestrate_apply(
        &state.paths,
        settings,
        previous_settings,
        &mut **core,
        proxy.as_ref(),
        binary,
        resource_dir(app).as_deref(),
    ) {
        Ok(()) => Ok(()),
        Err(err) if err.code == "config.empty_outbounds" => {
            if matches!(
                core.state().status,
                CoreStatus::Running | CoreStatus::Starting | CoreStatus::Error
            ) {
                orchestrate_stop(&state.paths, &mut **core, proxy.as_ref())?;
                return Err(AppError::new(
                    ErrorCode::CoreStoppedNoNodes,
                    "内核已停止：没有可用的订阅节点",
                ));
            }
            if soft_empty {
                Ok(())
            } else {
                Err(err)
            }
        }
        Err(err) => Err(err),
    }
}

fn apply_after_subscription_change(
    app: &AppHandle,
    state: &AppState,
    settings: &AppSettings,
) -> Option<AppError> {
    match apply_after_change(app, state, settings, settings, true) {
        Ok(()) => None,
        Err(err) => {
            tracing::warn!(code = %err.code, error = %err.message, "apply after subscription change failed");
            Some(err)
        }
    }
}

fn attach_apply_warning(value: &mut serde_json::Value, warning: Option<AppError>) {
    if let Some(w) = warning {
        value["apply_warning"] = serde_json::json!({
            "code": w.code,
            "message": w.message,
        });
    }
}

#[derive(Deserialize)]
pub struct AddSubscriptionRequest {
    pub url: String,
    pub name: Option<String>,
}

#[tauri::command]
pub fn add_subscription(
    app: AppHandle,
    state: State<'_, AppState>,
    req: AddSubscriptionRequest,
) -> Result<serde_json::Value, AppError> {
    let _orch = lock_orchestrate(&state)?;
    let paths = SubscriptionPaths::from_app(&state.paths);
    let mgr = SubscriptionManager::open(paths);
    let meta = mgr
        .add(&req.url, req.name.as_deref())
        .map_err(AppError::from)?;

    let settings = current_settings(&state.paths)?;
    let apply_warning = apply_after_subscription_change(&app, &state, &settings);
    let mut value = serde_json::to_value(meta)
        .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("serialize: {e}")))?;
    attach_apply_warning(&mut value, apply_warning);
    Ok(value)
}

#[derive(Deserialize)]
pub struct IdRequest {
    pub id: Uuid,
}

#[tauri::command]
pub fn remove_subscription(
    app: AppHandle,
    state: State<'_, AppState>,
    req: IdRequest,
) -> Result<serde_json::Value, AppError> {
    let _orch = lock_orchestrate(&state)?;
    let paths = SubscriptionPaths::from_app(&state.paths);
    ice_subscription::remove_subscription(&paths, req.id).map_err(AppError::from)?;

    let settings = current_settings(&state.paths)?;
    let apply_warning = apply_after_subscription_change(&app, &state, &settings);
    let mut value = serde_json::json!({ "ok": true });
    attach_apply_warning(&mut value, apply_warning);
    Ok(value)
}

#[tauri::command]
pub fn update_subscription(
    app: AppHandle,
    state: State<'_, AppState>,
    req: IdRequest,
) -> Result<serde_json::Value, AppError> {
    let _orch = lock_orchestrate(&state)?;
    let paths = SubscriptionPaths::from_app(&state.paths);
    let mgr = SubscriptionManager::open(paths);
    let meta = mgr.update(req.id).map_err(AppError::from)?;

    let settings = current_settings(&state.paths)?;
    let apply_warning = apply_after_subscription_change(&app, &state, &settings);
    let mut value = serde_json::to_value(meta)
        .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("serialize: {e}")))?;
    attach_apply_warning(&mut value, apply_warning);
    Ok(value)
}

#[tauri::command]
pub fn update_all_subscriptions(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, AppError> {
    // Fetches (parallel, up to one FETCH_TIMEOUT) run without the orchestrate lock so a
    // long batch doesn't queue Start/Stop/Settings behind it; the lock is re-acquired
    // for the final Apply step. Subscription writes are atomic file renames, so
    // concurrent readers see a consistent snapshot.
    let paths = SubscriptionPaths::from_app(&state.paths);
    let mgr = SubscriptionManager::open(paths);
    let results = mgr.update_all();

    let _orch = lock_orchestrate(&state)?;
    let settings = current_settings(&state.paths)?;
    let apply_warning = apply_after_subscription_change(&app, &state, &settings);
    let summary: Vec<_> = results
        .into_iter()
        .map(|(id, r)| {
            serde_json::json!({
                "id": id,
                "ok": r.is_ok(),
                "error": r.err().map(|e| e.to_string()),
            })
        })
        .collect();
    let mut value = serde_json::json!({ "results": summary });
    attach_apply_warning(&mut value, apply_warning);
    Ok(value)
}

#[derive(Deserialize)]
pub struct SetActiveRequest {
    pub id: Uuid,
    pub active: bool,
}

#[tauri::command]
pub fn set_active_subscription(
    app: AppHandle,
    state: State<'_, AppState>,
    req: SetActiveRequest,
) -> Result<serde_json::Value, AppError> {
    let _orch = lock_orchestrate(&state)?;
    let paths = SubscriptionPaths::from_app(&state.paths);
    let meta = ice_subscription::set_active(&paths, req.id, req.active).map_err(AppError::from)?;

    let settings = current_settings(&state.paths)?;
    let apply_warning = apply_after_subscription_change(&app, &state, &settings);
    let mut value = serde_json::to_value(meta)
        .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("serialize: {e}")))?;
    attach_apply_warning(&mut value, apply_warning);
    Ok(value)
}

#[tauri::command]
pub fn apply_subscriptions(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    let _orch = lock_orchestrate(&state)?;
    let settings = current_settings(&state.paths)?;
    apply_after_change(&app, &state, &settings, &settings, false)
}

#[derive(Serialize)]
pub struct NodeInfo {
    pub tag: String,
    pub outbound_type: String,
    /// Live member currently used by a strategy group (Clash API `now`), when core running.
    pub group_now: Option<String>,
    /// Live member tags of a strategy group, when core running.
    pub group_all: Option<Vec<String>>,
}

#[tauri::command]
pub async fn list_nodes(state: State<'_, AppState>) -> Result<Vec<NodeInfo>, AppError> {
    let outbounds = merged_outbounds(state.inner())?;
    let settings = current_settings(&state.paths)?;
    let selections = load_group_selections(&state.paths.group_selections());
    let live = if {
        let core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
        core.state().status == CoreStatus::Running
    } {
        let endpoints = clash_endpoints(&settings);
        tauri::async_runtime::spawn_blocking(move || proxy_groups(&endpoints).ok())
            .await
            .map_err(blocking_join_err("list_nodes proxy_groups"))?
    } else {
        None
    };
    Ok(outbounds
        .iter()
        .map(|o| {
            let ty = o
                .outbound
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let is_group = ["selector", "urltest", "fallback", "loadbalance"]
                .iter()
                .any(|g| g == &ty);
            let live_state = live
                .as_ref()
                .and_then(|groups| groups.iter().find(|g| g.tag == o.tag));
            let static_members: Vec<String> = o
                .outbound
                .get("outbounds")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|m| m.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let static_now = if ty == "selector" {
                selections
                    .get(&o.tag)
                    .cloned()
                    .or_else(|| {
                        o.outbound
                            .get("default")
                            .and_then(|v| v.as_str())
                            .map(String::from)
                    })
                    .or_else(|| static_members.first().cloned())
            } else {
                None
            };
            NodeInfo {
                tag: o.tag.clone(),
                outbound_type: ty,
                group_now: live_state
                    .map(|g| g.now.clone())
                    .filter(|n| !n.is_empty())
                    .or(static_now)
                    .filter(|_| is_group),
                group_all: if is_group {
                    Some(live_state.map(|g| g.all.clone()).unwrap_or(static_members))
                } else {
                    None
                },
            }
        })
        .collect())
}

#[derive(Serialize)]
pub struct RuleTypeCount {
    pub rule_type: String,
    pub count: usize,
}

#[derive(Serialize)]
pub struct RuleOverview {
    pub total: usize,
    /// Disabled fingerprints that match a current rule (subscription or custom).
    pub disabled: usize,
    pub custom: usize,
    pub rule_sets: usize,
    /// Subscription rule counts by classified type, most frequent first.
    pub types: Vec<RuleTypeCount>,
}

#[derive(Deserialize)]
pub struct ListRulesRequest {
    /// Case-insensitive substring match over the rule JSON.
    #[serde(default)]
    pub keyword: Option<String>,
    /// Classified rule type (one of `rule_type_of` keys); None = all types.
    #[serde(default, rename = "type")]
    pub rule_type: Option<String>,
    /// `"all"` (default) | `"disabled"` | `"enabled"`.
    #[serde(default)]
    pub disabled: Option<String>,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_rules_page_size")]
    pub limit: usize,
}

fn default_rules_page_size() -> usize {
    50
}

pub const MAX_RULES_PAGE_SIZE: usize = 200;

#[derive(Serialize)]
pub struct RuleRow {
    /// Position in the active subscription's `route.rules`; None for custom rules.
    pub index: Option<usize>,
    pub fingerprint: String,
    pub rule: serde_json::Value,
    pub custom: bool,
    pub disabled: bool,
    pub rule_type: String,
}

#[derive(Serialize)]
pub struct ListRulesResponse {
    /// Count of rules matching the filters (before pagination).
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub items: Vec<RuleRow>,
}

fn load_overrides(state: &AppState) -> RuleOverrides {
    load_rule_overrides(&state.paths.rule_overrides())
}

fn rule_exists(profile: &NormalizedProfile, overrides: &RuleOverrides, fingerprint: &str) -> bool {
    profile
        .route
        .rules
        .iter()
        .any(|r| rule_fingerprint(r) == fingerprint)
        || overrides
            .custom
            .iter()
            .any(|r| rule_fingerprint(r) == fingerprint)
}

/// Persist rule overrides then Apply (hot reload when running), like subscription mutations.
fn apply_after_rule_change(
    app: &AppHandle,
    state: &AppState,
) -> Result<Option<AppError>, AppError> {
    let settings = current_settings(&state.paths)?;
    Ok(apply_after_subscription_change(app, state, &settings))
}

/// Rules for the active subscription only (single-active model, architecture §11.5).
fn rule_overview(state: &AppState) -> Result<RuleOverview, AppError> {
    let profile = active_profile(state)?;
    let overrides = load_overrides(state);
    let mut counts: std::collections::HashMap<&'static str, usize> =
        std::collections::HashMap::new();
    let mut disabled = 0usize;
    for rule in &profile.route.rules {
        let fp = rule_fingerprint(rule);
        if overrides.is_disabled(&fp) {
            disabled += 1;
        }
        *counts.entry(rule_type_of(rule)).or_default() += 1;
    }
    for rule in &overrides.custom {
        if overrides.is_disabled(&rule_fingerprint(rule)) {
            disabled += 1;
        }
    }
    let mut types: Vec<RuleTypeCount> = counts
        .into_iter()
        .map(|(rule_type, count)| RuleTypeCount {
            rule_type: rule_type.to_string(),
            count,
        })
        .collect();
    types.sort_by(|a, b| b.count.cmp(&a.count).then(a.rule_type.cmp(&b.rule_type)));
    Ok(RuleOverview {
        total: profile.route.rules.len(),
        disabled,
        custom: overrides.custom.len(),
        rule_sets: profile.route.rule_sets.len(),
        types,
    })
}

#[tauri::command]
pub fn get_rule_overview(state: State<'_, AppState>) -> Result<RuleOverview, AppError> {
    rule_overview(state.inner())
}

/// Query rules with server-side filtering + pagination. Never ships the full rule list
/// over IPC: big subscriptions (up to 10k rules) stay cheap for the UI.
fn query_rules(state: &AppState, req: &ListRulesRequest) -> Result<ListRulesResponse, AppError> {
    let profile = active_profile(state)?;
    let overrides = load_overrides(state);
    let limit = req.limit.clamp(1, MAX_RULES_PAGE_SIZE);
    let keyword = req
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(str::to_ascii_lowercase);
    let want_disabled = match req.disabled.as_deref() {
        Some("disabled") => Some(true),
        Some("enabled") => Some(false),
        _ => None,
    };

    let mut filtered: Vec<RuleRow> = Vec::new();
    for rule in &overrides.custom {
        let fp = rule_fingerprint(rule);
        let disabled = overrides.is_disabled(&fp);
        if !matches_filter(
            rule_type_of(rule),
            disabled,
            &want_disabled,
            &keyword,
            &req.rule_type,
            rule,
        ) {
            continue;
        }
        filtered.push(RuleRow {
            index: None,
            fingerprint: fp,
            rule: rule.clone(),
            custom: true,
            disabled,
            rule_type: rule_type_of(rule).to_string(),
        });
    }
    for (idx, rule) in profile.route.rules.iter().enumerate() {
        let fp = rule_fingerprint(rule);
        let disabled = overrides.is_disabled(&fp);
        if !matches_filter(
            rule_type_of(rule),
            disabled,
            &want_disabled,
            &keyword,
            &req.rule_type,
            rule,
        ) {
            continue;
        }
        filtered.push(RuleRow {
            index: Some(idx),
            fingerprint: fp,
            rule: rule.clone(),
            custom: false,
            disabled,
            rule_type: rule_type_of(rule).to_string(),
        });
    }

    let total = filtered.len();
    let items: Vec<RuleRow> = filtered.into_iter().skip(req.offset).take(limit).collect();
    Ok(ListRulesResponse {
        total,
        offset: req.offset,
        limit,
        items,
    })
}

#[tauri::command]
pub fn list_rules(
    state: State<'_, AppState>,
    req: ListRulesRequest,
) -> Result<ListRulesResponse, AppError> {
    query_rules(state.inner(), &req)
}

fn matches_filter(
    rule_type: &str,
    disabled: bool,
    want_disabled: &Option<bool>,
    keyword: &Option<String>,
    type_filter: &Option<String>,
    rule: &serde_json::Value,
) -> bool {
    if let Some(want) = want_disabled {
        if disabled != *want {
            return false;
        }
    }
    if let Some(ty) = type_filter {
        if rule_type != ty {
            return false;
        }
    }
    if let Some(kw) = keyword {
        if !serde_json::to_string(rule)
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains(kw.as_str())
        {
            return false;
        }
    }
    true
}

#[derive(Deserialize)]
pub struct SetRuleDisabledRequest {
    pub fingerprint: String,
    pub disabled: bool,
}

/// Validate + persist the disable/enable toggle (no Apply).
fn persist_rule_disabled(state: &AppState, req: &SetRuleDisabledRequest) -> Result<(), AppError> {
    let profile = active_profile(state)?;
    let mut overrides = load_overrides(state);
    if !rule_exists(&profile, &overrides, &req.fingerprint) {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            "unknown rule fingerprint",
        ));
    }
    overrides.set_disabled(req.fingerprint.clone(), req.disabled);
    save_rule_overrides(&state.paths.rule_overrides(), &overrides)?;
    Ok(())
}

/// Disable / re-enable a rule (subscription or custom). Persisted by fingerprint so the
/// state survives subscription updates; Apply regenerates config (hot reload when running).
#[tauri::command]
pub fn set_rule_disabled(
    app: AppHandle,
    state: State<'_, AppState>,
    req: SetRuleDisabledRequest,
) -> Result<serde_json::Value, AppError> {
    let _orch = lock_orchestrate(&state)?;
    persist_rule_disabled(state.inner(), &req)?;

    let apply_warning = apply_after_rule_change(&app, &state)?;
    let mut value = serde_json::json!({ "ok": true, "disabled": req.disabled });
    attach_apply_warning(&mut value, apply_warning);
    Ok(value)
}

#[derive(Deserialize)]
pub struct AddCustomRuleRequest {
    pub rule: serde_json::Value,
}

/// Validate + persist a custom rule (no Apply). Returns its fingerprint.
fn persist_add_custom_rule(
    state: &AppState,
    req: &AddCustomRuleRequest,
) -> Result<String, AppError> {
    if !req.rule.is_object() {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            "custom rule must be a JSON object",
        ));
    }
    if req.rule.get("outbound").and_then(|v| v.as_str()).is_none() {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            "custom rule must reference an outbound (e.g. \"outbound\": \"direct\")",
        ));
    }
    // sing-box 1.13 removed the `geoip` / `geosite` rule options; custom rules are
    // emitted verbatim into the runtime config, so these matchers would make sing-box
    // exit FATAL on the next reload. Only subscription rules are geoip-expanded.
    for key in ["geoip", "geosite"] {
        if req.rule.get(key).is_some() {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                format!(
                    "custom rule cannot use the `{key}` matcher (sing-box 1.13 removed it); use `rule_set` instead"
                ),
            ));
        }
    }
    // Validate `rule_set` references against the active profile's rule-sets so a bad
    // reference is caught here instead of failing every config build afterwards.
    if let Ok(profile) = active_profile(state) {
        let set_tags: Vec<&str> = profile
            .route
            .rule_sets
            .iter()
            .filter_map(|s| s.get("tag").and_then(|v| v.as_str()))
            .collect();
        if let Some(refs) = req.rule.get("rule_set").and_then(|v| v.as_array()) {
            for r in refs {
                if let Some(t) = r.as_str() {
                    if !set_tags.contains(&t) {
                        return Err(AppError::new(
                            ErrorCode::ConfigInvalid,
                            format!("custom rule references unknown rule_set: {t}"),
                        ));
                    }
                }
            }
        }
    }
    let fp = rule_fingerprint(&req.rule);
    let mut overrides = load_overrides(state);
    if overrides.custom.iter().any(|r| rule_fingerprint(r) == fp) {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            "custom rule already exists",
        ));
    }
    overrides.custom.push(req.rule.clone());
    save_rule_overrides(&state.paths.rule_overrides(), &overrides)?;
    Ok(fp)
}

/// Add a user-defined rule, prepended ahead of subscription rules at build time.
#[tauri::command]
pub fn add_custom_rule(
    app: AppHandle,
    state: State<'_, AppState>,
    req: AddCustomRuleRequest,
) -> Result<serde_json::Value, AppError> {
    let _orch = lock_orchestrate(&state)?;
    let fp = persist_add_custom_rule(state.inner(), &req)?;

    let apply_warning = apply_after_rule_change(&app, &state)?;
    let mut value = serde_json::json!({ "ok": true, "fingerprint": fp });
    attach_apply_warning(&mut value, apply_warning);
    Ok(value)
}

#[derive(Deserialize)]
pub struct RemoveCustomRuleRequest {
    pub fingerprint: String,
}

/// Validate + persist custom rule removal (no Apply).
fn persist_remove_custom_rule(
    state: &AppState,
    req: &RemoveCustomRuleRequest,
) -> Result<(), AppError> {
    let mut overrides = load_overrides(state);
    let before = overrides.custom.len();
    overrides.remove_custom(&req.fingerprint);
    if overrides.custom.len() == before {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            "custom rule not found",
        ));
    }
    save_rule_overrides(&state.paths.rule_overrides(), &overrides)?;
    Ok(())
}

/// Remove a user-added rule (also clears its disabled mark).
#[tauri::command]
pub fn remove_custom_rule(
    app: AppHandle,
    state: State<'_, AppState>,
    req: RemoveCustomRuleRequest,
) -> Result<serde_json::Value, AppError> {
    let _orch = lock_orchestrate(&state)?;
    persist_remove_custom_rule(state.inner(), &req)?;

    let apply_warning = apply_after_rule_change(&app, &state)?;
    let mut value = serde_json::json!({ "ok": true });
    attach_apply_warning(&mut value, apply_warning);
    Ok(value)
}

#[derive(Deserialize)]
pub struct TagRequest {
    pub tag: String,
}

#[tauri::command]
pub fn set_selected_node(
    app: AppHandle,
    state: State<'_, AppState>,
    req: TagRequest,
) -> Result<(), AppError> {
    let _orch = lock_orchestrate(&state)?;
    require_known_node_tag(&state, &req.tag)?;

    // With strategy groups the pick applies to the group containing the tag (top-level
    // group preferred); flat profiles use the injected `proxy` selector.
    let profile = active_profile(state.inner())?;
    let selection_group = if profile.groups.is_empty() {
        None
    } else {
        selection_group_for(&profile, &req.tag)
    };

    // Picking a strategy group that isn't itself a member of any other group (e.g. the
    // top-level group) is a live no-op: grouped profiles have no flat `proxy` selector
    // for select_outbound to target, and there is no parent group to set its member in.
    if is_unselectable_group(&profile, &req.tag) {
        return Ok(());
    }

    let previous = current_settings(&state.paths)?;
    let mut settings = previous.clone();
    settings.selected_tag = Some(req.tag.clone());

    // Persist the group member selection too (mirrors set_group_selection) so grouped
    // profiles keep the pick across restarts / config regeneration.
    let previous_selection = if let Some(group) = &selection_group {
        let mut selections = load_group_selections(&state.paths.group_selections());
        let prev = selections.insert(group.clone(), req.tag.clone());
        save_group_selections(&state.paths.group_selections(), &selections)?;
        Some((group.clone(), prev))
    } else {
        None
    };

    persist_settings(&state.paths.settings(), &settings)?;
    if let Err(err) = generate_config(&state.paths, &settings, resource_dir(&app).as_deref()) {
        let _ = persist_settings(&state.paths.settings(), &previous);
        rollback_group_selection(&state, &previous_selection);
        return Err(err);
    }

    let should_select = {
        let core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
        core.state().status == CoreStatus::Running
    };
    if should_select {
        let endpoints = clash_endpoints(&settings);
        let result = match &selection_group {
            Some(group) => select_group(&endpoints, group, &req.tag),
            None => select_outbound(&endpoints, &req.tag),
        };
        if let Err(err) = result {
            let _ = persist_settings(&state.paths.settings(), &previous);
            rollback_group_selection(&state, &previous_selection);
            let _ = generate_config(&state.paths, &previous, resource_dir(&app).as_deref());
            return Err(AppError::from(err));
        }
    }

    Ok(())
}

/// Outermost group whose direct members include `tag`; prefers the profile's top-level
/// group (`default_outbound`). Returns `None` when the tag belongs to no group.
fn selection_group_for(profile: &NormalizedProfile, tag: &str) -> Option<String> {
    if profile.groups.is_empty() {
        return None;
    }
    let contains = |g: &NormalizedOutbound| {
        g.outbound
            .get("outbounds")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).any(|m| m == tag))
            .unwrap_or(false)
    };
    if let Some(top) = profile.default_outbound.as_deref() {
        if profile.groups.iter().any(|g| g.tag == top && contains(g)) {
            return Some(top.to_string());
        }
    }
    profile
        .groups
        .iter()
        .find(|g| contains(g))
        .map(|g| g.tag.clone())
}

/// Whether `tag` is a strategy group that no other group contains (e.g. the top-level
/// group). Such picks are a live no-op for the Clash API.
fn is_unselectable_group(profile: &NormalizedProfile, tag: &str) -> bool {
    !profile.groups.is_empty()
        && profile.groups.iter().any(|g| g.tag == tag)
        && selection_group_for(profile, tag).is_none()
}

/// Restore the group selection map to its state before `set_selected_node`.
fn rollback_group_selection(state: &AppState, previous: &Option<(String, Option<String>)>) {
    if let Some((group, prev)) = previous {
        let mut selections = load_group_selections(&state.paths.group_selections());
        match prev {
            Some(member) => {
                selections.insert(group.clone(), member.clone());
            }
            None => {
                selections.remove(group);
            }
        }
        let _ = save_group_selections(&state.paths.group_selections(), &selections);
    }
}

#[derive(Deserialize)]
pub struct GroupSelectionRequest {
    pub group: String,
    pub member: String,
}

/// Switch a strategy group member: persists the selection always (survives restarts /
/// config regeneration), and applies it live via Clash API when the core is running.
#[tauri::command]
pub fn set_group_selection(
    app: AppHandle,
    state: State<'_, AppState>,
    req: GroupSelectionRequest,
) -> Result<(), AppError> {
    let _orch = lock_orchestrate(&state)?;
    let outbounds = merged_outbounds(state.inner())?;
    validate_static_group_member(&outbounds, &req.group, &req.member)?;

    let mut selections = load_group_selections(&state.paths.group_selections());
    selections.insert(req.group.clone(), req.member.clone());
    save_group_selections(&state.paths.group_selections(), &selections)?;

    let settings = current_settings(&state.paths)?;
    let should_apply_live = {
        let core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
        core.state().status == CoreStatus::Running
    };
    if should_apply_live {
        let endpoints = clash_endpoints(&settings);
        select_group(&endpoints, &req.group, &req.member).map_err(AppError::from)?;
    } else {
        generate_config(&state.paths, &settings, resource_dir(&app).as_deref())?;
    }
    Ok(())
}

fn validate_static_group_member(
    outbounds: &[NormalizedOutbound],
    group: &str,
    member: &str,
) -> Result<(), AppError> {
    let g = outbounds.iter().find(|o| o.tag == group).ok_or_else(|| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("unknown strategy group: {group}"),
        )
    })?;
    if g.outbound.get("type").and_then(|v| v.as_str()) != Some("selector") {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            format!("{group} is not a selector group"),
        ));
    }
    let members: Vec<&str> = g
        .outbound
        .get("outbounds")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if !members.contains(&member) {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            format!("{member} is not a member of group {group}"),
        ));
    }
    Ok(())
}

#[derive(Serialize)]
pub struct DelayTestResponse {
    pub tag: String,
    pub delay_ms: u32,
}

#[tauri::command]
pub async fn test_node_delay(
    state: State<'_, AppState>,
    req: TagRequest,
) -> Result<DelayTestResponse, AppError> {
    require_known_node_tag(&state, &req.tag)?;
    let settings = current_settings(&state.paths)?;
    require_running_core(&state)?;
    let endpoints = clash_endpoints(&settings);
    let tag = req.tag.clone();
    let delay_ms = tauri::async_runtime::spawn_blocking(move || {
        proxy_delay(&endpoints, &tag, 5000, DELAY_TEST_URL)
    })
    .await
    .map_err(blocking_join_err("test_node_delay"))?
    .map_err(AppError::from)?;
    Ok(DelayTestResponse {
        tag: req.tag,
        delay_ms,
    })
}

#[tauri::command]
pub async fn get_connection_stats(
    state: State<'_, AppState>,
) -> Result<ConnectionStats, AppError> {
    let settings = current_settings(&state.paths)?;
    require_running_core(&state)?;
    let endpoints = clash_endpoints(&settings);
    tauri::async_runtime::spawn_blocking(move || connection_stats(&endpoints))
        .await
        .map_err(blocking_join_err("get_connection_stats"))?
        .map_err(AppError::from)
}

#[tauri::command]
pub async fn get_traffic_sample(state: State<'_, AppState>) -> Result<TrafficSample, AppError> {
    let settings = current_settings(&state.paths)?;
    require_running_core(&state)?;
    let endpoints = clash_endpoints(&settings);
    tauri::async_runtime::spawn_blocking(move || traffic_sample(&endpoints))
        .await
        .map_err(blocking_join_err("get_traffic_sample"))?
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ice_config::{AppPaths, NormalizedOutbound};
    use ice_subscription::{
        load_index, read_profile, write_subscription_success, SubscriptionFormat, SubscriptionMeta,
        SubscriptionPaths,
    };
    use std::fs;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    fn temp_state_with_node(label: &str) -> AppState {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-cmd-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = AppPaths::new(&dir);
        paths.ensure_dirs().unwrap();
        let sub = SubscriptionPaths::from_app(&paths);
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
        };
        let nodes = vec![NormalizedOutbound {
            tag: "n1".into(),
            outbound: serde_json::json!({"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}),
        }];
        write_subscription_success(
            &sub,
            &meta,
            "{}",
            &ice_config::NormalizedProfile::from_nodes_only(nodes),
        )
        .unwrap();

        AppState {
            paths: paths.clone(),
            core: Mutex::new(
                Box::new(ice_core::CoreController::default()) as Box<dyn ice_core::CoreHandle>
            ),
            proxy: Mutex::new(Box::new(ice_proxy_sys::NoopSystemProxy)),
            orchestrate: Mutex::new(()),
            proxy_recovery_warning: Mutex::new(None),
            proxy_applied_cache: Mutex::new(None),
            _instance_lock: crate::test_instance_lock(&paths),
        }
    }

    fn temp_state_with_rules(label: &str, rules: Vec<serde_json::Value>) -> AppState {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-cmd-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = AppPaths::new(&dir);
        paths.ensure_dirs().unwrap();
        let sub = SubscriptionPaths::from_app(&paths);
        let id = Uuid::new_v4();
        let meta = SubscriptionMeta {
            id,
            name: "t".into(),
            url: "https://example.com/s".into(),
            active: true,
            format: SubscriptionFormat::SingBox,
            node_count: 1,
            group_count: 0,
            rule_count: rules.len(),
            has_dns: false,
            parse_warnings: vec![],
            last_updated: None,
            last_error: None,
            etag: None,
            last_modified: None,
        };
        let mut profile = ice_config::NormalizedProfile::from_nodes_only(vec![
            NormalizedOutbound {
                tag: "n1".into(),
                outbound: serde_json::json!({"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}),
            },
        ]);
        profile.route.rules = rules;
        write_subscription_success(&sub, &meta, "{}", &profile).unwrap();

        AppState {
            paths: paths.clone(),
            core: Mutex::new(
                Box::new(ice_core::CoreController::default()) as Box<dyn ice_core::CoreHandle>
            ),
            proxy: Mutex::new(Box::new(ice_proxy_sys::NoopSystemProxy)),
            orchestrate: Mutex::new(()),
            proxy_recovery_warning: Mutex::new(None),
            proxy_applied_cache: Mutex::new(None),
            _instance_lock: crate::test_instance_lock(&paths),
        }
    }

    #[test]
    fn parse_proxy_mode_accepts_valid_and_rejects_unknown() {
        assert_eq!(parse_proxy_mode("rule").unwrap(), ProxyMode::Rule);
        assert_eq!(parse_proxy_mode("global").unwrap(), ProxyMode::Global);
        assert_eq!(parse_proxy_mode("direct").unwrap(), ProxyMode::Direct);
        let err = parse_proxy_mode("nope").expect_err("unknown mode");
        assert_eq!(err.code, "config.invalid");
        assert!(err.message.contains("unknown proxy mode"));
    }

    #[test]
    fn require_known_node_tag_rejects_unknown() {
        let state = temp_state_with_node("tag");
        let err = require_known_node_tag(&state, "missing").expect_err("unknown tag");
        assert_eq!(err.code, "config.invalid");
        assert!(err.message.contains("unknown node tag"));
        let _ = fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn require_known_node_tag_accepts_merged_node() {
        let state = temp_state_with_node("ok");
        require_known_node_tag(&state, "n1").expect("known tag");
        let _ = fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn validate_static_group_member_accepts_member() {
        let outbounds = vec![NormalizedOutbound {
            tag: "Proxies".into(),
            outbound: serde_json::json!({
                "type": "selector",
                "tag": "Proxies",
                "outbounds": ["n1", "n2"],
            }),
        }];
        validate_static_group_member(&outbounds, "Proxies", "n2").expect("member");
    }

    #[test]
    fn validate_static_group_member_rejects_unknown_group() {
        let outbounds = vec![NormalizedOutbound {
            tag: "Proxies".into(),
            outbound: serde_json::json!({"type": "selector", "outbounds": ["n1"]}),
        }];
        let err = validate_static_group_member(&outbounds, "missing", "n1").expect_err("unknown");
        assert_eq!(err.code, "config.invalid");
        assert!(err.message.contains("unknown strategy group"));
    }

    #[test]
    fn validate_static_group_member_rejects_non_member() {
        let outbounds = vec![NormalizedOutbound {
            tag: "Proxies".into(),
            outbound: serde_json::json!({"type": "selector", "outbounds": ["n1"]}),
        }];
        let err =
            validate_static_group_member(&outbounds, "Proxies", "nope").expect_err("non member");
        assert_eq!(err.code, "config.invalid");
        assert!(err.message.contains("is not a member"));
    }

    #[test]
    fn validate_static_group_member_rejects_non_selector() {
        let outbounds = vec![NormalizedOutbound {
            tag: "auto".into(),
            outbound: serde_json::json!({"type": "urltest", "outbounds": ["n1"]}),
        }];
        let err = validate_static_group_member(&outbounds, "auto", "n1").expect_err("not selector");
        assert_eq!(err.code, "config.invalid");
        assert!(err.message.contains("is not a selector group"));
    }

    #[test]
    fn selection_group_for_flat_profile_is_none() {
        let profile = NormalizedProfile::from_nodes_only(vec![NormalizedOutbound {
            tag: "n1".into(),
            outbound: serde_json::json!({"type": "socks", "tag": "n1"}),
        }]);
        assert_eq!(selection_group_for(&profile, "n1"), None);
    }

    #[test]
    fn selection_group_for_prefers_top_level_group() {
        let profile = NormalizedProfile {
            nodes: vec![NormalizedOutbound {
                tag: "HK".into(),
                outbound: serde_json::json!({"type": "socks", "tag": "HK"}),
            }],
            groups: vec![
                NormalizedOutbound {
                    tag: "Proxies".into(),
                    outbound: serde_json::json!({
                        "type": "selector",
                        "tag": "Proxies",
                        "outbounds": ["auto", "HK", "direct"],
                    }),
                },
                NormalizedOutbound {
                    tag: "auto".into(),
                    outbound: serde_json::json!({
                        "type": "urltest",
                        "tag": "auto",
                        "outbounds": ["HK", "JP"],
                    }),
                },
            ],
            route: Default::default(),
            dns: None,
            default_outbound: Some("Proxies".into()),
            parse_stats: Default::default(),
        };
        assert_eq!(
            selection_group_for(&profile, "HK").as_deref(),
            Some("Proxies"),
            "leaf in top group selects there"
        );
        assert_eq!(
            selection_group_for(&profile, "auto").as_deref(),
            Some("Proxies"),
            "sub-group in top group selects there"
        );
        assert_eq!(
            selection_group_for(&profile, "JP").as_deref(),
            Some("auto"),
            "leaf only in sub-group selects there"
        );
        assert_eq!(
            selection_group_for(&profile, "Proxies").as_deref(),
            None,
            "selecting the top group itself is a no-op"
        );
        assert!(is_unselectable_group(&profile, "Proxies"));
        assert!(!is_unselectable_group(&profile, "auto"));
        assert!(!is_unselectable_group(&profile, "HK"));
    }

    fn sample_rules() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({ "domain_suffix": ["youtube.com"], "outbound": "n1" }),
            serde_json::json!({ "domain_suffix": ["google.com"], "outbound": "n1" }),
            serde_json::json!({ "geoip": ["cn"], "outbound": "direct" }),
            serde_json::json!({ "ip_is_private": true, "outbound": "direct" }),
        ]
    }

    #[test]
    fn list_rules_returns_all_with_indexes_and_filters() {
        let state = temp_state_with_rules("rules-all", sample_rules());
        let resp = query_rules(
            &state,
            &ListRulesRequest {
                keyword: None,
                rule_type: None,
                disabled: None,
                offset: 0,
                limit: 2,
            },
        )
        .unwrap();
        assert_eq!(resp.total, 4);
        assert_eq!(resp.items.len(), 2);
        assert_eq!(resp.items[0].index, Some(0));
        assert_eq!(resp.items[1].index, Some(1));

        let filtered = query_rules(
            &state,
            &ListRulesRequest {
                keyword: Some("geo".into()),
                rule_type: None,
                disabled: None,
                offset: 0,
                limit: 50,
            },
        )
        .unwrap();
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.items[0].rule_type, "geoip");

        let typed = query_rules(
            &state,
            &ListRulesRequest {
                keyword: None,
                rule_type: Some("domain_suffix".into()),
                disabled: None,
                offset: 1,
                limit: 50,
            },
        )
        .unwrap();
        assert_eq!(typed.total, 2);
        assert_eq!(typed.items[0].index, Some(1));
        let _ = fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn rule_overview_counts_types_and_disabled() {
        let state = temp_state_with_rules("rules-overview", sample_rules());
        let fp = rule_fingerprint(&sample_rules()[0]);
        let mut overrides = load_rule_overrides(&state.paths.rule_overrides());
        overrides.set_disabled(fp, true);
        save_rule_overrides(&state.paths.rule_overrides(), &overrides).unwrap();

        let overview = rule_overview(&state).unwrap();
        assert_eq!(overview.total, 4);
        assert_eq!(overview.disabled, 1);
        assert_eq!(overview.rule_sets, 0);
        assert_eq!(overview.custom, 0);
        let suffix = overview
            .types
            .iter()
            .find(|t| t.rule_type == "domain_suffix")
            .unwrap();
        assert_eq!(suffix.count, 2);
        let _ = fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn set_rule_disabled_persists_and_generates_config_without_rule() {
        let state = temp_state_with_rules("rules-disable", sample_rules());
        let fp = rule_fingerprint(&sample_rules()[0]);
        persist_rule_disabled(
            &state,
            &SetRuleDisabledRequest {
                fingerprint: fp.clone(),
                disabled: true,
            },
        )
        .unwrap();

        generate_config(&state.paths, &AppSettings::default(), None).unwrap();
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(state.paths.config()).unwrap()).unwrap();
        let rules = config["route"]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 3, "disabled rule dropped from runtime config");
        assert!(!serde_json::to_string(rules)
            .unwrap()
            .contains("youtube.com"));

        let listed = query_rules(
            &state,
            &ListRulesRequest {
                keyword: None,
                rule_type: None,
                disabled: Some("disabled".into()),
                offset: 0,
                limit: 50,
            },
        )
        .unwrap();
        assert_eq!(listed.total, 1);
        assert_eq!(listed.items[0].fingerprint, fp);
        let _ = fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn set_rule_disabled_rejects_unknown_fingerprint() {
        let state = temp_state_with_rules("rules-unknown", sample_rules());
        let err = persist_rule_disabled(
            &state,
            &SetRuleDisabledRequest {
                fingerprint: "nope".into(),
                disabled: true,
            },
        )
        .expect_err("unknown fingerprint");
        assert_eq!(err.code, "config.invalid");
        let _ = fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn add_remove_custom_rule_round_trip_and_validation() {
        let state = temp_state_with_rules("rules-custom", sample_rules());
        let custom = serde_json::json!({ "domain": ["example.com"], "outbound": "block" });
        let fp = persist_add_custom_rule(
            &state,
            &AddCustomRuleRequest {
                rule: custom.clone(),
            },
        )
        .unwrap();

        let listed = query_rules(
            &state,
            &ListRulesRequest {
                keyword: Some("example".into()),
                rule_type: None,
                disabled: None,
                offset: 0,
                limit: 50,
            },
        )
        .unwrap();
        assert_eq!(listed.total, 1);
        assert!(listed.items[0].custom);
        assert_eq!(listed.items[0].index, None);

        let overview = rule_overview(&state).unwrap();
        assert_eq!(overview.custom, 1);

        persist_remove_custom_rule(&state, &RemoveCustomRuleRequest { fingerprint: fp }).unwrap();

        let overview = rule_overview(&state).unwrap();
        assert_eq!(overview.custom, 0);

        let err = persist_add_custom_rule(
            &state,
            &AddCustomRuleRequest {
                rule: serde_json::json!("not-an-object"),
            },
        )
        .expect_err("non object");
        assert_eq!(err.code, "config.invalid");

        let err = persist_add_custom_rule(
            &state,
            &AddCustomRuleRequest {
                rule: serde_json::json!({ "domain": ["x.com"] }),
            },
        )
        .expect_err("missing outbound");
        assert_eq!(err.code, "config.invalid");
        let _ = fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn add_custom_rule_rejects_geoip_and_geosite_matchers() {
        let state = temp_state_with_rules("rules-custom-geo", sample_rules());
        for key in ["geoip", "geosite"] {
            let err = persist_add_custom_rule(
                &state,
                &AddCustomRuleRequest {
                    rule: serde_json::json!({ key: ["cn"], "outbound": "direct" }),
                },
            )
            .expect_err(&format!("{key} must be rejected"));
            assert_eq!(err.code, "config.invalid");
            assert!(
                err.message.contains(key),
                "message should name the matcher: {err}"
            );
        }
        let overrides = load_rule_overrides(&state.paths.rule_overrides());
        assert_eq!(overrides.custom.len(), 0, "nothing persisted");
        let _ = fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn add_custom_rule_validates_rule_set_references() {
        let state = temp_state_with_rules("rules-custom-set", vec![]);
        let sub = SubscriptionPaths::from_app(&state.paths);
        let index = load_index(&sub).unwrap();
        let id = index.items[0].id;
        let mut profile = read_profile(&sub, id).unwrap();
        profile.route.rule_sets = vec![serde_json::json!({
            "type": "remote",
            "tag": "cn",
            "url": "https://example.com/cn.srs",
        })];
        fs::write(
            sub.profile(id),
            serde_json::to_vec(&profile).unwrap(),
        )
        .unwrap();

        let fp = persist_add_custom_rule(
            &state,
            &AddCustomRuleRequest {
                rule: serde_json::json!({ "rule_set": ["cn"], "outbound": "direct" }),
            },
        )
        .expect("known rule_set accepted");
        assert!(!fp.is_empty());

        let err = persist_add_custom_rule(
            &state,
            &AddCustomRuleRequest {
                rule: serde_json::json!({ "rule_set": ["missing"], "outbound": "direct" }),
            },
        )
        .expect_err("unknown rule_set");
        assert_eq!(err.code, "config.invalid");
        assert!(err.message.contains("unknown rule_set"));
        let _ = fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn custom_rule_disabled_dropped_from_runtime_config() {
        let state = temp_state_with_rules("rules-custom-off", sample_rules());
        let custom = serde_json::json!({ "domain": ["blockme.com"], "outbound": "block" });
        let fp = rule_fingerprint(&custom);
        let mut overrides = load_rule_overrides(&state.paths.rule_overrides());
        overrides.custom.push(custom);
        overrides.set_disabled(fp, true);
        save_rule_overrides(&state.paths.rule_overrides(), &overrides).unwrap();

        generate_config(&state.paths, &AppSettings::default(), None).unwrap();
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(state.paths.config()).unwrap()).unwrap();
        let rules = config["route"]["rules"].as_array().unwrap();
        assert!(!serde_json::to_string(rules)
            .unwrap()
            .contains("blockme.com"));
        let _ = fs::remove_dir_all(state.paths.root());
    }
}
