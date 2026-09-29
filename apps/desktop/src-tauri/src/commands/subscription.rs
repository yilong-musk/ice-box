// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[tauri::command]
pub async fn list_subscriptions(app: AppHandle) -> Result<serde_json::Value, AppError> {
    run_blocking("list_subscriptions", move || {
        let state = app.state::<AppState>();
        list_subscriptions_use_case(state.inner())
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
        add_subscription_use_case(&app, state.inner(), req)
    })
    .await
}

#[tauri::command]
pub async fn remove_subscription(
    app: AppHandle,
    req: IdRequest,
) -> Result<serde_json::Value, AppError> {
    run_blocking("remove_subscription", move || {
        let state = app.state::<AppState>();
        remove_subscription_use_case(&app, state.inner(), req)
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
        update_subscription_use_case(&app, state.inner(), req)
    })
    .await
}

#[tauri::command]
pub async fn update_all_subscriptions(app: AppHandle) -> Result<serde_json::Value, AppError> {
    run_blocking("update_all_subscriptions", move || {
        let state = app.state::<AppState>();
        update_all_subscriptions_use_case(&app, state.inner())
    })
    .await
}

#[tauri::command]
pub async fn set_active_subscription(
    app: AppHandle,
    req: SetActiveRequest,
) -> Result<serde_json::Value, AppError> {
    run_blocking("set_active_subscription", move || {
        let state = app.state::<AppState>();
        set_active_subscription_use_case(&app, state.inner(), req)
    })
    .await
}

#[tauri::command]
pub async fn set_auto_update_subscription(
    app: AppHandle,
    req: SetAutoUpdateRequest,
) -> Result<serde_json::Value, AppError> {
    run_blocking("set_auto_update_subscription", move || {
        let state = app.state::<AppState>();
        set_auto_update_subscription_use_case(state.inner(), req)
    })
    .await
}
