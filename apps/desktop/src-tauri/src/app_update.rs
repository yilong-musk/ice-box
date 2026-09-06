//! In-app update checks and installation (architecture §25).
//!
//! Integrity is minisign via `tauri-plugin-updater`. Apple / Authenticode
//! signing is not required. Prompt throttle and skipped-version state live in
//! `update-check.json`, not `settings.json`.

use crate::orchestrate::current_settings;
use crate::shutdown::graceful_stop;
use crate::AppState;
use chrono::{DateTime, Duration, Utc};
use ice_config::{is_loopback_host, write_json_atomic, AppError, AppSettings};
use ice_core::CoreStatus;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration as StdDuration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;
use url::Url;

/// Background GitHub checks at most once per this interval.
pub const CHECK_INTERVAL: Duration = Duration::hours(24);

pub const ERR_UPDATE_CHECK_FAILED: &str = "update.check_failed";
pub const ERR_UPDATE_FEED_UNAVAILABLE: &str = "update.feed_unavailable";
pub const ERR_UPDATE_INSTALL_FAILED: &str = "update.install_failed";
pub const ERR_UPDATE_DISABLED: &str = "update.disabled";
pub const UPDATE_PROGRESS_EVENT: &str = "app-update://progress";

static UPDATE_INSTALLING: AtomicBool = AtomicBool::new(false);

/// True while an update is being applied so `ExitRequested` can let the
/// Windows installer take over instead of `prevent_exit()`.
pub fn update_installing() -> bool {
    UPDATE_INSTALLING.load(Ordering::SeqCst)
}

/// Persisted check / last-known-available state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateCheckState {
    pub last_check_at: Option<DateTime<Utc>>,
    /// Legacy field from the dialog-prompt design; ignored by current UI.
    #[serde(default)]
    pub last_prompt_at: Option<DateTime<Utc>>,
    /// Legacy field from the dialog-prompt design; ignored by current UI.
    #[serde(default)]
    pub skipped_version: Option<String>,
    #[serde(default)]
    pub available_version: Option<String>,
    #[serde(default)]
    pub available_notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CheckAppUpdateRequest {
    #[serde(default)]
    pub background: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckAppUpdateResponse {
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub skipped: bool,
    pub should_prompt: bool,
}

#[derive(Debug, Deserialize)]
pub struct SkipAppUpdateRequest {
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateProgressPayload {
    pub phase: String,
    pub downloaded: u64,
    pub content_length: Option<u64>,
}

fn empty_response() -> CheckAppUpdateResponse {
    CheckAppUpdateResponse {
        available: false,
        version: None,
        notes: None,
        skipped: false,
        should_prompt: false,
    }
}

/// Last GitHub result, used when a background check is still inside the 24h window.
pub fn cached_check_response(state: &UpdateCheckState) -> CheckAppUpdateResponse {
    match state.available_version.as_deref() {
        Some(version) if !normalize_version(version).is_empty() => CheckAppUpdateResponse {
            available: true,
            version: Some(normalize_version(version).to_string()),
            notes: state.available_notes.clone(),
            skipped: false,
            should_prompt: false,
        },
        _ => empty_response(),
    }
}

/// Strip a leading `v` so `v0.1.6` and `0.1.6` compare equal.
pub fn normalize_version(version: &str) -> &str {
    let version = version.trim();
    version
        .strip_prefix('v')
        .or_else(|| version.strip_prefix('V'))
        .unwrap_or(version)
}

pub fn load_update_check_state(path: &std::path::Path) -> UpdateCheckState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_update_check_state(
    path: &std::path::Path,
    state: &UpdateCheckState,
) -> Result<(), AppError> {
    write_json_atomic(path, state).map_err(|err| {
        AppError::with_code(
            ERR_UPDATE_CHECK_FAILED,
            format!("write update-check.json: {err}"),
        )
    })
}

pub fn check_is_due(state: &UpdateCheckState, now: DateTime<Utc>) -> bool {
    match state.last_check_at {
        None => true,
        Some(at) => now >= at + CHECK_INTERVAL,
    }
}

/// HTTP proxy URL for the updater when the mixed inbound is reachable.
pub fn updater_proxy_url(settings: &AppSettings, core_running: bool) -> Option<Url> {
    if !core_running {
        return None;
    }
    let host = if is_loopback_host(&settings.mixed_listen) {
        settings.mixed_listen.as_str()
    } else {
        // `allow_lan` binds 0.0.0.0; Mixed is still reachable on loopback.
        "127.0.0.1"
    };
    format!("http://{host}:{}", settings.mixed_port)
        .parse()
        .ok()
}

fn core_is_running(state: &AppState) -> bool {
    state
        .core
        .lock()
        .map(|core| core.state().status == CoreStatus::Running)
        .unwrap_or(false)
}

fn map_updater_err(code: &str, err: tauri_plugin_updater::Error) -> AppError {
    AppError::with_code(code, err.to_string())
}

/// `ReleaseNotFound` is a missing `latest.json` (typical before the first
/// updater-capable GitHub Release), not a Mixed-proxy connectivity failure.
pub fn check_error_code(err: &tauri_plugin_updater::Error) -> &'static str {
    match err {
        tauri_plugin_updater::Error::ReleaseNotFound => ERR_UPDATE_FEED_UNAVAILABLE,
        _ => ERR_UPDATE_CHECK_FAILED,
    }
}

fn map_check_err(err: tauri_plugin_updater::Error) -> AppError {
    AppError::with_code(check_error_code(&err), err.to_string())
}

fn build_updater(
    app: &AppHandle,
    proxy: Option<Url>,
) -> Result<tauri_plugin_updater::Updater, AppError> {
    let mut builder = app.updater_builder().timeout(StdDuration::from_secs(30));
    builder = if let Some(proxy) = proxy {
        builder.proxy(proxy)
    } else {
        builder.no_proxy()
    };
    #[cfg(windows)]
    {
        let handle = app.clone();
        builder = builder.on_before_exit(move || {
            UPDATE_INSTALLING.store(true, Ordering::SeqCst);
            if let Err(err) = stop_for_update(&handle) {
                tracing::error!(error = %err, "updater on_before_exit: stop failed");
            }
        });
    }
    builder
        .build()
        .map_err(|err| map_updater_err(ERR_UPDATE_CHECK_FAILED, err))
}

fn stop_for_update(app: &AppHandle) -> Result<(), AppError> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(());
    };
    let binary = crate::commands::binary_for(app).unwrap_or_default();
    graceful_stop(state.inner(), binary)
}

#[tauri::command]
pub async fn check_app_update(
    app: AppHandle,
    req: CheckAppUpdateRequest,
) -> Result<CheckAppUpdateResponse, AppError> {
    let (settings, paths, core_running) = {
        let state = app.state::<AppState>();
        let settings = current_settings(&state.paths).unwrap_or_default();
        if req.background && !settings.check_app_updates {
            return Ok(empty_response());
        }
        (
            settings,
            state.paths.clone(),
            core_is_running(state.inner()),
        )
    };

    let mut disk = load_update_check_state(&paths.update_check());
    // debug / tauri dev: never hit GitHub in the background; still surface a
    // previously cached available version so the sidebar indicator can show.
    if cfg!(debug_assertions) && req.background {
        return Ok(cached_check_response(&disk));
    }

    let now = Utc::now();
    if req.background && !check_is_due(&disk, now) {
        return Ok(cached_check_response(&disk));
    }

    let proxy = updater_proxy_url(&settings, core_running);
    let updater = build_updater(&app, proxy)?;
    let found = updater.check().await.map_err(map_check_err)?;

    disk.last_check_at = Some(now);
    let Some(update) = found else {
        disk.available_version = None;
        disk.available_notes = None;
        save_update_check_state(&paths.update_check(), &disk)?;
        return Ok(empty_response());
    };

    let version = update.version.clone();
    disk.available_version = Some(version.clone());
    disk.available_notes = update.body.clone();
    save_update_check_state(&paths.update_check(), &disk)?;

    Ok(CheckAppUpdateResponse {
        available: true,
        notes: update.body,
        version: Some(version),
        skipped: false,
        should_prompt: false,
    })
}

#[tauri::command]
pub async fn record_update_prompt(app: AppHandle) -> Result<(), AppError> {
    let paths = app.state::<AppState>().paths.clone();
    let mut disk = load_update_check_state(&paths.update_check());
    disk.last_prompt_at = Some(Utc::now());
    save_update_check_state(&paths.update_check(), &disk)
}

#[tauri::command]
pub async fn skip_app_update(app: AppHandle, req: SkipAppUpdateRequest) -> Result<(), AppError> {
    let version = normalize_version(&req.version).to_string();
    if version.is_empty() {
        return Err(AppError::with_code(
            ERR_UPDATE_CHECK_FAILED,
            "skip_app_update requires a version",
        ));
    }
    let paths = app.state::<AppState>().paths.clone();
    let mut disk = load_update_check_state(&paths.update_check());
    disk.skipped_version = Some(version);
    disk.last_prompt_at = Some(Utc::now());
    save_update_check_state(&paths.update_check(), &disk)
}

#[tauri::command]
pub async fn install_app_update(app: AppHandle) -> Result<(), AppError> {
    if cfg!(debug_assertions) {
        return Err(AppError::with_code(
            ERR_UPDATE_DISABLED,
            "app updates cannot be installed from a debug / tauri dev build",
        ));
    }

    let (settings, core_running) = {
        let state = app.state::<AppState>();
        (
            current_settings(&state.paths).unwrap_or_default(),
            core_is_running(state.inner()),
        )
    };
    let proxy = updater_proxy_url(&settings, core_running);
    let updater = build_updater(&app, proxy)?;
    let update = updater
        .check()
        .await
        .map_err(|err| map_updater_err(ERR_UPDATE_INSTALL_FAILED, err))?
        .ok_or_else(|| AppError::with_code(ERR_UPDATE_INSTALL_FAILED, "no update is available"))?;

    let handle = app.clone();
    let mut downloaded: u64 = 0;
    let bytes = update
        .download(
            move |chunk, content_length| {
                downloaded = downloaded.saturating_add(chunk as u64);
                let _ = handle.emit(
                    UPDATE_PROGRESS_EVENT,
                    UpdateProgressPayload {
                        phase: "progress".into(),
                        downloaded,
                        content_length,
                    },
                );
            },
            || {},
        )
        .await
        .map_err(|err| map_updater_err(ERR_UPDATE_INSTALL_FAILED, err))?;

    UPDATE_INSTALLING.store(true, Ordering::SeqCst);
    if let Err(err) = stop_for_update(&app) {
        UPDATE_INSTALLING.store(false, Ordering::SeqCst);
        return Err(err);
    }

    update.install(bytes).map_err(|err| {
        UPDATE_INSTALLING.store(false, Ordering::SeqCst);
        map_updater_err(ERR_UPDATE_INSTALL_FAILED, err)
    })?;

    // Windows: `install` exits the process after launching NSIS (which restarts
    // the app). macOS replaces the bundle in-place and needs an explicit relaunch.
    #[cfg(not(windows))]
    {
        app.restart();
    }
    #[cfg(windows)]
    {
        Ok(())
    }
}

/// Persist-only when the user flipped the auto-check switch and nothing else.
pub fn only_check_app_updates_changed(previous: &AppSettings, next: &AppSettings) -> bool {
    let mut left = previous.clone();
    let mut right = next.clone();
    left.check_app_updates = true;
    right.check_app_updates = true;
    left == right && previous.check_app_updates != next.check_app_updates
}

#[cfg(test)]
mod tests {
    use super::*;
    use ice_config::AppPaths;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_paths(label: &str) -> AppPaths {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-update-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = AppPaths::new(&dir);
        paths.ensure_dirs().unwrap();
        paths
    }

    #[test]
    fn normalize_version_strips_v_prefix() {
        assert_eq!(normalize_version("v0.1.6"), "0.1.6");
        assert_eq!(normalize_version("0.1.6"), "0.1.6");
        assert_eq!(normalize_version(" V0.1.6 "), "0.1.6");
    }

    #[test]
    fn cached_check_response_surfaces_last_available_version() {
        assert!(!cached_check_response(&UpdateCheckState::default()).available);
        let state = UpdateCheckState {
            available_version: Some(" v0.1.6 ".into()),
            available_notes: Some("fixes".into()),
            ..UpdateCheckState::default()
        };
        let response = cached_check_response(&state);
        assert!(response.available);
        assert_eq!(response.version.as_deref(), Some("0.1.6"));
        assert_eq!(response.notes.as_deref(), Some("fixes"));
        assert!(!response.should_prompt);
    }

    #[test]
    fn check_is_due_matches_24h() {
        let now = Utc::now();
        assert!(check_is_due(&UpdateCheckState::default(), now));
        let recent = UpdateCheckState {
            last_check_at: Some(now - Duration::hours(1)),
            ..UpdateCheckState::default()
        };
        assert!(!check_is_due(&recent, now));
        let old = UpdateCheckState {
            last_check_at: Some(now - Duration::hours(25)),
            ..UpdateCheckState::default()
        };
        assert!(check_is_due(&old, now));
    }

    #[test]
    fn updater_proxy_url_only_when_core_running() {
        let settings = AppSettings::default();
        assert!(updater_proxy_url(&settings, false).is_none());
        let url = updater_proxy_url(&settings, true).expect("loopback mixed");
        assert_eq!(url.as_str(), "http://127.0.0.1:17890/");
    }

    #[test]
    fn updater_proxy_url_uses_loopback_when_allow_lan_binds_all() {
        let settings = AppSettings {
            mixed_listen: "0.0.0.0".into(),
            mixed_port: 18080,
            allow_lan: true,
            ..AppSettings::default()
        };
        let url = updater_proxy_url(&settings, true).expect("lan mixed");
        assert_eq!(url.as_str(), "http://127.0.0.1:18080/");
    }

    #[test]
    fn available_version_round_trip_on_disk() {
        let paths = temp_paths("available");
        let state = UpdateCheckState {
            available_version: Some("0.1.6".into()),
            available_notes: Some("fixes".into()),
            ..UpdateCheckState::default()
        };
        save_update_check_state(&paths.update_check(), &state).unwrap();
        let loaded = load_update_check_state(&paths.update_check());
        assert_eq!(loaded.available_version.as_deref(), Some("0.1.6"));
        assert_eq!(loaded.available_notes.as_deref(), Some("fixes"));
        let _ = std::fs::remove_dir_all(paths.root());
    }

    #[test]
    fn only_check_app_updates_changed_ignores_other_fields() {
        let off = AppSettings {
            check_app_updates: false,
            ..AppSettings::default()
        };
        let on = AppSettings {
            check_app_updates: true,
            ..AppSettings::default()
        };
        assert!(only_check_app_updates_changed(&off, &on));
        let mut other = on.clone();
        other.mixed_port = 18080;
        assert!(!only_check_app_updates_changed(&off, &other));
    }

    #[test]
    fn release_not_found_is_feed_unavailable() {
        assert_eq!(
            check_error_code(&tauri_plugin_updater::Error::ReleaseNotFound),
            ERR_UPDATE_FEED_UNAVAILABLE
        );
        assert_eq!(
            check_error_code(&tauri_plugin_updater::Error::EmptyEndpoints),
            ERR_UPDATE_CHECK_FAILED
        );
    }
}
