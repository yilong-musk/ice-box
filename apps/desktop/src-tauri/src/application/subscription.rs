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

/// Manager whose subscription fetches follow the system proxy, like a browser.
pub(crate) fn subscription_manager(
    state: &AppState,
) -> Result<SubscriptionManager<DefaultFetcher>, AppError> {
    let proxy = subscription_fetch_proxy(state);
    match SubscriptionManager::open_with_proxy_config(
        SubscriptionPaths::from_app(&state.paths),
        host_platform(),
        proxy,
    ) {
        Ok(mgr) => Ok(mgr),
        Err(err) => {
            // A malformed system proxy must not disable subscription updates
            // entirely. Retry directly; the fetch will still validate its URL.
            tracing::warn!(
                error = %err.redacted_display(),
                "subscription fetch: proxy setup failed; connecting directly"
            );
            SubscriptionManager::open_with_proxy_config(
                SubscriptionPaths::from_app(&state.paths),
                host_platform(),
                SubscriptionProxy::default(),
            )
            .map_err(AppError::from)
        }
    }
}

/// System proxy endpoints for subscription fetches.
///
/// The app applies its local mixed inbound as the OS system proxy while the
/// proxy service runs, so reading the OS settings is what a browser does: the
/// fetch follows the same proxy when one is on and goes direct when none is.
fn subscription_fetch_proxy(state: &AppState) -> SubscriptionProxy {
    let backend = match state.proxy.lock() {
        Ok(backend) => backend,
        Err(_) => {
            tracing::warn!("subscription fetch: proxy state lock poisoned; connecting directly");
            return SubscriptionProxy::default();
        }
    };
    match backend.backup() {
        Ok(backup) => proxy_url_from_backup(&backup),
        Err(err) => {
            tracing::warn!(
                error = %err,
                "subscription fetch: reading the system proxy failed; connecting directly"
            );
            SubscriptionProxy::default()
        }
    }
}

/// Pick the endpoints a browser would use. The fetcher prefers the endpoint
/// matching each URL scheme and falls back to the other scheme, then SOCKS.
///
/// Only explicit endpoints are used: PAC/WPAD scripts and the OS bypass list
/// are not evaluated.
fn proxy_url_from_backup(backup: &ProxyBackup) -> SubscriptionProxy {
    if !backup.enabled {
        return SubscriptionProxy::default();
    }
    SubscriptionProxy {
        http: first_non_empty([backup.http.as_deref()])
            .map(|endpoint| with_proxy_scheme("http", endpoint)),
        https: first_non_empty([backup.https.as_deref()])
            .map(|endpoint| with_proxy_scheme("http", endpoint)),
        socks: first_non_empty([backup.socks.as_deref()])
            .map(|endpoint| with_proxy_scheme("socks5", endpoint)),
    }
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> Option<&'a str> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
}

fn with_proxy_scheme(default_scheme: &str, endpoint: &str) -> String {
    if endpoint.contains("://") {
        endpoint.to_string()
    } else {
        format!("{default_scheme}://{endpoint}")
    }
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
    // Fetch (up to FETCH_TOTAL_TIMEOUT) runs without the orchestrate lock so Start/Stop/Apply/
    // save_settings are not queued behind it; the lock is taken for the disk write + Apply.
    let mgr = subscription_manager(state)?;
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
    ice_engine::remove_subscription(&paths, req.id).map_err(|err| {
        tracing::warn!(
            id = %req.id,
            error = %err.redacted_display(),
            code = %err.code().as_str(),
            "remove_subscription: failed"
        );
        AppError::from(err)
    })?;
    if let Err(err) = remove_subscription_favorites(&state.paths.node_favorites(), req.id) {
        tracing::warn!(id = %req.id, error = %err.message, "remove_subscription: purge favorites failed");
    }

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
    // Fetch (up to FETCH_TOTAL_TIMEOUT) runs without the orchestrate lock; the lock is taken
    // for the disk write + Apply step.
    let mgr = subscription_manager(state)?;
    let fetched = match mgr.fetch_update(req.id) {
        Ok(upd) => upd,
        Err(err) => {
            tracing::warn!(
                id = %req.id,
                error = %err.redacted_display(),
                code = %err.code().as_str(),
                "update_subscription: fetch failed"
            );
            // Keep the pre-split behavior: record `last_error` on a failed fetch.
            let recorded = {
                let _orch = lock_orchestrate(state)?;
                write_subscription_error(mgr.paths(), req.id, err.ui_message())
                    .map_err(AppError::from)
            };
            if recorded.is_ok() {
                broadcast_state_change(app);
            }
            recorded?;
            return Err(AppError::from(err));
        }
    };

    let _orch = lock_orchestrate(state)?;
    let meta = mgr.apply_update(fetched).map_err(|err| {
        tracing::warn!(
            id = %req.id,
            error = %err.redacted_display(),
            code = %err.code().as_str(),
            "update_subscription: apply failed"
        );
        AppError::from(err)
    })?;

    let settings = current_settings(&state.paths)?;
    let apply_warning = apply_after_subscription_change(app, state, &settings);
    let mut value = serde_json::to_value(meta)
        .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("serialize: {e}")))?;
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
    let meta = ice_engine::set_active(&paths, req.id, req.active).map_err(|err| {
        tracing::warn!(
            id = %req.id,
            active = req.active,
            error = %err.redacted_display(),
            code = %err.code().as_str(),
            "set_active_subscription: failed"
        );
        AppError::from(err)
    })?;

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

/// Copy the share payload with the system clipboard.
///
/// The webview clipboard needs the original click, and this text is only
/// available after a disk read. The returned text is not logged.
pub(crate) fn copy_subscription_share_use_case(
    state: &AppState,
    req: SubscriptionShareRequest,
) -> Result<(), AppError> {
    let text = subscription_share_use_case(state, req)?;
    crate::proxy_terminal::copy_to_clipboard(&text)
}

pub(crate) fn set_auto_update_subscription_use_case(
    app: &impl AppHost,
    state: &AppState,
    req: SetAutoUpdateRequest,
) -> Result<serde_json::Value, AppError> {
    let result = (|| {
        let _orch = lock_orchestrate(state)?;
        let paths = SubscriptionPaths::from_app(&state.paths);
        let meta =
            ice_engine::set_auto_update(&paths, req.id, req.auto_update, req.auto_update_interval)
                .map_err(|err| {
                    tracing::warn!(
                        id = %req.id,
                        auto_update = req.auto_update,
                        interval = ?req.auto_update_interval,
                        error = %err.redacted_display(),
                        code = %err.code().as_str(),
                        "set_auto_update_subscription: failed"
                    );
                    AppError::from(err)
                })?;
        tracing::info!(
            id = %req.id,
            auto_update = req.auto_update,
            interval = ?req.auto_update_interval,
            "set_auto_update_subscription"
        );
        serde_json::to_value(meta)
            .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("serialize: {e}")))
    })();
    broadcast_state_change(app);
    result
}

#[cfg(test)]
mod tests {
    use super::{proxy_url_from_backup, singbox_export_filename};
    use crate::application::{ProxyBackup, SubscriptionProxy};

    fn assert_proxy(
        proxy: &SubscriptionProxy,
        http: Option<&str>,
        https: Option<&str>,
        socks: Option<&str>,
    ) {
        assert_eq!(proxy.http.as_deref(), http);
        assert_eq!(proxy.https.as_deref(), https);
        assert_eq!(proxy.socks.as_deref(), socks);
    }

    #[test]
    fn subscription_proxy_follows_the_system_browser_proxy() {
        let disabled = ProxyBackup::default();
        assert_proxy(&proxy_url_from_backup(&disabled), None, None, None);

        let backup = ProxyBackup {
            enabled: true,
            http: Some("10.0.0.2:8080".into()),
            https: Some("127.0.0.1:17890".into()),
            ..Default::default()
        };
        assert_proxy(
            &proxy_url_from_backup(&backup),
            Some("http://10.0.0.2:8080"),
            Some("http://127.0.0.1:17890"),
            None,
        );

        let https_only = ProxyBackup {
            enabled: true,
            https: Some("127.0.0.1:17890".into()),
            ..Default::default()
        };
        assert_proxy(
            &proxy_url_from_backup(&https_only),
            None,
            Some("http://127.0.0.1:17890"),
            None,
        );

        let http_only = ProxyBackup {
            enabled: true,
            http: Some("10.0.0.2:8080".into()),
            ..Default::default()
        };
        assert_proxy(
            &proxy_url_from_backup(&http_only),
            Some("http://10.0.0.2:8080"),
            None,
            None,
        );

        let socks = ProxyBackup {
            enabled: true,
            socks: Some("127.0.0.1:1080".into()),
            ..Default::default()
        };
        assert_proxy(
            &proxy_url_from_backup(&socks),
            None,
            None,
            Some("socks5://127.0.0.1:1080"),
        );

        let socks_with_scheme = ProxyBackup {
            enabled: true,
            socks: Some("socks5://127.0.0.1:1080".into()),
            ..Default::default()
        };
        assert_proxy(
            &proxy_url_from_backup(&socks_with_scheme),
            None,
            None,
            Some("socks5://127.0.0.1:1080"),
        );
    }

    #[test]
    fn enabled_proxy_without_an_endpoint_connects_directly() {
        let proxy_enabled = ProxyBackup {
            enabled: true,
            ..Default::default()
        };
        assert_proxy(&proxy_url_from_backup(&proxy_enabled), None, None, None);

        // PAC/WPAD-only settings are not evaluated; the fetch goes direct.
        let pac_only = ProxyBackup {
            enabled: true,
            extra: serde_json::json!({ "autoconfig_url": "http://wpad/wpad.dat" }),
            ..Default::default()
        };
        assert_proxy(&proxy_url_from_backup(&pac_only), None, None, None);
    }

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
