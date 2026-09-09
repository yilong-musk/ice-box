// SPDX-License-Identifier: GPL-3.0-or-later

use super::settings::{apply_after_subscription_change, attach_apply_warning};
use super::*;

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
pub async fn list_nodes(app: AppHandle) -> Result<Vec<NodeInfo>, AppError> {
    run_blocking("list_nodes", move || {
        let state = app.state::<AppState>();
        let Some(outbounds) = merged_outbounds_opt(&state)? else {
            return Ok(vec![]);
        };
        let settings = current_settings(&state.paths)?;
        let selections = load_group_selections(&state.paths.group_selections());
        let core_running = state.core_snapshot.load().state.status == CoreStatus::Running;
        let live = if core_running {
            let endpoints = clash_endpoints(&settings);
            proxy_groups(&endpoints).ok()
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
    })
    .await
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
    /// Restrict to custom rules (`Some(true)`) or subscription rules (`Some(false)`).
    #[serde(default)]
    pub custom: Option<bool>,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_rules_page_size")]
    pub limit: usize,
}

pub(crate) fn default_rules_page_size() -> usize {
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

pub(crate) fn load_overrides(state: &AppState) -> RuleOverrides {
    let mut overrides = load_rule_overrides(&state.paths.rule_overrides());
    if let Ok(Some(entry)) = cached_profile(state) {
        let mut rules: Vec<_> = entry.profile.route.rules.clone();
        rules.extend(overrides.custom.clone());
        if overrides.migrate_legacy_fingerprints(rules.iter()) {
            let _ = save_rule_overrides(&state.paths.rule_overrides(), &overrides);
        }
    }
    overrides
}

pub(crate) fn rule_exists(
    profile: &NormalizedProfile,
    overrides: &RuleOverrides,
    fingerprint: &str,
) -> bool {
    profile
        .route
        .rules
        .iter()
        .any(|r| rule_matches_fingerprint(r, fingerprint))
        || overrides
            .custom
            .iter()
            .any(|r| rule_matches_fingerprint(r, fingerprint))
}

/// Persist rule overrides then Apply (hot reload when running), like subscription mutations.
pub(crate) fn apply_after_rule_change(
    app: &AppHandle,
    state: &AppState,
) -> Result<Option<AppError>, AppError> {
    let settings = current_settings(&state.paths)?;
    Ok(apply_after_subscription_change(app, state, &settings))
}

/// Rules for the active subscription only (single-active model).
pub(crate) fn rule_overview(state: &AppState) -> Result<RuleOverview, AppError> {
    let cached = cached_profile(state)?;
    let empty = NormalizedProfile::from_nodes_only(vec![]);
    let profile = cached
        .as_ref()
        .map(|entry| entry.profile.as_ref())
        .unwrap_or(&empty);
    let fingerprints: &[String] = cached
        .as_ref()
        .map(|entry| entry.fingerprints.as_ref())
        .map_or(&[], |v| v);
    let overrides = load_overrides(state);
    let mut counts: std::collections::HashMap<&'static str, usize> =
        std::collections::HashMap::new();
    let mut disabled = 0usize;
    for (idx, rule) in profile.route.rules.iter().enumerate() {
        let fp: std::borrow::Cow<'_, str> = fingerprints
            .get(idx)
            .map(|fp| std::borrow::Cow::Borrowed(fp.as_str()))
            .unwrap_or_else(|| std::borrow::Cow::Owned(rule_fingerprint(rule)));
        if overrides.is_disabled(fp.as_ref()) {
            disabled += 1;
        }
        *counts.entry(rule_type_of(rule)).or_default() += 1;
    }
    for rule in &overrides.custom {
        if overrides.is_rule_disabled(rule) {
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
pub async fn get_rule_overview(app: AppHandle) -> Result<RuleOverview, AppError> {
    run_blocking("get_rule_overview", move || {
        let state = app.state::<AppState>();
        rule_overview(state.inner())
    })
    .await
}

/// Query rules with server-side filtering + pagination. Never ships the full rule list
/// over IPC: big subscriptions (up to 10k rules) stay cheap for the UI.
pub(crate) fn query_rules(
    state: &AppState,
    req: &ListRulesRequest,
) -> Result<ListRulesResponse, AppError> {
    let cached = cached_profile(state)?;
    let empty = NormalizedProfile::from_nodes_only(vec![]);
    let profile = cached
        .as_ref()
        .map(|entry| entry.profile.as_ref())
        .unwrap_or(&empty);
    let fingerprints: &[String] = cached
        .as_ref()
        .map(|entry| entry.fingerprints.as_ref())
        .map_or(&[], |v| v);
    // Lowercase-serialized rule text is built once per profile version (lazily,
    // only when a keyword is present) instead of per request.
    let keyword_texts = req
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .and_then(|_| cached.as_ref().map(|entry| entry.keyword_texts()));
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
        if req.custom == Some(false) {
            continue;
        }
        let fp = rule_fingerprint(rule);
        let disabled = overrides.is_rule_disabled(rule);
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
        if req.custom == Some(true) {
            continue;
        }
        let fp: std::borrow::Cow<'_, str> = fingerprints
            .get(idx)
            .map(|fp| std::borrow::Cow::Borrowed(fp.as_str()))
            .unwrap_or_else(|| std::borrow::Cow::Owned(rule_fingerprint(rule)));
        let disabled = overrides.is_disabled(fp.as_ref());
        if keyword_matches(&keyword, &keyword_texts, idx, rule)
            && matches_filter(
                rule_type_of(rule),
                disabled,
                &want_disabled,
                &None,
                &req.rule_type,
                rule,
            )
        {
            filtered.push(RuleRow {
                index: Some(idx),
                fingerprint: fp.into_owned(),
                rule: rule.clone(),
                custom: false,
                disabled,
                rule_type: rule_type_of(rule).to_string(),
            });
        }
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
pub async fn list_rules(
    app: AppHandle,
    req: ListRulesRequest,
) -> Result<ListRulesResponse, AppError> {
    run_blocking("list_rules", move || {
        let state = app.state::<AppState>();
        query_rules(state.inner(), &req)
    })
    .await
}

/// Keyword containment check. Subscription rules use the precomputed
/// lowercase text (built once per profile version); the fallback path (custom
/// rules are few) serializes on demand.
pub(crate) fn keyword_matches(
    keyword: &Option<String>,
    texts: &Option<Arc<Vec<String>>>,
    idx: usize,
    rule: &serde_json::Value,
) -> bool {
    match (keyword, texts) {
        (Some(kw), Some(texts)) => texts
            .get(idx)
            .map(|text| text.contains(kw.as_str()))
            .unwrap_or(false),
        (Some(kw), None) => serde_json::to_string(rule)
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains(kw.as_str()),
        (None, _) => true,
    }
}

pub(crate) fn matches_filter(
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
pub(crate) fn persist_rule_disabled(
    state: &AppState,
    req: &SetRuleDisabledRequest,
) -> Result<(), AppError> {
    let mut overrides = load_overrides(state);
    let profile = active_profile(state)?;
    if !rule_exists(&profile, &overrides, &req.fingerprint) {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            "unknown rule fingerprint",
        ));
    }
    if let Some(rule) = profile
        .route
        .rules
        .iter()
        .chain(overrides.custom.iter())
        .find(|r| rule_matches_fingerprint(r, &req.fingerprint))
        .cloned()
    {
        overrides.set_rule_disabled(&rule, req.disabled);
    } else {
        overrides.set_disabled(req.fingerprint.clone(), req.disabled);
    }
    save_rule_overrides(&state.paths.rule_overrides(), &overrides)?;
    Ok(())
}

/// Disable / re-enable a rule (subscription or custom). Persisted by fingerprint so the
/// state survives subscription updates; Apply regenerates config (hot reload when running).
#[tauri::command]
pub async fn set_rule_disabled(
    app: AppHandle,
    req: SetRuleDisabledRequest,
) -> Result<serde_json::Value, AppError> {
    run_blocking("set_rule_disabled", move || {
        let state = app.state::<AppState>();
        let _orch = lock_orchestrate(&state)?;
        persist_rule_disabled(state.inner(), &req)?;

        let apply_warning = apply_after_rule_change(&app, &state)?;
        let mut value = serde_json::json!({ "ok": true, "disabled": req.disabled });
        attach_apply_warning(&mut value, apply_warning);
        Ok(value)
    })
    .await
}

#[derive(Deserialize)]
pub struct AddCustomRuleRequest {
    pub rule: serde_json::Value,
}

/// Validate + persist a custom rule (no Apply). Returns its fingerprint.
pub(crate) fn persist_add_custom_rule(
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
pub async fn add_custom_rule(
    app: AppHandle,
    req: AddCustomRuleRequest,
) -> Result<serde_json::Value, AppError> {
    run_blocking("add_custom_rule", move || {
        let state = app.state::<AppState>();
        let _orch = lock_orchestrate(&state)?;
        let fp = persist_add_custom_rule(state.inner(), &req)?;

        let apply_warning = apply_after_rule_change(&app, &state)?;
        let mut value = serde_json::json!({ "ok": true, "fingerprint": fp });
        attach_apply_warning(&mut value, apply_warning);
        Ok(value)
    })
    .await
}

#[derive(Deserialize)]
pub struct RemoveCustomRuleRequest {
    pub fingerprint: String,
}

/// Validate + persist custom rule removal (no Apply).
pub(crate) fn persist_remove_custom_rule(
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
pub async fn remove_custom_rule(
    app: AppHandle,
    req: RemoveCustomRuleRequest,
) -> Result<serde_json::Value, AppError> {
    run_blocking("remove_custom_rule", move || {
        let state = app.state::<AppState>();
        let _orch = lock_orchestrate(&state)?;
        persist_remove_custom_rule(state.inner(), &req)?;

        let apply_warning = apply_after_rule_change(&app, &state)?;
        let mut value = serde_json::json!({ "ok": true });
        attach_apply_warning(&mut value, apply_warning);
        Ok(value)
    })
    .await
}

#[derive(Deserialize)]
pub struct TagRequest {
    pub tag: String,
}

#[tauri::command]
pub async fn set_selected_node(app: AppHandle, req: TagRequest) -> Result<(), AppError> {
    run_blocking("set_selected_node", move || {
        let state = app.state::<AppState>();
        let _orch = lock_orchestrate(&state)?;
        // One profile load (mtime-cached) validates the tag and computes the
        // selection group; the pick itself is applied live via the Clash API.
        let profile = active_profile(&state)?;
        if !profile.all_tags().iter().any(|t| t == &req.tag) {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                format!("unknown node tag: {}", req.tag),
            ));
        }

        // With strategy groups the pick applies to the group containing the tag (top-level
        // group preferred); flat profiles use the injected `proxy` selector.
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

        persist_settings(&state.paths.settings(), &settings, host_platform())?;
        // Persist the default in the runtime config. The live switch (below)
        // already applied the pick; the config write only bakes it in for
        // restarts, so patch the target selector's `default` instead of a full
        // rebuild. Fall back to `generate_config` when the selector is not
        // locatable (e.g. first run without a config yet).
        let selector_tag = if profile.groups.is_empty() {
            Some("proxy".to_string())
        } else {
            selection_group.clone()
        };
        let persist_result = match &selector_tag {
            Some(sel) => match patch_selected_tag_default(&state.paths, sel, &req.tag) {
                Ok(true) => Ok(()),
                Ok(false) => generate_config_with_cache(
                    &state.paths,
                    &settings,
                    resource_dir(&app).as_deref(),
                    state.capture.apply_intent(),
                    Some(state.profile_parse_cache.as_ref()),
                )
                .map(|_| ()),
                Err(err) => Err(err),
            },
            None => generate_config_with_cache(
                &state.paths,
                &settings,
                resource_dir(&app).as_deref(),
                state.capture.apply_intent(),
                Some(state.profile_parse_cache.as_ref()),
            )
            .map(|_| ()),
        };
        if let Err(err) = persist_result {
            let _ = persist_settings(&state.paths.settings(), &previous, host_platform());
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
                let _ = persist_settings(&state.paths.settings(), &previous, host_platform());
                rollback_group_selection(&state, &previous_selection);
                let _ = generate_config_with_cache(
                    &state.paths,
                    &previous,
                    resource_dir(&app).as_deref(),
                    state.capture.apply_intent(),
                    Some(state.profile_parse_cache.as_ref()),
                );
                return Err(AppError::from(err));
            }
        }

        Ok(())
    })
    .await
}

/// Outermost group whose direct members include `tag`; prefers the profile's top-level
/// group (`default_outbound`). Returns `None` when the tag belongs to no group.
pub(crate) fn selection_group_for(profile: &NormalizedProfile, tag: &str) -> Option<String> {
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
pub(crate) fn is_unselectable_group(profile: &NormalizedProfile, tag: &str) -> bool {
    !profile.groups.is_empty()
        && profile.groups.iter().any(|g| g.tag == tag)
        && selection_group_for(profile, tag).is_none()
}

/// Restore the group selection map to its state before `set_selected_node`.
pub(crate) fn rollback_group_selection(
    state: &AppState,
    previous: &Option<(String, Option<String>)>,
) {
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
pub async fn set_group_selection(
    app: AppHandle,
    req: GroupSelectionRequest,
) -> Result<(), AppError> {
    run_blocking("set_group_selection", move || {
        let state = app.state::<AppState>();
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
        } else if !patch_selected_tag_default(&state.paths, &req.group, &req.member)? {
            generate_config_with_cache(
                &state.paths,
                &settings,
                resource_dir(&app).as_deref(),
                state.capture.apply_intent(),
                Some(state.profile_parse_cache.as_ref()),
            )?;
        }
        Ok(())
    })
    .await
}

pub(crate) fn validate_static_group_member(
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
    app: AppHandle,
    req: TagRequest,
) -> Result<DelayTestResponse, AppError> {
    run_blocking("test_node_delay", move || {
        let state = app.state::<AppState>();
        require_known_node_tag(&state, &req.tag)?;
        let settings = current_settings(&state.paths)?;
        require_running_core(&state)?;
        let endpoints = clash_endpoints(&settings);
        let delay_ms =
            proxy_delay(&endpoints, &req.tag, 5000, DELAY_TEST_URL).map_err(AppError::from)?;
        Ok(DelayTestResponse {
            tag: req.tag,
            delay_ms,
        })
    })
    .await
}
