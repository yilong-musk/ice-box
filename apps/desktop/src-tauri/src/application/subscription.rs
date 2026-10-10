// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[derive(Deserialize)]
pub struct AddSubscriptionRequest {
    pub url: String,
    pub name: Option<String>,
    #[serde(default)]
    pub auto_update: bool,
    #[serde(default)]
    pub auto_update_interval: Option<ice_engine::AutoUpdateInterval>,
}

#[derive(Deserialize)]
pub struct ImportSubscriptionFileRequest {
    pub content: String,
    pub name: Option<String>,
}

#[derive(Deserialize)]
pub struct IdRequest {
    pub id: Uuid,
}

#[derive(Deserialize)]
pub struct SetActiveRequest {
    pub id: Uuid,
    pub active: bool,
}

#[derive(Deserialize)]
pub struct SetAutoUpdateRequest {
    pub id: Uuid,
    pub auto_update: bool,
    #[serde(default)]
    pub auto_update_interval: Option<ice_engine::AutoUpdateInterval>,
}

pub(crate) fn list_subscriptions_use_case(state: &AppState) -> Result<serde_json::Value, AppError> {
    let paths = SubscriptionPaths::from_app(&state.paths);
    let index = ice_engine::load_index(&paths).map_err(AppError::from)?;
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

pub(crate) fn add_subscription_use_case(
    app: &impl AppHost,
    state: &AppState,
    req: AddSubscriptionRequest,
) -> Result<serde_json::Value, AppError> {
    let redacted = redact_subscription_url_for_log(&req.url);
    tracing::info!(url = %redacted, name = ?req.name, auto_update = req.auto_update, interval = ?req.auto_update_interval, "add_subscription: start");
    // Fetch (up to FETCH_TIMEOUT) runs without the orchestrate lock so Start/Stop/Apply/
    // save_settings are not queued behind it; the lock is taken for the disk write + Apply.
    let paths = SubscriptionPaths::from_app(&state.paths);
    let mgr = SubscriptionManager::open(paths, host_platform());
    let fetched = mgr
        .fetch_add(
            &req.url,
            req.name.as_deref(),
            req.auto_update,
            req.auto_update_interval,
        )
        .map_err(|e| {
            tracing::warn!(url = %redacted, error = %e.redacted_display(), code = %e.code().as_str(), "add_subscription: fetch/parse failed");
            AppError::from(e)
        })?;

    let _orch = lock_orchestrate(state)?;
    let meta = mgr.apply_add(fetched).map_err(|e| {
        tracing::warn!(url = %redacted, error = %e.redacted_display(), code = %e.code().as_str(), "add_subscription: apply failed");
        AppError::from(e)
    })?;
    tracing::info!(url = %redacted, id = %meta.id, name = %meta.name, nodes = meta.node_count, format = ?meta.format, "add_subscription: imported");

    let settings = current_settings(&state.paths)?;
    let apply_warning = apply_after_subscription_change(app, state, &settings);
    if let Some(w) = &apply_warning {
        tracing::warn!(code = %w.code, error = %w.message, "add_subscription: apply warning");
    }
    let mut value = serde_json::to_value(meta)
        .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("serialize: {e}")))?;
    attach_apply_warning(&mut value, apply_warning);
    drop(_orch);
    broadcast_state_change(app);
    Ok(value)
}

pub(crate) fn import_subscription_file_use_case(
    app: &impl AppHost,
    state: &AppState,
    req: ImportSubscriptionFileRequest,
) -> Result<serde_json::Value, AppError> {
    let bytes = req.content.len();
    let name = req.name;
    let content = req.content;
    tracing::info!(bytes, name = ?name, "import_subscription_file: start");
    // Parse before the orchestrate lock. The body is not logged.
    let paths = SubscriptionPaths::from_app(&state.paths);
    let mgr = SubscriptionManager::open(paths, host_platform());
    let prepared = mgr
        .prepare_file_import(&content, name.as_deref())
        .map_err(|e| {
            tracing::warn!(bytes, error = %e.redacted_display(), code = %e.code().as_str(), "import_subscription_file: parse failed");
            AppError::from(e)
        })?;
    drop(content);

    let _orch = lock_orchestrate(state)?;
    let meta = mgr.apply_add(prepared).map_err(|e| {
        tracing::warn!(bytes, error = %e.redacted_display(), code = %e.code().as_str(), "import_subscription_file: apply failed");
        AppError::from(e)
    })?;
    tracing::info!(id = %meta.id, name = %meta.name, nodes = meta.node_count, format = ?meta.format, "import_subscription_file: imported");

    let settings = current_settings(&state.paths)?;
    let apply_warning = apply_after_subscription_change(app, state, &settings);
    if let Some(w) = &apply_warning {
        tracing::warn!(code = %w.code, error = %w.message, "import_subscription_file: apply warning");
    }
    let mut value = serde_json::to_value(meta)
        .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("serialize: {e}")))?;
    attach_apply_warning(&mut value, apply_warning);
    drop(_orch);
    broadcast_state_change(app);
    Ok(value)
}

pub(crate) fn remove_subscription_use_case(
    app: &impl AppHost,
    state: &AppState,
    req: IdRequest,
) -> Result<serde_json::Value, AppError> {
    let _orch = lock_orchestrate(state)?;
    let paths = SubscriptionPaths::from_app(&state.paths);
    ice_engine::remove_subscription(&paths, req.id).map_err(AppError::from)?;

    let settings = current_settings(&state.paths)?;
    let apply_warning = apply_after_subscription_change(app, state, &settings);
    let mut value = serde_json::json!({ "ok": true });
    attach_apply_warning(&mut value, apply_warning);
    drop(_orch);
    broadcast_state_change(app);
    Ok(value)
}

pub(crate) fn update_subscription_use_case(
    app: &impl AppHost,
    state: &AppState,
    req: IdRequest,
) -> Result<serde_json::Value, AppError> {
    // Fetch (up to FETCH_TIMEOUT) runs without the orchestrate lock; the lock is taken
    // for the disk write + Apply step.
    let paths = SubscriptionPaths::from_app(&state.paths);
    let mgr = SubscriptionManager::open(paths, host_platform());
    let fetched = match mgr.fetch_update(req.id) {
        Ok(upd) => upd,
        Err(err) => {
            // Keep the pre-split behavior: record `last_error` on a failed fetch.
            let _orch = lock_orchestrate(state)?;
            write_subscription_error(mgr.paths(), req.id, err.ui_message())
                .map_err(AppError::from)?;
            return Err(AppError::from(err));
        }
    };

    let _orch = lock_orchestrate(state)?;
    let meta = mgr.apply_update(fetched).map_err(AppError::from)?;

    let settings = current_settings(&state.paths)?;
    let apply_warning = apply_after_subscription_change(app, state, &settings);
    let mut value = serde_json::to_value(meta)
        .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("serialize: {e}")))?;
    attach_apply_warning(&mut value, apply_warning);
    drop(_orch);
    broadcast_state_change(app);
    Ok(value)
}

pub(crate) fn update_all_subscriptions_use_case(
    app: &impl AppHost,
    state: &AppState,
) -> Result<serde_json::Value, AppError> {
    // Fetches (parallel, up to one FETCH_TIMEOUT) run without the orchestrate lock so a
    // long batch doesn't queue Start/Stop/Settings behind it. The lock is re-acquired for
    // the disk phase + Apply step so a concurrent add/remove/set_active cannot interleave
    // with the subscription writes (atomic file renames keep readers consistent, and
    // `apply_update` refuses to resurrect a subscription removed mid-flight).
    let paths = SubscriptionPaths::from_app(&state.paths);
    let mgr = SubscriptionManager::open(paths, host_platform());
    let fetched = mgr.fetch_all();

    let _orch = lock_orchestrate(state)?;
    let results = mgr.apply_all(fetched);
    let settings = current_settings(&state.paths)?;
    let apply_warning = apply_after_subscription_change(app, state, &settings);
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
    drop(_orch);
    broadcast_state_change(app);
    Ok(value)
}

pub(crate) fn set_active_subscription_use_case(
    app: &impl AppHost,
    state: &AppState,
    req: SetActiveRequest,
) -> Result<serde_json::Value, AppError> {
    let _orch = lock_orchestrate(state)?;
    let paths = SubscriptionPaths::from_app(&state.paths);
    let meta = ice_engine::set_active(&paths, req.id, req.active).map_err(AppError::from)?;

    let settings = current_settings(&state.paths)?;
    let apply_warning = apply_after_subscription_change(app, state, &settings);
    let mut value = serde_json::to_value(meta)
        .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("serialize: {e}")))?;
    attach_apply_warning(&mut value, apply_warning);
    drop(_orch);
    broadcast_state_change(app);
    Ok(value)
}

#[derive(Deserialize)]
pub struct SubscriptionShareRequest {
    pub id: Uuid,
    pub kind: SubscriptionShareKind,
}

#[derive(Deserialize)]
pub struct ExportSubscriptionRequest {
    pub id: Uuid,
    pub name: String,
    pub title: String,
}

/// Suggested file name for a sing-box export. Path characters are replaced so
/// the save dialog cannot be pointed at another directory by the name alone.
pub(crate) fn singbox_export_filename(name: &str) -> String {
    let mut cleaned = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_control() || ch == '\u{7f}' {
            continue;
        }
        if matches!(ch, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            cleaned.push('_');
            continue;
        }
        cleaned.push(ch);
    }
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        return "sing-box.json".to_string();
    }
    if has_json_extension(trimmed) {
        trimmed.to_string()
    } else {
        format!("{trimmed}.json")
    }
}

fn has_json_extension(name: &str) -> bool {
    name.get(name.len().saturating_sub(5)..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".json"))
}

/// Unredacted subscription URL, or a portable sing-box document.
///
/// The list command keeps URLs redacted. This is the explicit share action;
/// the returned text must not be logged.
pub(crate) fn subscription_share_use_case(
    state: &AppState,
    req: SubscriptionShareRequest,
) -> Result<String, AppError> {
    let paths = SubscriptionPaths::from_app(&state.paths);
    subscription_share_text(&paths, req.id, req.kind).map_err(AppError::from)
}

pub(crate) fn set_auto_update_subscription_use_case(
    state: &AppState,
    req: SetAutoUpdateRequest,
) -> Result<serde_json::Value, AppError> {
    let _orch = lock_orchestrate(state)?;
    let paths = SubscriptionPaths::from_app(&state.paths);
    let meta =
        ice_engine::set_auto_update(&paths, req.id, req.auto_update, req.auto_update_interval)
            .map_err(AppError::from)?;
    tracing::info!(
        id = %req.id,
        auto_update = req.auto_update,
        interval = ?req.auto_update_interval,
        "set_auto_update_subscription"
    );
    serde_json::to_value(meta)
        .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("serialize: {e}")))
}

#[cfg(test)]
mod tests {
    use super::singbox_export_filename;

    #[test]
    fn export_filename_is_a_json_basename() {
        assert_eq!(singbox_export_filename("a/b:c"), "a_b_c.json");
        assert_eq!(singbox_export_filename("  "), "sing-box.json");
        assert_eq!(singbox_export_filename("node.json"), "node.json");
        assert_eq!(singbox_export_filename("Node.JSON"), "Node.JSON");
        assert_eq!(singbox_export_filename("日本"), "日本.json");
        assert_eq!(singbox_export_filename(".hidden."), "hidden.json");
        assert_eq!(singbox_export_filename("foo\nbar"), "foobar.json");
    }
}
