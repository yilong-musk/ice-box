// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

use tauri_plugin_dialog::DialogExt;

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
pub async fn subscription_share(
    app: AppHandle,
    req: SubscriptionShareRequest,
) -> Result<String, AppError> {
    run_blocking("subscription_share", move || {
        let state = app.state::<AppState>();
        subscription_share_use_case(state.inner(), req)
    })
    .await
}

/// Save one subscription's sing-box document through the native save dialog.
///
/// `"cancelled"` when the dialog is dismissed. The document is not logged.
#[tauri::command]
pub async fn export_subscription_singbox(
    app: AppHandle,
    req: ExportSubscriptionRequest,
) -> Result<String, AppError> {
    let id = req.id;
    let filename = singbox_export_filename(&req.name);
    let title = req.title;
    let text = run_blocking("export_subscription_singbox", {
        let app = app.clone();
        move || {
            let state = app.state::<AppState>();
            subscription_share_use_case(
                state.inner(),
                SubscriptionShareRequest {
                    id,
                    kind: SubscriptionShareKind::Singbox,
                },
            )
        }
    })
    .await?;

    let mut dialog = app
        .dialog()
        .file()
        .add_filter("JSON", &["json"])
        .set_file_name(&filename);
    let trimmed_title = title.trim();
    if !trimmed_title.is_empty() {
        dialog = dialog.set_title(trimmed_title);
    }
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    let Some(picked) = dialog.blocking_save_file() else {
        return Ok("cancelled".into());
    };
    let path = with_json_extension(picked.into_path().map_err(|err| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("export sing-box config: {err}"),
        )
    })?);
    run_blocking("export_subscription_singbox_write", move || {
        std::fs::write(&path, text.as_bytes()).map_err(|err| {
            AppError::new(ErrorCode::SubIo, format!("export sing-box config: {err}"))
        })
    })
    .await?;
    Ok("saved".into())
}

fn with_json_extension(path: PathBuf) -> PathBuf {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("json") => path,
        _ => path.with_extension("json"),
    }
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
