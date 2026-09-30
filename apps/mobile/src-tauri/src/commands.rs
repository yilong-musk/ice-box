// SPDX-License-Identifier: GPL-3.0-or-later

//! Commands the shared React UI already calls. Desktop-only commands are not
//! registered; the phone transport does not invoke them.

use std::sync::Mutex;

use ice_config::{AppError, AppPaths, ErrorCode, ProxyMode, SettingsPatch};
use ice_subscription::{AutoUpdateInterval, SubscriptionManager};
use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager, State, Wry};
use tauri_plugin_tunnel::{Tunnel, TunnelError};
use uuid::Uuid;

use crate::config::PLATFORM;
use crate::host::{
    stopped_view, tunnel_view, ListRulesRequest, ListRulesResponse, MobileHost, NodeInfo,
    RemoveResult, RuleMutation, RuleOverview,
};
use crate::status::{StatusResponse, TunnelPhase, VpnPermission};

#[derive(Deserialize)]
pub struct TagRequest {
    pub tag: String,
}

#[derive(Deserialize)]
pub struct GroupSelectionRequest {
    pub group: String,
    pub member: String,
}

#[derive(Deserialize)]
pub struct AddSubscriptionRequest {
    pub url: String,
    pub name: Option<String>,
    #[serde(default)]
    pub auto_update: bool,
    #[serde(default)]
    pub auto_update_interval: Option<AutoUpdateInterval>,
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
pub struct SetProxyModeRequest {
    pub mode: ProxyMode,
}

#[derive(Deserialize)]
pub struct LogViewRequest {
    pub n: usize,
}

#[derive(Deserialize)]
pub struct SetRuleDisabledRequest {
    pub fingerprint: String,
    pub disabled: bool,
}

#[derive(Deserialize)]
pub struct AddCustomRuleRequest {
    pub rule: serde_json::Value,
}

#[derive(Deserialize)]
pub struct FingerprintRequest {
    pub fingerprint: String,
}

struct TunnelRead {
    permission: String,
    package_name: Option<String>,
}

fn map_tunnel(err: TunnelError) -> AppError {
    match err {
        TunnelError::Unavailable => AppError::new(ErrorCode::TunNotSupported, err.to_string()),
        TunnelError::Plugin(message) => AppError::new(ErrorCode::TunApplyFailed, message),
    }
}

fn lock<'a>(
    host: &'a State<'_, Mutex<MobileHost>>,
) -> Result<std::sync::MutexGuard<'a, MobileHost>, AppError> {
    host.lock()
        .map_err(|_| AppError::new(ErrorCode::LockPoisoned, "host lock poisoned"))
}

fn ready<'a>(
    app: &AppHandle,
    host: &'a State<'_, Mutex<MobileHost>>,
    tunnel: &State<'_, Tunnel<Wry>>,
) -> Result<std::sync::MutexGuard<'a, MobileHost>, AppError> {
    let mut guard = lock(host)?;
    if guard.paths_ready() {
        return Ok(guard);
    }
    let private = app
        .path()
        .app_data_dir()
        .map_err(|err| AppError::new(ErrorCode::ConfigInvalid, format!("app data dir: {err}")))?;
    let shared = match tunnel.shared_dir() {
        Ok(path) => Some(path),
        Err(TunnelError::Unavailable) => None,
        Err(err) => return Err(map_tunnel(err)),
    };
    guard.ensure_paths(AppPaths::new(private), shared)?;
    Ok(guard)
}

fn read_tunnel(tunnel: &Tunnel<Wry>) -> Result<TunnelRead, AppError> {
    match tunnel.status() {
        Ok(snapshot) => Ok(TunnelRead {
            permission: snapshot.permission,
            package_name: Some(snapshot.package_name).filter(|name| !name.is_empty()),
        }),
        Err(err) => Err(map_tunnel(err)),
    }
}

fn package_name(tunnel: &Tunnel<Wry>) -> Option<String> {
    read_tunnel(tunnel).ok().and_then(|read| read.package_name)
}

fn apply_config(host: &mut MobileHost, tunnel: &Tunnel<Wry>) -> Result<(), AppError> {
    match tunnel.status() {
        Ok(snapshot) => {
            let live = TunnelPhase::parse(&snapshot.phase).is_live();
            let package = Some(snapshot.package_name).filter(|name| !name.is_empty());
            host.rewrite_config(package.as_deref())?;
            if live {
                tunnel.reload().map_err(map_tunnel)?;
            }
            Ok(())
        }
        Err(TunnelError::Unavailable) => host.rewrite_config(None),
        Err(err) => Err(map_tunnel(err)),
    }
}

fn notify(app: &AppHandle) {
    let _ = app.emit("core://status-changed", ());
    let _ = app.emit("app://state-changed", ());
}

/// Blocking IPC must not run on the UI thread. A subscription fetch holds the
/// WebView for the whole timeout and Android reports the app as not responding.
async fn run_blocking<T: Send + 'static>(
    context: &'static str,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| AppError::new(ErrorCode::ConfigInvalid, format!("{context}: {err}")))
}

fn guidance_of(tunnel: &Tunnel<Wry>) -> Option<crate::status::DeviceGuidance> {
    let status = tunnel.device_status().ok()?;
    Some(crate::status::DeviceGuidance {
        battery_unrestricted: status.battery_unrestricted,
        private_dns_strict: status.private_dns_strict,
        always_on_vpn: status.always_on_vpn,
    })
}

#[tauri::command]
pub fn get_status(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
) -> Result<StatusResponse, AppError> {
    let mut host = ready(&app, &host, &tunnel)?;
    let (view, message, memory) = match tunnel.status() {
        Ok(snapshot) => (
            tunnel_view(&snapshot.phase, &snapshot.permission),
            snapshot.message,
            snapshot.memory_bytes,
        ),
        Err(TunnelError::Unavailable) => (stopped_view(), None, None),
        Err(TunnelError::Plugin(message)) => (tunnel_view("error", "unknown"), Some(message), None),
    };
    host.status(view, message.as_deref(), memory, guidance_of(&tunnel))
}

#[tauri::command]
pub fn start(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
) -> Result<(), AppError> {
    let mut host = ready(&app, &host, &tunnel)?;
    let current = read_tunnel(&tunnel)?;
    if VpnPermission::parse(&current.permission) != VpnPermission::Granted {
        let permission = tunnel.prepare().map_err(map_tunnel)?;
        if permission != "granted" {
            return Err(AppError::new(
                ErrorCode::TunPermissionRequired,
                "VPN permission denied",
            ));
        }
    }
    let package = package_name(&tunnel).or(current.package_name);
    host.rewrite_config(package.as_deref())?;
    tunnel.start().map_err(map_tunnel)?;
    host.set_service_enabled(true)?;
    drop(host);
    notify(&app);
    Ok(())
}

#[tauri::command]
pub fn stop(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
) -> Result<(), AppError> {
    let mut host = ready(&app, &host, &tunnel)?;
    let _ = read_tunnel(&tunnel)?;
    tunnel
        .stop()
        .map_err(|err| AppError::new(ErrorCode::TunRestoreFailed, err.to_string()))?;
    host.set_service_enabled(false)?;
    drop(host);
    notify(&app);
    Ok(())
}

#[tauri::command]
pub fn list_subscriptions(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
) -> Result<Vec<serde_json::Value>, AppError> {
    let host = ready(&app, &host, &tunnel)?;
    host.list_subscriptions()
}

#[tauri::command]
pub async fn add_subscription(
    app: AppHandle,
    req: AddSubscriptionRequest,
) -> Result<serde_json::Value, AppError> {
    let paths = {
        let host = app.state::<Mutex<MobileHost>>();
        let tunnel = app.state::<Tunnel<Wry>>();
        let guard = ready(&app, &host, &tunnel)?;
        guard.subscription_paths()?
    };
    let url = req.url;
    let name = req.name;
    // The phone does not schedule subscription refreshes. The flags stay on
    // the request so the shared form can send them; they are not stored.
    let _ = (req.auto_update, req.auto_update_interval);
    let fetched = run_blocking("add_subscription", move || {
        SubscriptionManager::open(paths, PLATFORM).fetch_add(&url, name.as_deref(), false, None)
    })
    .await?
    .map_err(AppError::from)?;
    let host = app.state::<Mutex<MobileHost>>();
    let tunnel = app.state::<Tunnel<Wry>>();
    let mut guard = ready(&app, &host, &tunnel)?;
    let meta = guard.apply_added(fetched)?;
    apply_config(&mut guard, &tunnel)?;
    drop(guard);
    notify(&app);
    Ok(meta)
}

#[tauri::command]
pub fn remove_subscription(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
    req: IdRequest,
) -> Result<RemoveResult, AppError> {
    let mut host = ready(&app, &host, &tunnel)?;
    host.remove_subscription(req.id)?;
    let apply_warning = apply_config(&mut host, &tunnel).err();
    drop(host);
    notify(&app);
    Ok(RemoveResult {
        ok: true,
        apply_warning,
    })
}

#[tauri::command]
pub async fn update_subscription(
    app: AppHandle,
    req: IdRequest,
) -> Result<serde_json::Value, AppError> {
    let paths = {
        let host = app.state::<Mutex<MobileHost>>();
        let tunnel = app.state::<Tunnel<Wry>>();
        let guard = ready(&app, &host, &tunnel)?;
        guard.subscription_paths()?
    };
    let id = req.id;
    let fetched = run_blocking("update_subscription", move || {
        SubscriptionManager::open(paths, PLATFORM).fetch_update(id)
    })
    .await?;
    let host = app.state::<Mutex<MobileHost>>();
    let tunnel = app.state::<Tunnel<Wry>>();
    let mut guard = ready(&app, &host, &tunnel)?;
    let meta = match fetched {
        Ok(update) => guard.apply_updated(update)?,
        Err(err) => {
            guard.note_subscription_error(id, err.ui_message())?;
            drop(guard);
            notify(&app);
            return Err(err.into());
        }
    };
    apply_config(&mut guard, &tunnel)?;
    drop(guard);
    notify(&app);
    Ok(meta)
}

#[tauri::command]
pub async fn update_all_subscriptions(app: AppHandle) -> Result<serde_json::Value, AppError> {
    let paths = {
        let host = app.state::<Mutex<MobileHost>>();
        let tunnel = app.state::<Tunnel<Wry>>();
        let guard = ready(&app, &host, &tunnel)?;
        guard.subscription_paths()?
    };
    let fetched = run_blocking("update_all_subscriptions", move || {
        SubscriptionManager::open(paths, PLATFORM).fetch_all()
    })
    .await?;
    let host = app.state::<Mutex<MobileHost>>();
    let tunnel = app.state::<Tunnel<Wry>>();
    let mut guard = ready(&app, &host, &tunnel)?;
    let report = guard.apply_updates(fetched)?;
    apply_config(&mut guard, &tunnel)?;
    drop(guard);
    notify(&app);
    Ok(report)
}

#[tauri::command]
pub fn set_active_subscription(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
    req: SetActiveRequest,
) -> Result<serde_json::Value, AppError> {
    let mut host = ready(&app, &host, &tunnel)?;
    let meta = host.set_active(req.id, req.active)?;
    apply_config(&mut host, &tunnel)?;
    drop(host);
    notify(&app);
    Ok(meta)
}

#[tauri::command]
pub fn list_nodes(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
) -> Result<Vec<NodeInfo>, AppError> {
    let host = ready(&app, &host, &tunnel)?;
    let live = tunnel
        .status()
        .ok()
        .map(|snapshot| TunnelPhase::parse(&snapshot.phase) == TunnelPhase::Connected)
        .unwrap_or(false);
    host.list_nodes(live)
}

#[tauri::command]
pub fn set_selected_node(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
    req: TagRequest,
) -> Result<(), AppError> {
    let mut host = ready(&app, &host, &tunnel)?;
    host.set_selected_node(&req.tag)?;
    apply_config(&mut host, &tunnel)?;
    drop(host);
    notify(&app);
    Ok(())
}

#[tauri::command]
pub fn set_group_selection(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
    req: GroupSelectionRequest,
) -> Result<(), AppError> {
    let mut host = ready(&app, &host, &tunnel)?;
    host.set_group_selection(&req.group, &req.member)?;
    apply_config(&mut host, &tunnel)?;
    drop(host);
    notify(&app);
    Ok(())
}

#[tauri::command]
pub fn test_node_delay(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
    req: TagRequest,
) -> Result<DelayTestResponse, AppError> {
    let host = ready(&app, &host, &tunnel)?;
    let live = tunnel
        .status()
        .ok()
        .map(|snapshot| TunnelPhase::parse(&snapshot.phase) == TunnelPhase::Connected)
        .unwrap_or(false);
    let delay_ms = host.test_delay(&req.tag, live)?;
    Ok(DelayTestResponse {
        tag: req.tag,
        delay_ms,
    })
}

#[derive(serde::Serialize)]
pub struct DelayTestResponse {
    pub tag: String,
    pub delay_ms: u32,
}

#[tauri::command]
pub fn get_traffic_snapshot(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
) -> Result<ice_core::TrafficSnapshot, AppError> {
    let mut host = ready(&app, &host, &tunnel)?;
    let live = tunnel
        .status()
        .ok()
        .map(|snapshot| TunnelPhase::parse(&snapshot.phase) == TunnelPhase::Connected)
        .unwrap_or(false);
    host.traffic_snapshot(live)
}

#[tauri::command]
pub fn get_traffic_since(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
    cursor: Option<u64>,
) -> Result<ice_core::TrafficDelta, AppError> {
    let mut host = ready(&app, &host, &tunnel)?;
    let live = tunnel
        .status()
        .ok()
        .map(|snapshot| TunnelPhase::parse(&snapshot.phase) == TunnelPhase::Connected)
        .unwrap_or(false);
    host.traffic_since(cursor, live)
}

#[tauri::command]
pub fn get_log_view(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
    req: LogViewRequest,
) -> Result<Vec<String>, AppError> {
    let host = ready(&app, &host, &tunnel)?;
    host.log_view(req.n)
}

#[tauri::command]
pub fn get_runtime_config(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
) -> Result<String, AppError> {
    let host = ready(&app, &host, &tunnel)?;
    host.runtime_config()
}

#[tauri::command]
pub fn get_settings(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
) -> Result<ice_config::AppSettings, AppError> {
    let host = ready(&app, &host, &tunnel)?;
    host.settings()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
    patch: SettingsPatch,
) -> Result<(), AppError> {
    let mut host = ready(&app, &host, &tunnel)?;
    host.save_settings(&patch)?;
    apply_config(&mut host, &tunnel)?;
    drop(host);
    notify(&app);
    Ok(())
}

#[tauri::command]
pub fn set_proxy_mode(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
    req: SetProxyModeRequest,
) -> Result<(), AppError> {
    let mut host = ready(&app, &host, &tunnel)?;
    host.set_proxy_mode(req.mode)?;
    apply_config(&mut host, &tunnel)?;
    drop(host);
    notify(&app);
    Ok(())
}

#[tauri::command]
pub fn get_rule_overview(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
) -> Result<RuleOverview, AppError> {
    let host = ready(&app, &host, &tunnel)?;
    host.rule_overview()
}

#[tauri::command]
pub fn list_rules(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
    req: ListRulesRequest,
) -> Result<ListRulesResponse, AppError> {
    let host = ready(&app, &host, &tunnel)?;
    host.list_rules(&req)
}

#[tauri::command]
pub fn set_rule_disabled(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
    req: SetRuleDisabledRequest,
) -> Result<RuleMutation, AppError> {
    let mut host = ready(&app, &host, &tunnel)?;
    host.set_rule_disabled(&req.fingerprint, req.disabled)?;
    let apply_warning = apply_config(&mut host, &tunnel).err();
    drop(host);
    notify(&app);
    Ok(RuleMutation {
        ok: true,
        disabled: Some(req.disabled),
        fingerprint: None,
        apply_warning,
    })
}

#[tauri::command]
pub fn add_custom_rule(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
    req: AddCustomRuleRequest,
) -> Result<RuleMutation, AppError> {
    let mut host = ready(&app, &host, &tunnel)?;
    let fingerprint = host.add_custom_rule(req.rule)?;
    let apply_warning = apply_config(&mut host, &tunnel).err();
    drop(host);
    notify(&app);
    Ok(RuleMutation {
        ok: true,
        disabled: None,
        fingerprint: Some(fingerprint),
        apply_warning,
    })
}

#[tauri::command]
pub async fn check_app_update(
    app: AppHandle,
    req: crate::app_update::CheckAppUpdateRequest,
) -> Result<crate::app_update::CheckAppUpdateResponse, AppError> {
    let paths = {
        let host = app.state::<Mutex<MobileHost>>();
        let tunnel = app.state::<Tunnel<Wry>>();
        let guard = ready(&app, &host, &tunnel)?;
        guard.app_paths()?
    };
    let background = req.background;
    let startup = req.startup;
    run_blocking("check_app_update", move || {
        crate::app_update::check_release(&paths, background, startup)
    })
    .await?
}

#[tauri::command]
pub fn record_app_update_check(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
) -> Result<(), AppError> {
    let guard = ready(&app, &host, &tunnel)?;
    crate::app_update::record_check(&guard.app_paths()?)
}

#[tauri::command]
pub fn open_app_download(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
) -> Result<(), AppError> {
    let guard = ready(&app, &host, &tunnel)?;
    let url = crate::app_update::cached_apk_url(&guard.app_paths()?)?;
    drop(guard);
    tunnel.open_https_url(&url).map_err(map_tunnel)
}

#[tauri::command]
pub fn request_battery_exemption(tunnel: State<'_, Tunnel<Wry>>) -> Result<(), AppError> {
    tunnel.request_battery_exemption().map_err(map_tunnel)
}

#[tauri::command]
pub fn open_network_settings(tunnel: State<'_, Tunnel<Wry>>) -> Result<(), AppError> {
    tunnel.open_network_settings().map_err(map_tunnel)
}

#[tauri::command]
pub fn open_vpn_settings(tunnel: State<'_, Tunnel<Wry>>) -> Result<(), AppError> {
    tunnel.open_vpn_settings().map_err(map_tunnel)
}

#[tauri::command]
pub fn remove_custom_rule(
    app: AppHandle,
    host: State<'_, Mutex<MobileHost>>,
    tunnel: State<'_, Tunnel<Wry>>,
    req: FingerprintRequest,
) -> Result<RuleMutation, AppError> {
    let mut host = ready(&app, &host, &tunnel)?;
    host.remove_custom_rule(&req.fingerprint)?;
    let apply_warning = apply_config(&mut host, &tunnel).err();
    drop(host);
    notify(&app);
    Ok(RuleMutation {
        ok: true,
        disabled: None,
        fingerprint: None,
        apply_warning,
    })
}
