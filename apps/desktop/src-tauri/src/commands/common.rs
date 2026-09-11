// SPDX-License-Identifier: GPL-3.0-or-later

pub(crate) use crate::capture::{
    only_tun_enabled_changed, tun_topology_changed, TrafficCapture, TunStatus,
};
pub(crate) use crate::orchestrate::{
    current_settings, endpoints_from_settings, generate_config_with_cache,
    orchestrate_apply_with_cache, orchestrate_set_proxy_mode_with_apply,
    orchestrate_start_with_cache, orphan_reclaim_cores, patch_selected_tag_default, resolve_binary,
};
pub(crate) use crate::shutdown::graceful_stop;
pub(crate) use crate::tray::{self, TrayLanguage};
pub(crate) use crate::AppState;
pub(crate) use ice_config::NormalizedOutbound;
pub(crate) use ice_config::{
    load_group_selections, load_rule_overrides, redact_config_str, rule_fingerprint,
    rule_matches_fingerprint, rule_type_of, save_group_selections, save_rule_overrides,
    save_settings_for as persist_settings, set_proxy_service_enabled_for, AppError, AppPaths,
    AppSettings, CaptureIntent, ErrorCode, NormalizedProfile, ProxyMode, RuleOverrides,
    SettingsPatch, UiMessage,
};
pub(crate) use ice_core::{
    proxy_delay, proxy_groups, select_group, select_outbound, CoreState, CoreStatus,
    HealthEndpoints, TrafficDelta, TrafficSnapshot, DELAY_TEST_URL,
};
pub(crate) use ice_engine::{
    active_subscription, host_platform, list_profile_outbounds, load_index,
    redact_subscription_url_for_log, redact_subscription_url_for_ui, write_subscription_error,
    SubscriptionError, SubscriptionManager, SubscriptionPaths,
};
pub(crate) use ice_proxy_sys::{
    disk_proxy_state, is_proxy_live_applied, proxy_backup_indicates_ownership,
    recover_if_applied_hinted, DiskProxyState, ProxyEndpoints, RecoverOutcome,
};
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use std::path::Path;
#[cfg(target_os = "windows")]
pub(crate) use std::path::PathBuf;
pub(crate) use std::sync::atomic::Ordering;
pub(crate) use std::sync::mpsc::SyncSender;
pub(crate) use std::sync::{Arc, Mutex, MutexGuard};
pub(crate) use std::time::{Instant, SystemTime};
pub(crate) use tauri::{AppHandle, Manager, State};
pub(crate) use uuid::Uuid;

pub(crate) use crate::lock_poisoned;

pub(crate) fn lock_orchestrate(state: &AppState) -> Result<MutexGuard<'_, ()>, AppError> {
    state
        .orchestrate
        .lock()
        .map_err(|_| lock_poisoned("orchestrate"))
}

fn is_sticky_recovery(msg: &UiMessage) -> bool {
    msg.key == ErrorCode::SettingsReset.message_key()
        || msg.key == ErrorCode::LogsOversized.message_key()
}

pub(crate) fn clear_transient_recovery_warnings(state: &AppState) {
    if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
        slot.retain(is_sticky_recovery);
    }
}

pub(crate) fn append_recovery_warning(state: &AppState, warning: UiMessage) {
    if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
        slot.push(warning);
    }
}

pub(crate) fn replace_recovery_warnings(state: &AppState, warnings: Vec<UiMessage>) {
    if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
        *slot = warnings;
    }
}

pub(crate) fn resource_dir<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<std::path::PathBuf> {
    app.path().resource_dir().ok()
}

pub(crate) fn binary_for<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<std::path::PathBuf, AppError> {
    resolve_binary(resource_dir(app).as_deref())
}

pub(crate) fn require_running_core(state: &AppState) -> Result<(), AppError> {
    if state.core_snapshot.load().state.status != CoreStatus::Running {
        return Err(AppError::new(
            ErrorCode::CoreInvalidState,
            "operation requires running core",
        ));
    }
    Ok(())
}

pub(crate) fn clash_endpoints(
    paths: &AppPaths,
    settings: &AppSettings,
) -> Result<HealthEndpoints, AppError> {
    let secret = ice_config::ensure_clash_api_secret(&paths.clash_api_secret())?;
    Ok(
        HealthEndpoints::new(settings.clash_api_listen.clone(), settings.clash_api_port)
            .with_secret(secret),
    )
}

pub(crate) fn attach_traffic(state: &AppState, settings: &AppSettings) -> Result<(), AppError> {
    state
        .traffic
        .set_endpoints(Some(clash_endpoints(&state.paths, settings)?));
    Ok(())
}

pub(crate) fn detach_traffic(state: &AppState) {
    state.traffic.set_endpoints(None);
}

/// Join-error mapping for `spawn_blocking` (blocking work must not run on the
/// main thread — sync commands freeze the UI event loop).
pub(crate) fn blocking_join_err<E: std::fmt::Display>(context: &str) -> impl FnOnce(E) -> AppError {
    let context = context.to_string();
    move |e| AppError::new(ErrorCode::ConfigInvalid, format!("{context}: {e}"))
}

/// Run blocking IPC work on Tokio's blocking pool so the UI event loop stays live.
pub(crate) async fn run_blocking<T: Send + 'static>(
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
    pub proxy_recovery_warning: Vec<UiMessage>,
    /// Live OS match when the platform backend is available and core is running.
    pub system_proxy_applied: Option<bool>,
    /// On-disk `applied` flag (enables「停止代理服务」even when the OS was changed externally).
    pub system_proxy_recorded: Option<bool>,
    /// False on platforms without a real system-proxy backend (e.g. Linux Noop).
    pub system_proxy_available: bool,
    // --- TUN capture status ---
    /// Derived only from the runtime capture controller.
    pub traffic_capture: TrafficCapture,
    /// Committed settings desire (`settings.tun.enabled`).
    pub configured_tun: bool,
    pub tun_status: TunStatus,
    pub tun_interface: Option<String>,
    pub tun_error: Option<AppError>,
    pub capture_transition_id: Option<String>,
    pub tun_available: bool,
    pub tun_unavailable_reason: Option<ice_config::UiMessage>,
    /// True when the platform must not surface TUN controls at all; the
    /// frontend hides the TUN card and switches when set.
    pub tun_ui_hidden: bool,
    /// Privileged helper daemon installed + authorized (read-only probe).
    /// Drives the「安装/卸载辅助组件」actions in Settings and Home.
    pub helper_installed: bool,
    /// Whether this platform has an installable privileged helper at all.
    /// macOS only in this release: Windows elevates the TUN core through a
    /// one-time scheduled-task setup (`ensure_tun_elevation`); unelevated
    /// transitions fail with `tun.permission_required` until that task
    /// exists. The frontend hides the helper install/uninstall actions and
    /// the install-before-enable guide when this is false, and offers the
    /// one-time task setup instead.
    pub helper_supported: bool,
    /// The installed helper's root-owned core differs from the app's bundled
    /// core (app updated): only one core version may exist, so TUN stays
    /// blocked until the helper is refreshed.
    pub helper_stale: bool,
    /// Windows scheduled-task elevation is installed (plan B): when true, TUN
    /// start/stop runs without any elevation prompt; when false the frontend
    /// runs the one-time `ensure_tun_elevation` (single UAC) before persisting
    /// the TUN-on next-start desire. Always true on non-Windows hosts (no
    /// task concept there).
    pub tun_elevation_ready: bool,
}

/// How long a `system_proxy_applied` check result is reused. The check spawns
/// `networksetup` subprocesses (list + 4 gets per service); status is polled every 2s
/// by two components, so caching keeps the subprocess storm away while the result stays
/// fresh enough for the "proxy syncing…" indicator.
const PROXY_APPLIED_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(2);

pub(crate) fn proxy_applied_cache_fresh(
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
pub(crate) fn cached_system_proxy_applied(
    state: &AppState,
    settings: &AppSettings,
) -> Option<bool> {
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
    pub(crate) fn keyword_texts(&self) -> Arc<Vec<String>> {
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
    pub(crate) sigs: Vec<Option<(SystemTime, u64)>>,
    pub(crate) n: usize,
    pub(crate) debug: bool,
    pub(crate) lines: Vec<String>,
}

pub(crate) fn file_sig(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Load the active profile from a mtime-keyed cache. `Ok(None)` when no active
/// subscription exists. Returns the built-in-default-rules-applied profile
/// (same semantics as `load_active_profile_with_default_rules`).
pub(crate) fn cached_profile(state: &AppState) -> Result<Option<ProfileCacheEntry>, AppError> {
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
    let profile = match state.profile_parse_cache.load_active_with_default_rules(
        &sub_paths,
        &index,
        auto_default_rules,
        host_platform(),
    ) {
        Ok(profile) => profile,
        Err(SubscriptionError::NoActiveSubscription) => return Ok(None),
        Err(err) => return Err(AppError::from(err)),
    };
    let entry = ProfileCacheEntry {
        sig,
        fingerprints: Arc::new(profile.route.rules.iter().map(rule_fingerprint).collect()),
        profile,
        keyword_text: Arc::new(Mutex::new(None)),
    };
    if let Ok(mut cache) = state.profile_cache.lock() {
        *cache = Some(entry.clone());
    }
    Ok(Some(entry))
}

pub(crate) fn active_profile(state: &AppState) -> Result<NormalizedProfile, AppError> {
    cached_profile(state)?
        .map(|entry| (*entry.profile).clone())
        .ok_or_else(|| AppError::new(ErrorCode::ConfigEmptyOutbounds, "no active subscription"))
}

pub(crate) fn merged_outbounds(state: &AppState) -> Result<Vec<NormalizedOutbound>, AppError> {
    Ok(list_profile_outbounds(&active_profile(state)?))
}

/// Like `merged_outbounds`, but `Ok(None)` when no active subscription exists
/// (first-run / all subscriptions removed). Read paths use this so the UI gets
/// an empty list instead of an error; mutation paths keep erroring via
/// `merged_outbounds`.
pub(crate) fn merged_outbounds_opt(
    state: &AppState,
) -> Result<Option<Vec<NormalizedOutbound>>, AppError> {
    Ok(cached_profile(state)?.map(|entry| list_profile_outbounds(&entry.profile)))
}

pub(crate) fn require_known_node_tag(state: &AppState, tag: &str) -> Result<(), AppError> {
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

pub(crate) fn cached_helper_installed(state: &AppState) -> bool {
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

pub(crate) fn reset_helper_probe_cache(state: &AppState) {
    if let Ok(mut cache) = state.helper_probe_cache.lock() {
        *cache = None;
    }
}

/// TTL for the Windows scheduled-task pin probe (one `schtasks /Query /XML`
/// subprocess per miss; status polls every 2s). Existence alone is not
/// enough: a pin-less leftover task would otherwise look ready and skip
/// the one-time XML import.
const TUN_TASK_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(10);

pub(crate) fn cached_tun_task_ready(state: &AppState) -> bool {
    if !cfg!(target_os = "windows") {
        return true;
    }
    let now = Instant::now();
    if let Ok(cache) = state.tun_task_cache.lock() {
        if let Some((at, value)) = *cache {
            if now.duration_since(at) < TUN_TASK_CACHE_TTL {
                return value;
            }
        }
    }
    let value = ice_tun_sys::tun_task_has_pin();
    if let Ok(mut cache) = state.tun_task_cache.lock() {
        *cache = Some((now, value));
    }
    value
}

#[cfg(target_os = "windows")]
pub(crate) fn reset_tun_task_cache(state: &AppState) {
    if let Ok(mut cache) = state.tun_task_cache.lock() {
        *cache = None;
    }
}

/// Live proxy-service posture: the OS proxy match plus the on-disk ownership
/// record. Read by `collect_status` (window) and by the tray menu, which both
/// answer the same question —「is the proxy service on?」— from one place.
pub(crate) struct ProxyServicePosture {
    /// Live OS match for the configured endpoints. `None` while the core is
    /// not running, the platform backend is unavailable, or an apply/restore
    /// holds the proxy lock (never wait on it from a status poll).
    pub live: Option<bool>,
    /// On-disk `applied` flag; `None` while the core is not running.
    pub recorded: Option<bool>,
}

impl ProxyServicePosture {
    /// Mirrors the Home power control (`proxyOn`): live or recorded counts as
    /// on, so an out-of-sync OS can still be restored; TUN owns capture on its
    /// own.
    pub fn engaged(&self, tun_active: bool) -> bool {
        tun_active || self.live == Some(true) || self.recorded == Some(true)
    }
}

pub(crate) fn proxy_service_posture(
    state: &AppState,
    settings: Option<&AppSettings>,
    running: bool,
) -> ProxyServicePosture {
    let recorded = running.then(|| match disk_proxy_state(&state.paths.proxy_backup()) {
        DiskProxyState::Applied => true,
        DiskProxyState::NotApplied => false,
        DiskProxyState::Unknown => true,
    });
    let live = if running && state.system_proxy_available {
        settings.and_then(|settings| cached_system_proxy_applied(state, settings))
    } else {
        None
    };
    ProxyServicePosture { live, recorded }
}

pub(crate) fn collect_status(state: &AppState) -> Result<StatusResponse, AppError> {
    // ORCH-1: never take `state.core`. Unexpected-exit reaping lives on the
    // watchdog (`reconcile_unexpected_core_exit`); status reads the snapshot.
    let core_state = state.core_snapshot.load().state.clone();
    let running = core_state.status == CoreStatus::Running;
    let paths = SubscriptionPaths::from_app(&state.paths);
    // `load_index` also sweeps leftover dirs under a process-wide commit lock.
    let count = ice_engine::read_index(&paths)
        .map(|i| i.items.len())
        .unwrap_or(0);
    let proxy_recovery_warning = state
        .proxy_recovery_warning
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();
    let proxy_available = state.system_proxy_available;
    let settings = current_settings(&state.paths).ok();
    let posture = proxy_service_posture(state, settings.as_ref(), running);
    let system_proxy_recorded = posture.recorded;
    let system_proxy_applied = posture.live;
    let capture = settings
        .as_ref()
        .map(|settings| state.capture.status(settings))
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
        helper_supported: cfg!(target_os = "macos"),
        helper_stale: crate::helper_install::helper_core_stale(state.capture.resource_dir()),
        tun_elevation_ready: cached_tun_task_ready(state),
    })
}
