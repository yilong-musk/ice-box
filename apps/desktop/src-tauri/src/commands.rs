//! Tauri IPC commands (architecture §14).

use crate::capture::{tun_topology_changed, TrafficCapture, TunStatus};
use crate::core_watch::reconcile_unexpected_core_exit;
use crate::orchestrate::{
    current_settings, endpoints_from_settings, generate_config, orchestrate_apply,
    orchestrate_set_proxy_mode_with_apply, orchestrate_start, patch_selected_tag_default,
    resolve_binary,
};
use crate::shutdown::graceful_stop;
use crate::AppState;
use ice_config::NormalizedOutbound;
use ice_config::{
    load_group_selections, load_rule_overrides, redact_config_str, rule_fingerprint, rule_type_of,
    save_group_selections, save_rule_overrides, save_settings as persist_settings, AppError,
    AppSettings, CaptureIntent, ErrorCode, NormalizedProfile, ProxyMode, RuleOverrides,
};
use ice_core::{
    proxy_delay, proxy_groups, select_group, select_outbound, CoreState, CoreStatus,
    HealthEndpoints, TrafficSnapshot, DELAY_TEST_URL,
};
use ice_proxy_sys::{is_proxy_applied_on_disk, is_proxy_live_applied, ProxyEndpoints};
use ice_subscription::{
    active_subscription, list_profile_outbounds, load_active_profile_with_default_rules,
    load_index, redact_subscription_url_for_log, redact_subscription_url_for_ui,
    write_subscription_error, SubscriptionError, SubscriptionManager, SubscriptionPaths,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Instant, SystemTime};
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

fn resource_dir<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<std::path::PathBuf> {
    app.path().resource_dir().ok()
}

pub(crate) fn binary_for<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<std::path::PathBuf, AppError> {
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

fn attach_traffic(state: &AppState, settings: &AppSettings) {
    state.traffic.set_endpoints(Some(clash_endpoints(settings)));
}

fn detach_traffic(state: &AppState) {
    state.traffic.set_endpoints(None);
}

/// Join-error mapping for `spawn_blocking` (blocking work must not run on the
/// main thread — sync commands freeze the UI event loop).
fn blocking_join_err<E: std::fmt::Display>(context: &str) -> impl FnOnce(E) -> AppError {
    let context = context.to_string();
    move |e| AppError::new(ErrorCode::ConfigInvalid, format!("{context}: {e}"))
}

/// Run blocking IPC work on Tokio's blocking pool so the UI event loop stays live.
async fn run_blocking<T: Send + 'static>(
    context: &'static str,
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(blocking_join_err(context))?
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub core: CoreState,
    pub subscription_count: usize,
    pub proxy_recovery_warning: Option<String>,
    /// Live OS match when the platform backend is available and core is running.
    pub system_proxy_applied: Option<bool>,
    /// On-disk `applied` flag (enables「停止代理服务」even when the OS was changed externally).
    pub system_proxy_recorded: Option<bool>,
    /// False on platforms without a real system-proxy backend (e.g. Linux Noop).
    pub system_proxy_available: bool,
    // --- TUN capture status (plan §4.3) ---
    /// Derived only from the runtime capture controller.
    pub traffic_capture: TrafficCapture,
    /// Committed settings desire (`settings.tun.enabled`).
    pub configured_tun: bool,
    pub tun_status: TunStatus,
    pub tun_interface: Option<String>,
    pub tun_error: Option<AppError>,
    pub capture_transition_id: Option<String>,
    pub tun_available: bool,
    pub tun_unavailable_reason: Option<String>,
    /// True when the platform must not surface TUN controls at all; the
    /// frontend hides the TUN card and switches when set.
    pub tun_ui_hidden: bool,
    /// Privileged helper daemon installed + authorized (read-only probe).
    /// Drives the「安装/卸载辅助组件」actions in Settings and Home.
    pub helper_installed: bool,
    /// The installed helper's root-owned core differs from the app's bundled
    /// core (app updated): only one core version may exist, so TUN stays
    /// blocked until the helper is refreshed.
    pub helper_stale: bool,
}

/// How long a `system_proxy_applied` check result is reused. The check spawns
/// `networksetup` subprocesses (list + 4 gets per service); status is polled every 2s
/// by two components, so caching keeps the subprocess storm away while the result stays
/// fresh enough for the "proxy syncing…" indicator.
const PROXY_APPLIED_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(2);

fn proxy_applied_cache_fresh(
    state: &AppState,
    endpoints: &ProxyEndpoints,
    now: std::time::Instant,
) -> Option<bool> {
    let cache = state.proxy_applied_cache.lock().ok()?;
    let (cached_endpoints, at, value) = cache.as_ref()?;
    if cached_endpoints == endpoints && now.duration_since(*at) < PROXY_APPLIED_CACHE_TTL {
        Some(*value)
    } else {
        None
    }
}

/// Live check of `is_proxy_live_applied`, memoized per endpoints for `PROXY_APPLIED_CACHE_TTL`.
fn cached_system_proxy_applied(state: &AppState, settings: &AppSettings) -> Option<bool> {
    let endpoints = endpoints_from_settings(settings);
    let now = std::time::Instant::now();
    if let Some(value) = proxy_applied_cache_fresh(state, &endpoints, now) {
        return Some(value);
    }
    // Do not hold the cache lock across the live OS check, and do not wait on
    // `proxy` while start/stop is applying or restoring (subprocess / registry).
    let Ok(proxy) = state.proxy.try_lock() else {
        // Apply/restore in flight and the memo is stale: do not serve an expired
        // snapshot (Home would show the pre-toggle live state until TTL elapsed).
        return None;
    };
    // Another poller may have filled the cache between the miss and this lock.
    if let Some(value) = proxy_applied_cache_fresh(state, &endpoints, std::time::Instant::now()) {
        return Some(value);
    }
    let value = is_proxy_live_applied(proxy.as_ref(), &state.paths.proxy_backup(), &endpoints);
    drop(proxy);
    if let Ok(mut cache) = state.proxy_applied_cache.lock() {
        *cache = Some((endpoints, now, value));
    }
    Some(value)
}

/// mtime/len signature of the inputs that determine the loaded active profile:
/// the subscription index (active id), the active profile.json, and settings
/// (`auto_default_rules`). Writes are atomic renames, so a changed signature
/// reliably implies changed content; identical signature implies the parsed
/// profile is still valid.
#[derive(Clone, Copy, PartialEq, Eq)]
struct ProfileSig {
    index: Option<(SystemTime, u64)>,
    profile: Option<(SystemTime, u64)>,
    settings: Option<(SystemTime, u64)>,
}

/// Cached parse of the active profile plus the per-rule fingerprints (one
/// serialization per rule per profile version, instead of per poll/request).
#[derive(Clone)]
pub struct ProfileCacheEntry {
    sig: ProfileSig,
    pub profile: Arc<NormalizedProfile>,
    /// Parallel to `profile.route.rules`.
    pub fingerprints: Arc<Vec<String>>,
    /// Lazy lowercase-serialized rule text for keyword search: built once per
    /// profile version on the first keyword query (10k rules ≈ a few MB), then
    /// reused. Never allocated for non-keyword reads.
    keyword_text: Arc<Mutex<Option<Arc<Vec<String>>>>>,
}

impl ProfileCacheEntry {
    /// Lowercase-serialized text of every subscription rule, built lazily.
    fn keyword_texts(&self) -> Arc<Vec<String>> {
        let mut slot = self
            .keyword_text
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(texts) = slot.as_ref() {
            return texts.clone();
        }
        let texts: Arc<Vec<String>> = Arc::new(
            self.profile
                .route
                .rules
                .iter()
                .map(|rule| {
                    serde_json::to_string(rule)
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                })
                .collect(),
        );
        *slot = Some(texts.clone());
        texts
    }
}

/// Change-detected merged log view (`get_log_view` polls every 2s).
pub struct LogViewCache {
    /// `file_sig` per source (app, core, helper), in read order; `None` for a
    /// missing/unreadable source or when the helper log is not in play.
    sigs: Vec<Option<(SystemTime, u64)>>,
    n: usize,
    lines: Vec<String>,
}

fn file_sig(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Load the active profile from a mtime-keyed cache. `Ok(None)` when no active
/// subscription exists. Returns the built-in-default-rules-applied profile
/// (same semantics as `load_active_profile_with_default_rules`).
fn cached_profile(state: &AppState) -> Result<Option<ProfileCacheEntry>, AppError> {
    let sub_paths = SubscriptionPaths::from_app(&state.paths);
    let index = load_index(&sub_paths).map_err(AppError::from)?;
    let active = active_subscription(&index);
    let sig = ProfileSig {
        index: file_sig(&sub_paths.index()),
        profile: active.and_then(|m| file_sig(&sub_paths.profile(m.id))),
        settings: file_sig(&state.paths.settings()),
    };
    if let Ok(cache) = state.profile_cache.lock() {
        if let Some(entry) = cache.as_ref() {
            if entry.sig == sig {
                return Ok(Some(entry.clone()));
            }
        }
    }
    let auto_default_rules = current_settings(&state.paths)
        .map(|s| s.auto_default_rules)
        .unwrap_or(true);
    let profile =
        match load_active_profile_with_default_rules(&sub_paths, &index, auto_default_rules) {
            Ok(profile) => profile,
            Err(SubscriptionError::NoActiveSubscription) => return Ok(None),
            Err(err) => return Err(AppError::from(err)),
        };
    let entry = ProfileCacheEntry {
        sig,
        fingerprints: Arc::new(profile.route.rules.iter().map(rule_fingerprint).collect()),
        profile: Arc::new(profile),
        keyword_text: Arc::new(Mutex::new(None)),
    };
    if let Ok(mut cache) = state.profile_cache.lock() {
        *cache = Some(entry.clone());
    }
    Ok(Some(entry))
}

fn active_profile(state: &AppState) -> Result<NormalizedProfile, AppError> {
    cached_profile(state)?
        .map(|entry| (*entry.profile).clone())
        .ok_or_else(|| AppError::new(ErrorCode::ConfigEmptyOutbounds, "no active subscription"))
}

fn merged_outbounds(state: &AppState) -> Result<Vec<NormalizedOutbound>, AppError> {
    Ok(list_profile_outbounds(&active_profile(state)?))
}

/// Like `merged_outbounds`, but `Ok(None)` when no active subscription exists
/// (first-run / all subscriptions removed). Read paths use this so the UI gets
/// an empty list instead of an error; mutation paths keep erroring via
/// `merged_outbounds`.
fn merged_outbounds_opt(state: &AppState) -> Result<Option<Vec<NormalizedOutbound>>, AppError> {
    Ok(cached_profile(state)?.map(|entry| list_profile_outbounds(&entry.profile)))
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

/// How long a helper-daemon reachability probe result is reused. The probe is a
/// socket roundtrip (bounded 200ms); status is polled every 2s by two
/// components, so caching keeps dead-daemon probes from stacking up. Explicitly
/// invalidated after install/uninstall so the Settings wait loop sees the
/// fresh state immediately.
const HELPER_PROBE_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(2);

fn cached_helper_installed(state: &AppState) -> bool {
    let now = Instant::now();
    if let Ok(cache) = state.helper_probe_cache.lock() {
        if let Some((at, value)) = *cache {
            if now.duration_since(at) < HELPER_PROBE_CACHE_TTL {
                return value;
            }
        }
    }
    let value = crate::helper_install::helper_installed(state);
    if let Ok(mut cache) = state.helper_probe_cache.lock() {
        *cache = Some((now, value));
    }
    value
}

fn reset_helper_probe_cache(state: &AppState) {
    if let Ok(mut cache) = state.helper_probe_cache.lock() {
        *cache = None;
    }
}

fn collect_status(state: &AppState) -> Result<StatusResponse, AppError> {
    reconcile_unexpected_core_exit(state);
    // Snapshot core state and drop the lock before disk / `networksetup` work so
    // start/stop are not stuck behind a status poll.
    let (core_state, running) = {
        let core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
        let core_state = core.state();
        let running = core_state.status == CoreStatus::Running;
        (core_state, running)
    };
    let paths = SubscriptionPaths::from_app(&state.paths);
    let count = ice_subscription::load_index(&paths)
        .map(|i| i.items.len())
        .unwrap_or(0);
    let proxy_recovery_warning = state
        .proxy_recovery_warning
        .lock()
        .ok()
        .and_then(|g| g.clone());
    let proxy_available = state.system_proxy_available;
    let system_proxy_recorded = if running {
        Some(is_proxy_applied_on_disk(&state.paths.proxy_backup()))
    } else {
        None
    };
    let system_proxy_applied = if running && proxy_available {
        current_settings(&state.paths)
            .ok()
            .and_then(|settings| cached_system_proxy_applied(state, &settings))
    } else {
        None
    };
    let capture = current_settings(&state.paths)
        .ok()
        .map(|settings| state.capture.status(&settings))
        .unwrap_or_else(|| state.capture.status(&ice_config::AppSettings::default()));
    Ok(StatusResponse {
        core: core_state,
        subscription_count: count,
        proxy_recovery_warning,
        system_proxy_applied,
        system_proxy_recorded,
        system_proxy_available: proxy_available,
        traffic_capture: capture.traffic_capture,
        configured_tun: capture.configured_tun,
        tun_status: capture.tun_status,
        tun_interface: capture.tun_interface,
        tun_error: capture.tun_error,
        capture_transition_id: capture.capture_transition_id,
        tun_available: capture.tun_available,
        tun_unavailable_reason: capture.tun_unavailable_reason,
        tun_ui_hidden: capture.tun_ui_hidden,
        helper_installed: cached_helper_installed(state),
        helper_stale: crate::helper_install::helper_core_stale(state.capture.resource_dir()),
    })
}

#[tauri::command]
pub async fn get_status(app: AppHandle) -> Result<StatusResponse, AppError> {
    run_blocking("get_status", move || {
        let state = app.state::<AppState>();
        collect_status(&state)
    })
    .await
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

/// Start the core only (no system proxy). Used on app launch.
pub fn start_core(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
    let _orch = lock_orchestrate(state)?;
    let settings = current_settings(&state.paths)?;
    {
        let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
        if core.state().status != CoreStatus::Running {
            // Do not hold `proxy` across spawn + healthcheck; start never applies OS proxy.
            let binary = binary_for(app)?;
            let _ = orchestrate_start(
                &state.paths,
                &settings,
                &mut **core,
                binary,
                resource_dir(app).as_deref(),
                CaptureIntent::Diagnostic,
            )?;
            if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
                *slot = None;
            }
        }
    }
    attach_traffic(state, &settings);
    Ok(())
}

/// Home「启动代理服务」: ensure core is running on the Diagnostic config, then
/// start the configured capture backend (system proxy or TUN, plan §2).
fn start_service(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
    let _orch = lock_orchestrate(state)?;
    let settings = current_settings(&state.paths)?;
    {
        let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
        if core.state().status != CoreStatus::Running {
            let binary = binary_for(app)?;
            let _ = orchestrate_start(
                &state.paths,
                &settings,
                &mut **core,
                binary,
                resource_dir(app).as_deref(),
                CaptureIntent::Diagnostic,
            )?;
            if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
                *slot = None;
            }
        }
    }
    if settings.tun.enabled {
        // TUN path: the controller stops the app-managed core and the
        // backend starts the elevated one (adopted afterwards).
        state.capture.refresh_backend()?;
        let binary = binary_for(app)?;
        let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
        let resolved = state
            .capture
            .enable_tun(&settings, &mut **core, binary.clone())?;
        // Persist the resolved interface name only after the transition is
        // healthy (plan §4.3 commit-after-health).
        if resolved
            .as_ref()
            .is_some_and(|name| settings.tun.interface_name.as_deref() != Some(name.as_str()))
        {
            let mut committed = settings.clone();
            committed.tun.interface_name = resolved;
            if let Err(commit_err) = persist_settings(&state.paths.settings(), &committed) {
                let rollback = {
                    let proxy = state.proxy.lock().map_err(|_| lock_poisoned("proxy"))?;
                    state.capture.disable_active_backend(
                        &settings,
                        &mut **core,
                        proxy.as_ref(),
                        binary,
                        true,
                    )
                };
                return match rollback {
                    Ok(()) => Err(commit_err),
                    Err(rollback_err) => Err(AppError::with_code(
                        "tun.recovery_required",
                        format!(
                            "TUN interface commit failed ({commit_err}) and rollback was not verified ({rollback_err})"
                        ),
                    )),
                };
            }
        }
        if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
            *slot = None;
        }
    } else {
        // System-proxy branch: exclusivity is enforced by the capture
        // controller (TUN active / preparing / stopping / recovery_required
        // all reject the enable), never inferred from `tun.enabled` alone.
        {
            let core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
            let proxy = state.proxy.lock().map_err(|_| lock_poisoned("proxy"))?;
            state
                .capture
                .enable_system_proxy(&settings, &**core, proxy.as_ref())?;
        }
        if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
            *slot = None;
        }
        if let Ok(mut cache) = state.proxy_applied_cache.lock() {
            *cache = None;
        }
    }
    attach_traffic(state, &settings);
    Ok(())
}

/// Home「启动代理服务」: ensure core is running, then take over the OS system proxy.
#[tauri::command]
pub async fn start(app: AppHandle) -> Result<(), AppError> {
    run_blocking("start", move || {
        let state = app.state::<AppState>();
        start_service(&app, &state)
    })
    .await
}

/// Home「停止代理服务」: disable whichever capture backend is active. The IPC
/// name is retained for compatibility (plan §4.3); it delegates to the
/// controller, so it restores the OS proxy for the system-proxy backend and
/// releases TUN capture (core may stay Running on the Diagnostic config).
fn disable_active_backend_inner(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
    let _orch = lock_orchestrate(state)?;
    let settings = current_settings(&state.paths)?;
    let binary = binary_for(app)?;
    let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
    let proxy = state.proxy.lock().map_err(|_| lock_poisoned("proxy"))?;
    state
        .capture
        .disable_active_backend(&settings, &mut **core, proxy.as_ref(), binary, true)?;
    if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
        *slot = None;
    }
    if let Ok(mut cache) = state.proxy_applied_cache.lock() {
        *cache = None;
    }
    Ok(())
}

/// Home「重试恢复」: on-demand retry of TUN recovery (plan §4.3 / §4.4). Runs
/// the journal recovery driver under the orchestration lock; never enables
/// capture. Returns a warning message when cleanup is still uncertain
/// (`recovery_required` persists, fail-closed).
#[tauri::command]
pub async fn recover_tun(app: AppHandle) -> Result<Option<String>, AppError> {
    run_blocking("recover_tun", move || {
        let state = app.state::<AppState>();
        let _orch = lock_orchestrate(&state)?;
        state.capture.refresh_backend()?;
        let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
        let warning = state.capture.recover(&mut **core)?;
        if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
            *slot = warning.clone();
        }
        Ok(warning)
    })
    .await
}

#[tauri::command]
pub async fn stop_system_proxy(app: AppHandle) -> Result<(), AppError> {
    run_blocking("stop_system_proxy", move || {
        let state = app.state::<AppState>();
        disable_active_backend_inner(&app, &state)
    })
    .await
}

/// Install and authorize the trusted helper component (TUN elevation path):
/// prompts with the system authorization dialog and runs the bundled
/// `ice-helper install` as root. macOS only; cancelling modifies nothing.
/// On success the backend is refreshed so the fresh helper is usable at once.
#[tauri::command]
pub async fn install_helper(app: AppHandle) -> Result<(), AppError> {
    run_blocking("install_helper", move || {
        let result = crate::helper_install::install_helper_inner(&app);
        let state = app.state::<AppState>();
        reset_helper_probe_cache(&state);
        result
    })
    .await
}

/// Uninstall the trusted helper component: prompts with the system
/// authorization dialog, runs `ice-helper uninstall` as root, and refreshes
/// the backend (back to fail-closed).
#[tauri::command]
pub async fn uninstall_helper(app: AppHandle) -> Result<(), AppError> {
    run_blocking("uninstall_helper", move || {
        let result = crate::helper_install::uninstall_helper_inner(&app);
        let state = app.state::<AppState>();
        reset_helper_probe_cache(&state);
        result
    })
    .await
}

/// Full stop used by tray Quit / app exit (disable TUN capture first, restore
/// system proxy, then kill core).
#[tauri::command]
pub async fn stop(app: AppHandle) -> Result<(), AppError> {
    run_blocking("stop", move || {
        let state = app.state::<AppState>();
        let binary = binary_for(&app)?;
        graceful_stop(&state, binary)
    })
    .await
}

#[derive(Deserialize)]
pub struct LogViewRequest {
    pub n: usize,
}

#[tauri::command]
pub async fn get_log_view(app: AppHandle, req: LogViewRequest) -> Result<Vec<String>, AppError> {
    run_blocking("get_log_view", move || {
        let state = app.state::<AppState>();
        // While TUN capture runs through the privileged helper (production
        // macOS path), the elevated core's output goes to the helper's fixed
        // log instead of the app-data core log; the dev sudo runner keeps the
        // app-data path, so it must not read the helper log. The capture
        // controller latches helper usage for the app session, so the merge
        // also persists after a TUN session ends.
        let helper_log = (state.capture.helper_core_used()
            && !ice_tun_sys::dev_sudo_runner_enabled())
        .then(|| std::path::Path::new(ice_tun_sys::install_paths::CORE_LOG_DEST));
        // Change detection: the view is polled every 2s; skip the read + parse
        // when no source file changed and the requested depth is unchanged.
        let sigs = vec![
            file_sig(&state.paths.app_log()),
            file_sig(&state.paths.core_log()),
            helper_log.and_then(file_sig),
        ];
        if let Ok(cache) = state.log_view_cache.lock() {
            if let Some(entry) = cache.as_ref() {
                if entry.n == req.n && entry.sigs == sigs {
                    return Ok(entry.lines.clone());
                }
            }
        }
        let lines = crate::log_view::read_log_view(
            &state.paths.app_log(),
            &state.paths.core_log(),
            helper_log,
            req.n,
        )?;
        if let Ok(mut cache) = state.log_view_cache.lock() {
            *cache = Some(LogViewCache {
                sigs,
                n: req.n,
                lines: lines.clone(),
            });
        }
        Ok(lines)
    })
    .await
}

#[tauri::command]
pub async fn get_runtime_config(app: AppHandle) -> Result<String, AppError> {
    run_blocking("get_runtime_config", move || {
        let state = app.state::<AppState>();
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
    })
    .await
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
pub async fn get_settings(app: AppHandle) -> Result<AppSettings, AppError> {
    run_blocking("get_settings", move || {
        let state = app.state::<AppState>();
        current_settings(&state.paths)
    })
    .await
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), AppError> {
    // A TUN transition can take seconds (system-proxy restore via
    // `networksetup` + elevated core restart + readiness waits); a sync
    // command would block the main thread and freeze the UI event loop.
    run_blocking("save_settings", move || {
        let state = app.state::<AppState>();
        let _orch = lock_orchestrate(&state)?;
        let previous = current_settings(&state.paths).unwrap_or_default();
        settings.validate()?;
        let active = state.capture.active_backend();
        let tun_transition = active != TrafficCapture::Inactive
            && (previous.tun.enabled != settings.tun.enabled
                || (previous.tun.enabled && tun_topology_changed(&previous.tun, &settings.tun)));
        if tun_transition {
            // Serialized backend transition (plan §4.3): the pending record is
            // committed only after the requested backend is healthy.
            let binary = binary_for(&app)?;
            let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
            let proxy = state.proxy.lock().map_err(|_| lock_poisoned("proxy"))?;
            state.capture.transition_tun_settings(
                &previous,
                &settings,
                &mut **core,
                proxy.as_ref(),
                binary,
            )?;
            if state.capture.active_backend() == TrafficCapture::Tun {
                // The transition re-enabled TUN from the full candidate, so the
                // runtime config already reflects the change (incl. ports/mode);
                // a second apply would tear down and re-create the TUN for
                // nothing. Re-target the traffic stream (endpoints may have
                // changed) — the mutation paths are the only re-attach points
                // since the 1s snapshot poll no longer reads settings.
                attach_traffic(&state, &settings);
                Ok(())
            } else {
                // TUN was disabled: the disable path restarted the app-managed
                // core on the previous config; apply the non-backend parts of
                // the change (ports, mode, rules) now.
                apply_after_change(&app, &state, &settings, &previous)
            }
        } else {
            persist_settings(&state.paths.settings(), &settings)?;
            apply_after_change(&app, &state, &settings, &previous)
        }
    })
    .await
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

fn set_proxy_mode_inner(
    app: &AppHandle,
    state: &AppState,
    req: SetProxyModeRequest,
) -> Result<(), AppError> {
    let _orch = lock_orchestrate(state)?;
    let mode = parse_proxy_mode(&req.mode)?;
    let previous = current_settings(&state.paths)?;
    if previous.proxy_mode == mode {
        return Ok(());
    }
    let mut settings = previous.clone();
    settings.proxy_mode = mode;
    persist_settings(&state.paths.settings(), &settings)?;
    let binary = binary_for(app)?;
    let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
    let proxy = state.proxy.lock().map_err(|_| lock_poisoned("proxy"))?;
    let tun_active = state.capture.active_backend() == TrafficCapture::Tun;
    let mut live_mode_ok = *state
        .clash_live_mode_cache
        .lock()
        .map_err(|_| lock_poisoned("clash_live_mode_cache"))?;
    let result = orchestrate_set_proxy_mode_with_apply(
        &state.paths,
        &settings,
        &previous,
        &mut **core,
        proxy.as_ref(),
        binary,
        resource_dir(app).as_deref(),
        state.capture.apply_intent(),
        &mut live_mode_ok,
        |paths, settings, previous, core, proxy, binary, resource_dir, intent| {
            if tun_active {
                // The rebuild + reload fallback cannot signal the elevated
                // core; route it through the capture controller so TUN health
                // is re-verified and failures fall back to disable/re-apply.
                state
                    .capture
                    .apply_while_tun_active(settings, previous, core, proxy, binary)
            } else {
                orchestrate_apply(
                    paths,
                    settings,
                    previous,
                    core,
                    proxy,
                    binary,
                    resource_dir,
                    intent,
                )
            }
        },
    );
    // Remember the probe outcome (one PATCH attempt per process; a core that
    // honors it keeps the fast path, one that ignores it skips future probes).
    if let Ok(mut cache) = state.clash_live_mode_cache.lock() {
        *cache = live_mode_ok;
    }
    result
}

/// Switch routing mode. With the pinned sing-box 1.13.19 the runtime Clash `mode-list` is
/// only `[<default_mode>]`, so a `PATCH /configs` to another mode is silently ignored and
/// the switch always takes the rebuild + reload/restart path (the PATCH attempt is a
/// forward-compatible capability gate). Settings are always persisted so the next apply
/// builds the new `default_mode`.
#[tauri::command]
pub async fn set_proxy_mode(app: AppHandle, req: SetProxyModeRequest) -> Result<(), AppError> {
    run_blocking("set_proxy_mode", move || {
        let state = app.state::<AppState>();
        set_proxy_mode_inner(&app, &state, req)
    })
    .await
}

/// Apply after a settings / subscription mutation. `generate_config` falls back
/// to a direct-only config when no subscription exists, so this always writes a
/// valid config.json (and reloads while Running). The capture intent comes
/// from the runtime controller, never from `tun.enabled` alone.
///
/// While TUN capture is active the apply is routed through the capture
/// controller: the elevated core cannot be signalled by the app, so the
/// controller runs the stop/reconfigure/start sequence and re-verifies TUN
/// health instead of a plain `core.reload()`.
fn apply_after_change(
    app: &AppHandle,
    state: &AppState,
    settings: &AppSettings,
    previous_settings: &AppSettings,
) -> Result<(), AppError> {
    let binary = binary_for(app)?;
    let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
    let proxy = state.proxy.lock().map_err(|_| lock_poisoned("proxy"))?;

    if state.capture.active_backend() == TrafficCapture::Tun {
        state.capture.apply_while_tun_active(
            settings,
            previous_settings,
            &mut **core,
            proxy.as_ref(),
            binary,
        )?;
    } else {
        orchestrate_apply(
            &state.paths,
            settings,
            previous_settings,
            &mut **core,
            proxy.as_ref(),
            binary,
            resource_dir(app).as_deref(),
            state.capture.apply_intent(),
        )?;
    }
    let running = core.state().status == CoreStatus::Running;
    drop(proxy);
    drop(core);
    if running {
        attach_traffic(state, settings);
    } else {
        detach_traffic(state);
    }
    Ok(())
}

fn apply_after_subscription_change(
    app: &AppHandle,
    state: &AppState,
    settings: &AppSettings,
) -> Option<AppError> {
    match apply_after_change(app, state, settings, settings) {
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
pub async fn add_subscription(
    app: AppHandle,
    req: AddSubscriptionRequest,
) -> Result<serde_json::Value, AppError> {
    run_blocking("add_subscription", move || {
        let state = app.state::<AppState>();
        let redacted = redact_subscription_url_for_log(&req.url);
        tracing::info!(url = %redacted, name = ?req.name, "add_subscription: start");
        // Fetch (up to FETCH_TIMEOUT) runs without the orchestrate lock so Start/Stop/Apply/
        // save_settings are not queued behind it; the lock is taken for the disk write + Apply.
        let paths = SubscriptionPaths::from_app(&state.paths);
        let mgr = SubscriptionManager::open(paths);
        let fetched = mgr.fetch_add(&req.url, req.name.as_deref()).map_err(|e| {
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
        let mgr = SubscriptionManager::open(paths);
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
        let mgr = SubscriptionManager::open(paths);
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

#[tauri::command]
pub async fn apply_subscriptions(app: AppHandle) -> Result<(), AppError> {
    run_blocking("apply_subscriptions", move || {
        let state = app.state::<AppState>();
        let _orch = lock_orchestrate(&state)?;
        let settings = current_settings(&state.paths)?;
        apply_after_change(&app, &state, &settings, &settings)
    })
    .await
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
pub async fn list_nodes(app: AppHandle) -> Result<Vec<NodeInfo>, AppError> {
    run_blocking("list_nodes", move || {
        let state = app.state::<AppState>();
        let Some(outbounds) = merged_outbounds_opt(&state)? else {
            return Ok(vec![]);
        };
        let settings = current_settings(&state.paths)?;
        let selections = load_group_selections(&state.paths.group_selections());
        let core_running = {
            let core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
            core.state().status == CoreStatus::Running
        };
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
pub async fn get_rule_overview(app: AppHandle) -> Result<RuleOverview, AppError> {
    run_blocking("get_rule_overview", move || {
        let state = app.state::<AppState>();
        rule_overview(state.inner())
    })
    .await
}

/// Query rules with server-side filtering + pagination. Never ships the full rule list
/// over IPC: big subscriptions (up to 10k rules) stay cheap for the UI.
fn query_rules(state: &AppState, req: &ListRulesRequest) -> Result<ListRulesResponse, AppError> {
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
        if req.custom == Some(true) {
            continue;
        }
        let fp: std::borrow::Cow<'_, str> = fingerprints
            .get(idx)
            .map(|fp| std::borrow::Cow::Borrowed(fp.as_str()))
            .unwrap_or_else(|| std::borrow::Cow::Owned(rule_fingerprint(rule)));
        let disabled = overrides.is_disabled(&fp);
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
fn keyword_matches(
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

        persist_settings(&state.paths.settings(), &settings)?;
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
                Ok(false) => generate_config(
                    &state.paths,
                    &settings,
                    resource_dir(&app).as_deref(),
                    state.capture.apply_intent(),
                )
                .map(|_| ()),
                Err(err) => Err(err),
            },
            None => generate_config(
                &state.paths,
                &settings,
                resource_dir(&app).as_deref(),
                state.capture.apply_intent(),
            )
            .map(|_| ()),
        };
        if let Err(err) = persist_result {
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
                let _ = generate_config(
                    &state.paths,
                    &previous,
                    resource_dir(&app).as_deref(),
                    state.capture.apply_intent(),
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
            generate_config(
                &state.paths,
                &settings,
                resource_dir(&app).as_deref(),
                state.capture.apply_intent(),
            )?;
        }
        Ok(())
    })
    .await
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

#[tauri::command]
pub async fn get_traffic_snapshot(app: AppHandle) -> Result<TrafficSnapshot, AppError> {
    run_blocking("get_traffic_snapshot", move || {
        let state = app.state::<AppState>();
        require_running_core(&state)?;
        // Endpoints are re-attached on every mutation path (start/stop/apply/
        // settings/TUN transition), so no per-second settings read is needed.
        Ok(state.traffic.snapshot())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::CaptureController;
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
            system_proxy_available: false,
            shutdown_requested: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            _instance_lock: crate::test_instance_lock(&paths),
            traffic: ice_core::TrafficMonitor::new(),
            capture: CaptureController::new(paths.clone(), None),
            profile_cache: Mutex::new(None),
            log_view_cache: Mutex::new(None),
            helper_probe_cache: Mutex::new(None),
            clash_live_mode_cache: Mutex::new(true),
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
            system_proxy_available: false,
            shutdown_requested: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            _instance_lock: crate::test_instance_lock(&paths),
            traffic: ice_core::TrafficMonitor::new(),
            capture: CaptureController::new(paths.clone(), None),
            profile_cache: Mutex::new(None),
            log_view_cache: Mutex::new(None),
            helper_probe_cache: Mutex::new(None),
            clash_live_mode_cache: Mutex::new(true),
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
    fn collect_status_snapshots_stopped_core() {
        let state = temp_state_with_node("status");
        let status = collect_status(&state).expect("status");
        assert_eq!(status.core.status, ice_core::CoreStatus::Stopped);
        assert_eq!(status.subscription_count, 1);
        assert_eq!(status.system_proxy_recorded, None);
        assert_eq!(status.system_proxy_applied, None);
        assert!(!status.system_proxy_available);
        let _ = fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn collect_status_does_not_block_on_held_proxy_lock() {
        let state = temp_state_with_node("proxy-held");
        // Warm the helper-core drift caches (one-time SHA-256 of the bundled
        // core) outside the measured window: the poll itself must stay cheap.
        let _ = crate::helper_install::helper_core_stale(state.capture.resource_dir());
        let _guard = state.proxy.lock().unwrap();
        let started = std::time::Instant::now();
        let status = collect_status(&state).expect("status");
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "status poll must not wait on system-proxy apply/restore"
        );
        assert_eq!(status.core.status, ice_core::CoreStatus::Stopped);
        assert!(
            !status.system_proxy_available,
            "Noop backend must not flip to available while proxy lock is held"
        );
        let _ = fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn cached_system_proxy_applied_ignores_expired_memo_when_proxy_busy() {
        let state = temp_state_with_node("stale-cache");
        let settings = ice_config::AppSettings::default();
        let endpoints = endpoints_from_settings(&settings);
        {
            let mut cache = state.proxy_applied_cache.lock().unwrap();
            *cache = Some((
                endpoints.clone(),
                std::time::Instant::now()
                    .checked_sub(std::time::Duration::from_secs(10))
                    .expect("monotonic clock"),
                true,
            ));
        }
        let _guard = state.proxy.lock().unwrap();
        assert_eq!(
            cached_system_proxy_applied(&state, &settings),
            None,
            "expired memo must not be served while apply/restore holds the proxy lock"
        );
        let _ = fs::remove_dir_all(state.paths.root());
    }

    #[test]
    fn cached_system_proxy_applied_serves_fresh_memo_without_proxy_lock() {
        let state = temp_state_with_node("fresh-cache");
        let settings = ice_config::AppSettings::default();
        let endpoints = endpoints_from_settings(&settings);
        {
            let mut cache = state.proxy_applied_cache.lock().unwrap();
            *cache = Some((endpoints, std::time::Instant::now(), true));
        }
        let _guard = state.proxy.lock().unwrap();
        assert_eq!(cached_system_proxy_applied(&state, &settings), Some(true));
        let _ = fs::remove_dir_all(state.paths.root());
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
    fn profile_cache_serves_unchanged_and_invalidates_on_update() {
        let state = temp_state_with_node("cache");
        let first = merged_outbounds_opt(&state).unwrap().unwrap();
        assert_eq!(first[0].tag, "n1");
        // mtime unchanged: the second read must come from the cache.
        let second = merged_outbounds_opt(&state).unwrap().unwrap();
        assert_eq!(second[0].tag, "n1");
        assert!(
            state.profile_cache.lock().unwrap().is_some(),
            "cache must be populated after a read"
        );

        // Simulate a subscription update: profile.json + index.json are
        // rewritten atomically, which must invalidate the cached entry.
        let sub = SubscriptionPaths::from_app(&state.paths);
        let index = load_index(&sub).unwrap();
        let meta = ice_subscription::active_subscription(&index)
            .unwrap()
            .clone();
        let nodes = vec![NormalizedOutbound {
            tag: "n2".into(),
            outbound: serde_json::json!({"type":"socks","tag":"n2","server":"2.2.2.2","server_port":1}),
        }];
        write_subscription_success(
            &sub,
            &meta,
            "{}",
            &ice_config::NormalizedProfile::from_nodes_only(nodes),
        )
        .unwrap();
        let updated = merged_outbounds_opt(&state).unwrap().unwrap();
        assert_eq!(updated[0].tag, "n2", "stale cache must not be served");
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
                custom: None,
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
                custom: None,
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
                custom: None,
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

        generate_config(
            &state.paths,
            &AppSettings::default(),
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(state.paths.config()).unwrap()).unwrap();
        let rules = config["route"]["rules"].as_array().unwrap();
        assert_eq!(
            rules.len(),
            5,
            "2 clash_mode rules + 3 remaining sample rules after the disabled one dropped"
        );
        assert!(!serde_json::to_string(rules)
            .unwrap()
            .contains("youtube.com"));

        let listed = query_rules(
            &state,
            &ListRulesRequest {
                keyword: None,
                rule_type: None,
                disabled: Some("disabled".into()),
                custom: None,
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
                custom: None,
                offset: 0,
                limit: 50,
            },
        )
        .unwrap();
        assert_eq!(listed.total, 1);
        assert!(listed.items[0].custom);
        assert_eq!(listed.items[0].index, None);

        let custom_only = query_rules(
            &state,
            &ListRulesRequest {
                keyword: None,
                rule_type: None,
                disabled: None,
                custom: Some(true),
                offset: 0,
                limit: 50,
            },
        )
        .unwrap();
        assert_eq!(custom_only.total, 1);
        assert!(custom_only.items[0].custom);

        let subscription_only = query_rules(
            &state,
            &ListRulesRequest {
                keyword: None,
                rule_type: None,
                disabled: None,
                custom: Some(false),
                offset: 0,
                limit: 50,
            },
        )
        .unwrap();
        assert_eq!(subscription_only.total, 4);
        assert!(subscription_only.items.iter().all(|r| !r.custom));

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
        fs::write(sub.profile(id), serde_json::to_vec(&profile).unwrap()).unwrap();

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

        generate_config(
            &state.paths,
            &AppSettings::default(),
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(state.paths.config()).unwrap()).unwrap();
        let rules = config["route"]["rules"].as_array().unwrap();
        assert!(!serde_json::to_string(rules)
            .unwrap()
            .contains("blockme.com"));
        let _ = fs::remove_dir_all(state.paths.root());
    }
}
