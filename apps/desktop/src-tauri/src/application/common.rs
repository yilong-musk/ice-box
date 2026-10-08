// SPDX-License-Identifier: GPL-3.0-or-later

pub(crate) use super::{AppHost, AppResources};
pub(crate) use crate::capture::{
    only_tun_enabled_changed, tun_topology_changed, TrafficCapture, TunStatus,
};
pub(crate) use crate::orchestrate::{
    current_settings, endpoints_from_settings, generate_config_with_cache,
    orchestrate_apply_with_cache, orchestrate_set_proxy_mode_with_apply,
    orchestrate_start_with_cache, orphan_reclaim_cores, patch_selected_tag_default, resolve_binary,
};
pub(crate) use crate::shutdown::graceful_stop;
pub(crate) use crate::AppState;
pub(crate) use ice_config::NormalizedOutbound;
pub(crate) use ice_config::{
    load_group_selections, load_rule_overrides, redact_config_str, rule_fingerprint,
    rule_matches_fingerprint, rule_type_of, save_group_selections, save_rule_overrides,
    save_settings_for as persist_settings, set_proxy_service_enabled_for, AppError, AppPaths,
    AppSettings, CaptureIntent, ErrorCode, NormalizedProfile, ProxyMode, RuleOverrides,
    SettingsPatch, TrayDisplayMode, UiMessage,
};
pub(crate) use ice_core::{
    pid_is_alive, proxy_delay, proxy_group_heads, proxy_selected_now, read_pid, select_group,
    select_outbound, CoreState, CoreStatus, GroupHead, HealthEndpoints, TrafficDelta,
    TrafficSnapshot, DELAY_TEST_URL,
};
pub(crate) use ice_engine::{
    active_subscription, host_platform, redact_subscription_url_for_log,
    redact_subscription_url_for_ui, write_subscription_error, SubscriptionError,
    SubscriptionManager, SubscriptionPaths,
};
pub(crate) use ice_proxy_sys::{
    disk_proxy_state, is_proxy_live_applied, proxy_backup_indicates_ownership,
    recover_if_applied_hinted, DiskProxyState, ProxyEndpoints, RecoverOutcome,
};
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use std::path::Path;
#[cfg(target_os = "windows")]
pub(crate) use std::path::PathBuf;
pub(crate) use std::sync::atomic::{AtomicBool, Ordering};
pub(crate) use std::sync::mpsc::SyncSender;
pub(crate) use std::sync::{Arc, Mutex, MutexGuard};
pub(crate) use std::time::Duration;
pub(crate) use std::time::{Instant, SystemTime};
pub(crate) use uuid::Uuid;

pub(crate) use crate::lock_poisoned;

pub(crate) struct MutationGuard<'a> {
    guard: Option<MutexGuard<'a, ()>>,
    state: &'a AppState,
}

impl Drop for MutationGuard<'_> {
    fn drop(&mut self) {
        self.state.runtime_status.invalidate_probes();
        drop(self.guard.take());
        // Publish only after releasing the write lock, including error paths
        // whose rollback may have changed the committed runtime state.
        let _ = collect_status(self.state);
    }
}

pub(crate) fn lock_orchestrate(state: &AppState) -> Result<MutationGuard<'_>, AppError> {
    let guard = state
        .orchestrate
        .lock()
        .map_err(|_| lock_poisoned("orchestrate"))?;
    state.runtime_status.invalidate_probes();
    Ok(MutationGuard {
        guard: Some(guard),
        state,
    })
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

pub(crate) fn resource_dir(app: &impl AppResources) -> Option<std::path::PathBuf> {
    app.resource_dir()
}

pub(crate) fn binary_for(app: &impl AppResources) -> Result<std::path::PathBuf, AppError> {
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

/// Process memory figures for the Home memory row.
///
/// Only the core (sing-box) and the app's main process are measured; WebView
/// helpers and the privileged helper daemon are out of scope by design.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct MemoryUsage {
    /// App main process memory; `None` when it cannot be read.
    pub app_bytes: Option<u64>,
    /// Core memory; `None` while the core is not running, its pid is not
    /// readable, or the process cannot be queried. A privileged macOS core
    /// (helper / TUN) is unreadable by design and is never probed with
    /// elevation: the UI then shows the app-only figure and says so.
    pub core_bytes: Option<u64>,
    /// Sum of the readable parts; `0` when nothing could be read (the UI shows
    /// `—` rather than a fake number).
    pub total_bytes: u64,
}

/// The exit Home shows. Member lists stay on the Nodes page and the tray.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SelectedOutbound {
    pub tag: String,
    pub outbound_type: String,
    pub group_now: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct StatusResponse {
    /// Monotonic publication revision of the entire committed runtime read.
    pub revision: u64,
    pub sampled_at_ms: u64,
    /// A mutation is in progress; this response is the last committed view.
    pub refresh_pending: bool,
    pub diagnostics: crate::runtime_status::ProbeFreshness,
    pub workers: Vec<crate::workers::WorkerStatus>,
    pub core: CoreState,
    pub subscription_count: usize,
    /// Process memory for the Home memory row.
    pub memory: MemoryUsage,
    pub proxy_recovery_warning: Vec<UiMessage>,
    /// Live OS match when the platform backend is available and core is running.
    pub system_proxy_applied: Option<bool>,
    /// On-disk ownership flag; permits stopping capture after external OS changes.
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
    /// Drives helper install/uninstall actions in Settings and Home.
    pub helper_installed: bool,
    /// Whether this platform has an installable privileged helper at all.
    /// macOS only in this release: Windows elevates the TUN core through a
    /// one-time scheduled-task setup (`ensure_tun_elevation`); unelevated
    /// transitions fail with `tun.permission_required` until that task
    /// exists. The frontend hides the helper install/uninstall actions and
    /// the install-before-enable guide when this is false, and offers the
    /// one-time task setup instead.
    pub helper_supported: bool,
    /// Whether this platform can register an OS login item at all (macOS and
    /// Windows). The Settings page hides the Startup card when it cannot, so
    /// the switch never offers an action that can only fail.
    pub launch_at_login_supported: bool,
    /// Menu-bar readout in Settings. macOS only; the frontend must not infer
    /// this from the user agent.
    pub tray_display_supported: bool,
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
    /// The active profile has at least one outbound. Home uses this instead of
    /// the full node list to choose the empty state.
    pub has_nodes: bool,
    /// Resolved exit for the Home row: settings tag, else the first outbound,
    /// with a strategy group's live `now` when that cache is warm.
    pub selected_outbound: Option<SelectedOutbound>,
}

/// Slow consistency fallback. Mutations and window activation invalidate the
/// memo immediately; the one-second tray readout never starts an OS probe.
const PROXY_APPLIED_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(30);

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
    // Publish the memo before releasing `proxy`: an apply/restore that follows
    // clears it afterwards, so a probe woken at the start of a mutation cannot
    // write a pre-mutation value over that clear.
    if let Ok(mut cache) = state.proxy_applied_cache.lock() {
        *cache = Some((endpoints, now, value));
    }
    drop(proxy);
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

/// Subscription rules kept only while the Rules page is open.
pub(crate) struct RulePage {
    pub(crate) rules: Arc<Vec<serde_json::Value>>,
    /// Parallel to `rules`.
    pub(crate) fingerprints: Arc<Vec<String>>,
    /// Lowercase-serialized rule text for keyword search. Built on the first
    /// keyword query (10k rules ≈ a few MB), then reused. Never allocated for
    /// non-keyword reads.
    keyword_text: Mutex<Option<Arc<Vec<String>>>>,
}

impl RulePage {
    fn from_rules(rules: Vec<serde_json::Value>) -> Self {
        let fingerprints = Arc::new(rules.iter().map(rule_fingerprint).collect());
        Self {
            rules: Arc::new(rules),
            fingerprints,
            keyword_text: Mutex::new(None),
        }
    }

    pub(crate) fn keyword_texts(&self) -> Arc<Vec<String>> {
        let mut slot = self
            .keyword_text
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(texts) = slot.as_ref() {
            return Arc::clone(texts);
        }
        let texts: Arc<Vec<String>> = Arc::new(
            self.rules
                .iter()
                .map(|rule| {
                    serde_json::to_string(rule)
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                })
                .collect(),
        );
        *slot = Some(Arc::clone(&texts));
        texts
    }
}

/// Cached nodes and groups for the active profile. Rule and DNS bodies are
/// not retained here; see [`ProfileCacheEntry::rule_page`].
#[derive(Clone)]
pub struct ProfileCacheEntry {
    sig: ProfileSig,
    pub profile: Arc<NormalizedProfile>,
    node_tags: Arc<std::collections::HashSet<String>>,
    rule_page: Arc<Mutex<Option<Arc<RulePage>>>>,
}

impl ProfileCacheEntry {
    /// Subscription rules, loaded from disk on the first Rules-page read and
    /// shared until [`Self::clear_rule_page`].
    pub(crate) fn rule_page(&self, state: &AppState) -> Result<Arc<RulePage>, AppError> {
        let mut slot = self
            .rule_page
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(page) = slot.as_ref() {
            return Ok(Arc::clone(page));
        }
        let rules = match read_full_profile(state)? {
            Some(mut profile) => std::mem::take(&mut profile.route.rules),
            None => Vec::new(),
        };
        let page = Arc::new(RulePage::from_rules(rules));
        *slot = Some(Arc::clone(&page));
        Ok(page)
    }

    pub(crate) fn clear_rule_page(&self) {
        let mut slot = self
            .rule_page
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = None;
    }
}

/// Drop the Rules-page copy of subscription rules, fingerprints, and the
/// keyword index. Home and the tray do not need them.
pub(crate) fn drop_rule_keyword_cache(state: &AppState) {
    let Ok(cache) = state.profile_cache.lock() else {
        return;
    };
    if let Some(entry) = cache.as_ref() {
        entry.clear_rule_page();
    }
}

/// Full profile from disk, including rules and DNS. Does not touch the
/// long-lived parse cache.
fn read_full_profile(state: &AppState) -> Result<Option<NormalizedProfile>, AppError> {
    let sub_paths = SubscriptionPaths::from_app(&state.paths);
    let index = ice_engine::read_index(&sub_paths).map_err(AppError::from)?;
    let auto_default_rules = current_settings(&state.paths)
        .map(|settings| settings.auto_default_rules)
        .unwrap_or(true);
    match ice_engine::load_active_profile_with_default_rules(
        &sub_paths,
        &index,
        auto_default_rules,
        host_platform(),
    ) {
        Ok(profile) => Ok(Some(
            Arc::try_unwrap(profile).unwrap_or_else(|arc| (*arc).clone()),
        )),
        Err(SubscriptionError::NoActiveSubscription) => Ok(None),
        Err(err) => Err(AppError::from(err)),
    }
}

#[cfg(test)]
pub(crate) fn subscription_rules_retained(state: &AppState) -> bool {
    let Ok(cache) = state.profile_cache.lock() else {
        return false;
    };
    cache.as_ref().is_some_and(|entry| {
        entry
            .rule_page
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
    })
}

pub(crate) type LogViewCache = crate::log_view::LogViewReader;

pub(crate) fn file_sig(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Load the active profile from a mtime-keyed cache. `Ok(None)` when no active
/// subscription exists.
///
/// The cached profile has nodes, groups, and rule-set metadata. `route.rules`
/// and `dns` are not retained; config generation re-reads them, and the Rules
/// page loads them into [`ProfileCacheEntry::rule_page`].
pub(crate) fn cached_profile(state: &AppState) -> Result<Option<ProfileCacheEntry>, AppError> {
    let sub_paths = SubscriptionPaths::from_app(&state.paths);
    let index = ice_engine::read_index(&sub_paths).map_err(AppError::from)?;
    let active = active_subscription(&index);
    let sig = ProfileSig {
        index: file_sig(&sub_paths.index()),
        profile: active.and_then(|m| file_sig(&sub_paths.profile(m.id))),
        settings: file_sig(&state.paths.settings()),
    };
    let auto_default_rules = current_settings(&state.paths)
        .map(|settings| settings.auto_default_rules)
        .unwrap_or(true);
    let platform = host_platform();
    if let Ok(mut cache) = state.profile_cache.lock() {
        if let Some(entry) = cache.as_mut() {
            if entry.sig == sig {
                // A config rebuild replaces the parse-cache Arc. Adopt it so
                // the previous resident copy does not stay alive beside it.
                if let Some(shared) = state.profile_parse_cache.resident_if_current(
                    &sub_paths,
                    &index,
                    auto_default_rules,
                    platform,
                ) {
                    if !Arc::ptr_eq(&entry.profile, &shared) {
                        entry.profile = shared;
                    }
                }
                return Ok(Some(entry.clone()));
            }
        }
    }
    let profile = match state.profile_parse_cache.load_resident(
        &sub_paths,
        &index,
        auto_default_rules,
        platform,
    ) {
        Ok(profile) => profile,
        Err(SubscriptionError::NoActiveSubscription) => return Ok(None),
        Err(err) => return Err(AppError::from(err)),
    };
    let entry = ProfileCacheEntry {
        sig,
        node_tags: Arc::new(profile.all_outbounds().map(|o| o.tag.clone()).collect()),
        profile,
        rule_page: Arc::new(Mutex::new(None)),
    };
    if let Ok(mut cache) = state.profile_cache.lock() {
        *cache = Some(entry.clone());
    }
    Ok(Some(entry))
}

pub(crate) fn active_profile(state: &AppState) -> Result<Arc<NormalizedProfile>, AppError> {
    cached_profile(state)?
        .map(|entry| entry.profile)
        .ok_or_else(|| AppError::new(ErrorCode::ConfigEmptyOutbounds, "no active subscription"))
}

/// Test adapter for checking the cached profile's group-first ordering.
#[cfg(test)]
pub(crate) fn merged_outbounds_opt(
    state: &AppState,
) -> Result<Option<Vec<NormalizedOutbound>>, AppError> {
    Ok(cached_profile(state)?.map(|entry| ice_engine::list_profile_outbounds(&entry.profile)))
}

pub(crate) fn require_known_node_tag(state: &AppState, tag: &str) -> Result<(), AppError> {
    let entry = cached_profile(state)?
        .ok_or_else(|| AppError::new(ErrorCode::ConfigEmptyOutbounds, "no active subscription"))?;
    if !entry.node_tags.contains(tag) {
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
    // Drop the memo first: invalidating wakes the probe worker, which must not
    // re-read the value being discarded.
    if let Ok(mut cache) = state.helper_probe_cache.lock() {
        *cache = None;
    }
    state.runtime_status.invalidate_probes();
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
    // Drop the memo first: invalidating wakes the probe worker, which must not
    // re-read the value being discarded.
    if let Ok(mut cache) = state.tun_task_cache.lock() {
        *cache = None;
    }
    state.runtime_status.invalidate_probes();
}

/// Live proxy-service posture: the OS proxy match plus the on-disk ownership
/// record. Read by `collect_status` (window) and by the tray menu, which both
/// derive proxy-service engagement from the same source.
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
    let mut posture = cached_proxy_service_posture(state, settings, running);
    if running && state.system_proxy_available {
        posture.live = settings.and_then(|settings| cached_system_proxy_applied(state, settings));
    }
    posture
}

/// Display-only read: never invokes a system-proxy subprocess.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn cached_proxy_service_posture(
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
        settings.and_then(|settings| {
            proxy_applied_cache_fresh(state, &endpoints_from_settings(settings), Instant::now())
        })
    } else {
        None
    };
    ProxyServicePosture { live, recorded }
}

/// Ask for a re-sample after window activation or a state announcement.
///
/// This does not withhold the current probe sample: readers keep the last value
/// (subject to `PROBE_MAX_AGE` and the core-generation / settings checks in
/// `collect_status`) until the new one lands. Withholding it here would drop
/// `system_proxy_applied` to `None` for the length of every probe and make the
/// Home subtitle flicker on each focus. Mutations invalidate through
/// `lock_orchestrate` instead, where the old value really is wrong.
///
/// The memo is cleared so the re-sample reads the OS; the core watchdog picks
/// the request up on its next tick and wakes the probe worker.
pub(crate) fn request_runtime_probe_refresh(state: &AppState) {
    if let Ok(mut cache) = state.proxy_applied_cache.lock() {
        *cache = None;
    }
    state.core_snapshot.request_probe_refresh();
}

/// Announce a state change to the window and the tray.
///
/// Anything the window did not itself initiate (tray menu actions, launch-time
/// restore) must not leave the Home page showing stale status or mode until its
/// fallback poll: the event makes the UI re-read both, and the tray re-derives
/// its menu items at once. Callers announce after releasing their locks.
pub(crate) fn broadcast_state_change(app: &impl AppHost) {
    app.state_changed();
}

/// How long a process-memory sample may be reused. The Home row does not need
/// a fresh syscall on every 10s status poll; a core start/stop still resamples.
const MEMORY_REFRESH: Duration = Duration::from_secs(30);

/// Shared live group exits (`now` only). Member tags stay on the profile.
/// Home does not use this document: it reads one group's `now`.
pub(crate) const PROXY_GROUPS_TTL: Duration = Duration::from_secs(10);

struct CachedGroups {
    endpoints: HealthEndpoints,
    generation: u64,
    fetched_at: Instant,
    groups: Arc<Vec<GroupHead>>,
}

struct CachedNow {
    endpoints: HealthEndpoints,
    generation: u64,
    tag: String,
    fetched_at: Instant,
    now: String,
}

struct CachedMemory {
    at: Instant,
    running: bool,
    usage: MemoryUsage,
}

/// Short-lived reads that status, nodes, and the tray share.
#[derive(Default)]
pub struct LiveCache {
    groups: Mutex<Option<CachedGroups>>,
    /// Single-flight fetch. Never held by a status read that is not refreshing.
    groups_fetch: Mutex<()>,
    selected_now: Mutex<Option<CachedNow>>,
    /// Single-flight fetch of one group's `now`. Separate from `groups_fetch`
    /// so Home does not queue behind a full proxy-map read.
    now_fetch: Mutex<()>,
    /// Home is the visible tab. Only then does a status poll refresh the
    /// selected group's `now`.
    home_interest: AtomicBool,
    memory: Mutex<Option<CachedMemory>>,
}

impl LiveCache {
    pub(crate) fn set_home_interest(&self, active: bool) {
        self.home_interest.store(active, Ordering::Relaxed);
    }

    pub(crate) fn home_interest(&self) -> bool {
        self.home_interest.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn seed_group_heads_for_test(
        &self,
        endpoints: HealthEndpoints,
        generation: u64,
        groups: Vec<GroupHead>,
        age: Duration,
    ) {
        *self.groups.lock().unwrap_or_else(|e| e.into_inner()) = Some(CachedGroups {
            endpoints,
            generation,
            fetched_at: Instant::now().checked_sub(age).unwrap_or_else(Instant::now),
            groups: Arc::new(groups),
        });
    }
}

fn groups_match(hit: &CachedGroups, endpoints: &HealthEndpoints, generation: u64) -> bool {
    hit.generation == generation && &hit.endpoints == endpoints
}

/// `refresh` fetches when the sample is missing or older than [`PROXY_GROUPS_TTL`].
/// A generation change never serves the previous core's groups. Fetch failure
/// keeps the last sample for this core. The sample is group exits only.
pub(crate) fn load_proxy_groups(
    cache: &LiveCache,
    endpoints: &HealthEndpoints,
    generation: u64,
    refresh: bool,
) -> Option<Arc<Vec<GroupHead>>> {
    let cached = |slot: &Option<CachedGroups>| {
        slot.as_ref()
            .filter(|hit| groups_match(hit, endpoints, generation))
            .map(|hit| hit.groups.clone())
    };
    let fresh = {
        let slot = cache.groups.lock().unwrap_or_else(|e| e.into_inner());
        slot.as_ref()
            .is_some_and(|hit| {
                groups_match(hit, endpoints, generation)
                    && hit.fetched_at.elapsed() < PROXY_GROUPS_TTL
            })
            .then(|| cached(&slot))
            .flatten()
    };
    if fresh.is_some() {
        return fresh;
    }
    if !refresh {
        let slot = cache.groups.lock().unwrap_or_else(|e| e.into_inner());
        return cached(&slot);
    }
    let _fetch = cache.groups_fetch.lock().unwrap_or_else(|e| e.into_inner());
    {
        let slot = cache.groups.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hit) = slot.as_ref() {
            if groups_match(hit, endpoints, generation)
                && hit.fetched_at.elapsed() < PROXY_GROUPS_TTL
            {
                return Some(hit.groups.clone());
            }
        }
    }
    match proxy_group_heads(endpoints) {
        Ok(groups) => {
            let groups = Arc::new(groups);
            *cache.groups.lock().unwrap_or_else(|e| e.into_inner()) = Some(CachedGroups {
                endpoints: endpoints.clone(),
                generation,
                fetched_at: Instant::now(),
                groups: groups.clone(),
            });
            Some(groups)
        }
        Err(error) => {
            tracing::debug!(%error, "clash /proxies refresh failed");
            let slot = cache.groups.lock().unwrap_or_else(|e| e.into_inner());
            cached(&slot)
        }
    }
}

fn now_hit_matches(
    hit: &CachedNow,
    endpoints: &HealthEndpoints,
    generation: u64,
    tag: &str,
) -> bool {
    hit.generation == generation && hit.tag == tag && &hit.endpoints == endpoints
}

fn now_from_groups(groups: &[GroupHead], tag: &str) -> Option<String> {
    groups
        .iter()
        .find(|group| group.tag == tag)
        .map(|group| group.now.clone())
        .filter(|now| !now.is_empty())
}

/// The selected group's live exit.
///
/// A fresh group-head sample (Nodes or the tray) is reused. Otherwise Home
/// fetches `GET /proxies/{tag}` and keeps only `now`. `refresh` is false when
/// Home is not the visible tab: the last sample for this core is served and
/// nothing is fetched.
pub(crate) fn load_selected_now(
    cache: &LiveCache,
    endpoints: &HealthEndpoints,
    generation: u64,
    tag: &str,
    refresh: bool,
) -> Option<String> {
    {
        let slot = cache.groups.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hit) = slot.as_ref() {
            if groups_match(hit, endpoints, generation)
                && (hit.fetched_at.elapsed() < PROXY_GROUPS_TTL || !refresh)
            {
                if let Some(now) = now_from_groups(&hit.groups, tag) {
                    return Some(now);
                }
            }
        }
    }
    let cached_now = |slot: &Option<CachedNow>| {
        slot.as_ref()
            .filter(|hit| now_hit_matches(hit, endpoints, generation, tag))
            .map(|hit| hit.now.clone())
            .filter(|now| !now.is_empty())
    };
    {
        let slot = cache.selected_now.lock().unwrap_or_else(|e| e.into_inner());
        if slot.as_ref().is_some_and(|hit| {
            now_hit_matches(hit, endpoints, generation, tag)
                && hit.fetched_at.elapsed() < PROXY_GROUPS_TTL
        }) {
            return cached_now(&slot);
        }
    }
    if !refresh {
        let slot = cache.selected_now.lock().unwrap_or_else(|e| e.into_inner());
        return cached_now(&slot);
    }
    let _fetch = cache.now_fetch.lock().unwrap_or_else(|e| e.into_inner());
    {
        let slot = cache.selected_now.lock().unwrap_or_else(|e| e.into_inner());
        if slot.as_ref().is_some_and(|hit| {
            now_hit_matches(hit, endpoints, generation, tag)
                && hit.fetched_at.elapsed() < PROXY_GROUPS_TTL
        }) {
            return cached_now(&slot);
        }
    }
    match proxy_selected_now(endpoints, tag) {
        Ok(now) => {
            *cache.selected_now.lock().unwrap_or_else(|e| e.into_inner()) = Some(CachedNow {
                endpoints: endpoints.clone(),
                generation,
                tag: tag.to_string(),
                fetched_at: Instant::now(),
                now: now.clone(),
            });
            (!now.is_empty()).then_some(now)
        }
        Err(error) => {
            tracing::debug!(%error, "clash proxy now refresh failed");
            let slot = cache.selected_now.lock().unwrap_or_else(|e| e.into_inner());
            cached_now(&slot)
        }
    }
}

/// Memory of the app process plus the running core, when readable.
///
/// Reused for [`MEMORY_REFRESH`] so a quiet status poll does not syscall.
/// An unknown core figure while the core is running is not cached: the pid
/// file often appears on the next poll.
fn memory_usage(state: &AppState, running: bool) -> MemoryUsage {
    // Hold the sample lock across the syscalls so parallel status polls share
    // one reading instead of publishing two revisions for the same moment.
    let mut slot = state
        .live_cache
        .memory
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = slot.as_ref() {
        if hit.running == running && hit.at.elapsed() < MEMORY_REFRESH {
            return hit.usage.clone();
        }
    }
    let app_bytes = crate::proc_memory::process_memory_bytes(std::process::id()).ok();
    let core_bytes = core_memory_bytes(state);
    let total_bytes = app_bytes.unwrap_or(0) + core_bytes.unwrap_or(0);
    let usage = MemoryUsage {
        app_bytes,
        core_bytes,
        total_bytes,
    };
    if !(running && usage.core_bytes.is_none()) {
        *slot = Some(CachedMemory {
            at: Instant::now(),
            running,
            usage: usage.clone(),
        });
    }
    usage
}

/// Core memory: only while the core is running and its pid file holds a live
/// pid. Everything else (not running, unreadable pid, unreadable process) is
/// `None`.
fn core_memory_bytes(state: &AppState) -> Option<u64> {
    if state.core_snapshot.load().state.status != CoreStatus::Running {
        return None;
    }
    let pid = read_pid(&state.paths.pid()).ok().flatten()?;
    if !pid_is_alive(pid) {
        return None;
    }
    crate::proc_memory::process_memory_bytes(pid).ok()
}

pub(crate) fn collect_status(state: &AppState) -> Result<StatusResponse, AppError> {
    let _read = state
        .runtime_status
        .read_gate
        .lock()
        .map_err(|_| lock_poisoned("runtime read"))?;
    // Readers never queue behind a multi-second capture transition.
    let _orch = match state.orchestrate.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::WouldBlock) => {
            let mut status = state.runtime_status.latest().ok_or_else(|| {
                AppError::new(
                    ErrorCode::CoreInvalidState,
                    "runtime status is initializing",
                )
            })?;
            status.refresh_pending = true;
            status.diagnostics.stale = true;
            return Ok(status);
        }
        Err(std::sync::TryLockError::Poisoned(_)) => return Err(lock_poisoned("orchestrate")),
    };
    // ORCH-1: never take `state.core`. Unexpected-exit reaping lives on the
    // watchdog (`reconcile_unexpected_core_exit`); status reads the snapshot.
    let core_snapshot = state.core_snapshot.load();
    let core_state = core_snapshot.state.clone();
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
    let posture = cached_proxy_service_posture(state, settings.as_ref(), running);
    let system_proxy_recorded = posture.recorded;
    let (probes, mut diagnostics) = state.runtime_status.probes_at(Instant::now());
    diagnostics.stale |= probes.core_generation != core_snapshot.generation
        || probes.settings_signature != file_sig(&state.paths.settings());
    let system_proxy_applied = if running && proxy_available && !diagnostics.stale {
        probes.system_proxy_applied
    } else {
        None
    };
    let capture = settings
        .as_ref()
        .map(|settings| state.capture.status(settings))
        .unwrap_or_else(|| state.capture.status(&ice_config::AppSettings::default()));
    let (has_nodes, selected_outbound) = super::outbound_summary(state, settings.as_ref(), running);
    Ok(state.runtime_status.publish(StatusResponse {
        revision: 0,
        sampled_at_ms: crate::runtime_status::timestamp_ms(),
        refresh_pending: false,
        diagnostics,
        workers: state.workers.statuses(),
        core: core_state,
        subscription_count: count,
        memory: memory_usage(state, running),
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
        helper_installed: probes.helper_installed,
        helper_supported: cfg!(target_os = "macos"),
        launch_at_login_supported: crate::autostart::SUPPORTED,
        tray_display_supported: cfg!(target_os = "macos"),
        helper_stale: probes.helper_stale,
        tun_elevation_ready: probes.tun_elevation_ready,
        has_nodes,
        selected_outbound,
    }))
}

/// Slow diagnostics run outside status reads and outside mutation locks.
pub(crate) fn refresh_runtime_probes(state: &AppState) -> bool {
    let Ok(_flight) = state.runtime_status.probe_refresh.try_lock() else {
        return false;
    };
    // Due at `PROBE_INTERVAL`, which is earlier than the sample stops being
    // served (`PROBE_MAX_AGE`): readers keep the old value while this runs.
    let Some(epoch) = state.runtime_status.begin_refresh(Instant::now()) else {
        return false;
    };
    let core = state.core_snapshot.load();
    let settings_sig = file_sig(&state.paths.settings());
    let values = (|| {
        let settings = current_settings(&state.paths)?;
        let system_proxy_applied =
            if core.state.status == CoreStatus::Running && state.system_proxy_available {
                Some(
                    cached_system_proxy_applied(state, &settings).ok_or_else(|| {
                        AppError::new(ErrorCode::CoreInvalidState, "proxy transition in progress")
                    })?,
                )
            } else {
                None
            };
        Ok(crate::runtime_status::ProbeValues {
            core_generation: core.generation,
            settings_signature: settings_sig,
            system_proxy_applied,
            helper_installed: cached_helper_installed(state),
            helper_stale: crate::helper_install::helper_core_stale(state.capture.resource_dir()),
            tun_elevation_ready: cached_tun_task_ready(state),
        })
    })();
    if core.generation != state.core_snapshot.load().generation
        || settings_sig != file_sig(&state.paths.settings())
    {
        state.runtime_status.invalidate_probes();
        return false;
    }
    state.runtime_status.complete_probe(
        epoch,
        values.map_err(|error: AppError| error.ui_message()),
        Instant::now(),
    )
}
