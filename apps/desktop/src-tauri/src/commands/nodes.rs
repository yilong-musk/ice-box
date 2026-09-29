// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[tauri::command]
pub async fn list_nodes(app: AppHandle) -> Result<Vec<NodeInfo>, AppError> {
    run_blocking("list_nodes", move || {
        let state = app.state::<AppState>();
        collect_nodes(state.inner())
    })
    .await
}

#[tauri::command]
pub async fn get_rule_overview(app: AppHandle) -> Result<RuleOverview, AppError> {
    run_blocking("get_rule_overview", move || {
        let state = app.state::<AppState>();
        rule_overview(state.inner())
    })
    .await
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

/// Disable / re-enable a rule (subscription or custom). Persisted by fingerprint so the
/// state survives subscription updates; Apply regenerates config (hot reload when running).
#[tauri::command]
pub async fn set_rule_disabled(
    app: AppHandle,
    req: SetRuleDisabledRequest,
) -> Result<serde_json::Value, AppError> {
    run_blocking("set_rule_disabled", move || {
        let state = app.state::<AppState>();
        set_rule_disabled_use_case(&app, state.inner(), req)
    })
    .await
}

/// Add a user-defined rule, prepended ahead of subscription rules at build time.
#[tauri::command]
pub async fn add_custom_rule(
    app: AppHandle,
    req: AddCustomRuleRequest,
) -> Result<serde_json::Value, AppError> {
    run_blocking("add_custom_rule", move || {
        let state = app.state::<AppState>();
        add_custom_rule_use_case(&app, state.inner(), req)
    })
    .await
}

/// Remove a user-added rule (also clears its disabled mark).
#[tauri::command]
pub async fn remove_custom_rule(
    app: AppHandle,
    req: RemoveCustomRuleRequest,
) -> Result<serde_json::Value, AppError> {
    run_blocking("remove_custom_rule", move || {
        let state = app.state::<AppState>();
        remove_custom_rule_use_case(&app, state.inner(), req)
    })
    .await
}

#[tauri::command]
pub async fn set_selected_node(app: AppHandle, req: TagRequest) -> Result<(), AppError> {
    run_blocking("set_selected_node", move || {
        let state = app.state::<AppState>();
        set_selected_node_use_case(&app, state.inner(), req)
    })
    .await
}

/// Switch a strategy group member: persists the selection always, and applies
/// it live when the core is running. Same path as the tray node menu.
#[tauri::command]
pub async fn set_group_selection(
    app: AppHandle,
    req: GroupSelectionRequest,
) -> Result<(), AppError> {
    run_blocking("set_group_selection", move || {
        let state = app.state::<AppState>();
        set_group_selection_use_case(&app, state.inner(), req)
    })
    .await
}

#[tauri::command]
pub async fn test_node_delay(
    app: AppHandle,
    req: TagRequest,
) -> Result<DelayTestResponse, AppError> {
    run_blocking("test_node_delay", move || {
        let state = app.state::<AppState>();
        test_node_delay_use_case(state.inner(), req)
    })
    .await
}
