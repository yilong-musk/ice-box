// SPDX-License-Identifier: GPL-3.0-or-later

use super::settings::{apply_after_subscription_change, attach_apply_warning};
use super::*;

#[derive(Deserialize)]
pub struct AddSubscriptionRequest {
    pub url: String,
    pub name: Option<String>,
    #[serde(default)]
    pub auto_update: bool,
    #[serde(default)]
    pub auto_update_interval: Option<ice_subscription::AutoUpdateInterval>,
}

#[tauri::command]
pub async fn list_subscriptions(app: AppHandle) -> Result<serde_json::Value, AppError> {
    run_blocking("list_subscriptions", move || {
        let state = app.state::<AppState>();
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
    })
    .await
}

#[tauri::command]
pub async fn add_subscription(
    app: AppHandle,
    req: AddSubscriptionRequest,
) -> Result<serde_json::Value, AppError> {
    run_blocking("add_subscription", move || {
        let state = app.state::<AppState>();
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

        let _orch = lock_orchestrate(&state)?;
        let meta = mgr.apply_add(fetched).map_err(|e| {
            tracing::warn!(url = %redacted, error = %e.redacted_display(), code = %e.code().as_str(), "add_subscription: apply failed");
            AppError::from(e)
        })?;
        tracing::info!(url = %redacted, id = %meta.id, name = %meta.name, nodes = meta.node_count, format = ?meta.format, "add_subscription: imported");

        let settings = current_settings(&state.paths)?;
        let apply_warning = apply_after_subscription_change(&app, &state, &settings);
        if let Some(w) = &apply_warning {
            tracing::warn!(code = %w.code, error = %w.message, "add_subscription: apply warning");
        }
        let mut value = serde_json::to_value(meta)
            .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("serialize: {e}")))?;
        attach_apply_warning(&mut value, apply_warning);
        Ok(value)
    })
    .await
}

#[derive(Deserialize)]
pub struct IdRequest {
    pub id: Uuid,
}

#[tauri::command]
pub async fn remove_subscription(
    app: AppHandle,
    req: IdRequest,
) -> Result<serde_json::Value, AppError> {
    run_blocking("remove_subscription", move || {
        let state = app.state::<AppState>();
        let _orch = lock_orchestrate(&state)?;
        let paths = SubscriptionPaths::from_app(&state.paths);
        ice_subscription::remove_subscription(&paths, req.id).map_err(AppError::from)?;

        let settings = current_settings(&state.paths)?;
        let apply_warning = apply_after_subscription_change(&app, &state, &settings);
        let mut value = serde_json::json!({ "ok": true });
        attach_apply_warning(&mut value, apply_warning);
        Ok(value)
    })
    .await
}

#[tauri::command]
pub async fn update_subscription(
    app: AppHandle,
    req: IdRequest,
) -> Result<serde_json::Value, AppError> {
    run_blocking("update_subscription", move || {
        let state = app.state::<AppState>();
        // Fetch (up to FETCH_TIMEOUT) runs without the orchestrate lock; the lock is taken
        // for the disk write + Apply step.
        let paths = SubscriptionPaths::from_app(&state.paths);
        let mgr = SubscriptionManager::open(paths, host_platform());
        let fetched = match mgr.fetch_update(req.id) {
            Ok(upd) => upd,
            Err(err) => {
                // Keep the pre-split behavior: record `last_error` on a failed fetch.
                let _orch = lock_orchestrate(&state)?;
                write_subscription_error(mgr.paths(), req.id, err.to_string())
                    .map_err(AppError::from)?;
                return Err(AppError::from(err));
            }
        };

        let _orch = lock_orchestrate(&state)?;
        let meta = mgr.apply_update(fetched).map_err(AppError::from)?;

        let settings = current_settings(&state.paths)?;
        let apply_warning = apply_after_subscription_change(&app, &state, &settings);
        let mut value = serde_json::to_value(meta)
            .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("serialize: {e}")))?;
        attach_apply_warning(&mut value, apply_warning);
        Ok(value)
    })
    .await
}

#[tauri::command]
pub async fn update_all_subscriptions(app: AppHandle) -> Result<serde_json::Value, AppError> {
    run_blocking("update_all_subscriptions", move || {
        let state = app.state::<AppState>();
        // Fetches (parallel, up to one FETCH_TIMEOUT) run without the orchestrate lock so a
        // long batch doesn't queue Start/Stop/Settings behind it. The lock is re-acquired for
        // the disk phase + Apply step so a concurrent add/remove/set_active cannot interleave
        // with the subscription writes (atomic file renames keep readers consistent, and
        // `apply_update` refuses to resurrect a subscription removed mid-flight).
        let paths = SubscriptionPaths::from_app(&state.paths);
        let mgr = SubscriptionManager::open(paths, host_platform());
        let fetched = mgr.fetch_all();

        let _orch = lock_orchestrate(&state)?;
        let results = mgr.apply_all(fetched);
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
    })
    .await
}

#[derive(Deserialize)]
pub struct SetActiveRequest {
    pub id: Uuid,
    pub active: bool,
}

#[tauri::command]
pub async fn set_active_subscription(
    app: AppHandle,
    req: SetActiveRequest,
) -> Result<serde_json::Value, AppError> {
    run_blocking("set_active_subscription", move || {
        let state = app.state::<AppState>();
        let _orch = lock_orchestrate(&state)?;
        let paths = SubscriptionPaths::from_app(&state.paths);
        let meta =
            ice_subscription::set_active(&paths, req.id, req.active).map_err(AppError::from)?;

        let settings = current_settings(&state.paths)?;
        let apply_warning = apply_after_subscription_change(&app, &state, &settings);
        let mut value = serde_json::to_value(meta)
            .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("serialize: {e}")))?;
        attach_apply_warning(&mut value, apply_warning);
        Ok(value)
    })
    .await
}

#[derive(Deserialize)]
pub struct SetAutoUpdateRequest {
    pub id: Uuid,
    pub auto_update: bool,
    #[serde(default)]
    pub auto_update_interval: Option<ice_subscription::AutoUpdateInterval>,
}

#[tauri::command]
pub async fn set_auto_update_subscription(
    app: AppHandle,
    req: SetAutoUpdateRequest,
) -> Result<serde_json::Value, AppError> {
    run_blocking("set_auto_update_subscription", move || {
        let state = app.state::<AppState>();
        let _orch = lock_orchestrate(&state)?;
        let paths = SubscriptionPaths::from_app(&state.paths);
        let meta = ice_subscription::set_auto_update(
            &paths,
            req.id,
            req.auto_update,
            req.auto_update_interval,
        )
        .map_err(AppError::from)?;
        tracing::info!(
            id = %req.id,
            auto_update = req.auto_update,
            interval = ?req.auto_update_interval,
            "set_auto_update_subscription"
        );
        serde_json::to_value(meta)
            .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("serialize: {e}")))
    })
    .await
}
