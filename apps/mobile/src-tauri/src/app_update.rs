// SPDX-License-Identifier: GPL-3.0-or-later

//! Phone update check. The Tauri updater plugin does not support Android, so
//! this reads the same GitHub `latest.json` the desktop updater uses and
//! opens the arm64 APK in the system browser. The app does not install it.

use chrono::{DateTime, Duration, Utc};
use ice_config::{write_json_atomic, AppError, AppPaths, ErrorCode};
use ice_subscription::{DirectFetcher, HttpFetcher};
use serde::{Deserialize, Serialize};

pub const LATEST_JSON_URL: &str =
    "https://github.com/yilong-musk/ice-box/releases/latest/download/latest.json";

const CHECK_INTERVAL: Duration = Duration::hours(24);

const ERR_UPDATE_CHECK_FAILED: ErrorCode = ErrorCode::UpdateCheckFailed;
const ERR_UPDATE_FEED_UNAVAILABLE: ErrorCode = ErrorCode::UpdateFeedUnavailable;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct UpdateCheckState {
    last_check_at: Option<DateTime<Utc>>,
    #[serde(default)]
    available_version: Option<String>,
    #[serde(default)]
    available_notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LatestCatalog {
    version: String,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CheckAppUpdateRequest {
    #[serde(default)]
    pub background: bool,
    #[serde(default)]
    pub startup: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CheckAppUpdateResponse {
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub skipped: bool,
    pub should_prompt: bool,
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

pub fn normalize_version(version: &str) -> &str {
    let version = version.trim();
    version
        .strip_prefix('v')
        .or_else(|| version.strip_prefix('V'))
        .unwrap_or(version)
}

pub fn is_newer_version(available: &str, installed: &str) -> bool {
    match (
        parse_version_nums(normalize_version(available)),
        parse_version_nums(normalize_version(installed)),
    ) {
        (Some(available), Some(installed)) => available > installed,
        _ => false,
    }
}

fn parse_version_nums(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.split(['-', '+']).next().unwrap_or(version);
    if core.is_empty() || core.split('.').any(|part| part.is_empty()) {
        return None;
    }
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = match parts.next() {
        Some(part) => part.parse().ok()?,
        None => 0,
    };
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Release asset URL for a dotted version. Anything that is not `X.Y.Z`
/// returns `None`, so a catalog value cannot change the host or the path.
pub fn android_apk_url(version: &str) -> Option<String> {
    let version = normalize_version(version);
    let mut parts = version.split('.');
    let major = parts.next()?;
    let minor = parts.next()?;
    let patch = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if [major, minor, patch]
        .into_iter()
        .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    Some(format!(
        "https://github.com/yilong-musk/ice-box/releases/download/v{version}/ice-box_{version}_android_arm64.apk"
    ))
}

fn installed_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn load_state(paths: &AppPaths) -> UpdateCheckState {
    std::fs::read_to_string(paths.update_check())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_state(paths: &AppPaths, state: &UpdateCheckState) -> Result<(), AppError> {
    write_json_atomic(&paths.update_check(), state).map_err(|err| {
        AppError::with_code(
            ERR_UPDATE_CHECK_FAILED,
            format!("write update-check.json: {err}"),
        )
    })
}

fn cached_response(state: &UpdateCheckState, installed: &str) -> CheckAppUpdateResponse {
    match state.available_version.as_deref() {
        Some(version) if is_newer_version(version, installed) => CheckAppUpdateResponse {
            available: true,
            version: Some(normalize_version(version).to_string()),
            notes: state.available_notes.clone(),
            skipped: false,
            should_prompt: false,
        },
        _ => empty_response(),
    }
}

fn check_is_due(state: &UpdateCheckState, now: DateTime<Utc>, startup: bool) -> bool {
    if startup {
        return true;
    }
    match state.last_check_at {
        None => true,
        Some(at) => now >= at + CHECK_INTERVAL,
    }
}

fn map_catalog_error(err: impl std::fmt::Display) -> AppError {
    let text = err.to_string();
    if text.contains("HTTP 404") {
        AppError::with_code(ERR_UPDATE_FEED_UNAVAILABLE, "update catalog was not found")
    } else {
        AppError::with_code(ERR_UPDATE_CHECK_FAILED, "update check failed")
    }
}

/// Fetch `latest.json` directly (the same SSRF-checked client subscriptions
/// use) and compare it with this build. Background checks in debug builds
/// stay on the cached result and do not contact GitHub.
pub fn check_release(
    paths: &AppPaths,
    background: bool,
    startup: bool,
) -> Result<CheckAppUpdateResponse, AppError> {
    let mut disk = load_state(paths);
    let installed = installed_version();
    if cfg!(debug_assertions) && background {
        return Ok(cached_response(&disk, installed));
    }
    let now = Utc::now();
    if background && !check_is_due(&disk, now, startup) {
        return Ok(cached_response(&disk, installed));
    }

    let fetched = DirectFetcher
        .get(LATEST_JSON_URL, None, None)
        .map_err(map_catalog_error)?;
    let catalog: LatestCatalog = serde_json::from_str(&fetched.body).map_err(|_| {
        AppError::with_code(ERR_UPDATE_CHECK_FAILED, "update catalog was not valid")
    })?;
    let version = normalize_version(&catalog.version).to_string();
    let newer = is_newer_version(&version, installed);
    if background {
        disk.last_check_at = Some(now);
    }
    if newer {
        disk.available_version = Some(version.clone());
        disk.available_notes = catalog.notes.filter(|notes| !notes.is_empty());
    } else {
        disk.available_version = None;
        disk.available_notes = None;
    }
    save_state(paths, &disk)?;
    if !newer {
        return Ok(empty_response());
    }
    Ok(CheckAppUpdateResponse {
        available: true,
        version: Some(version),
        notes: disk.available_notes.clone(),
        skipped: false,
        should_prompt: false,
    })
}

pub fn record_check(paths: &AppPaths) -> Result<(), AppError> {
    let mut disk = load_state(paths);
    disk.last_check_at = Some(Utc::now());
    save_state(paths, &disk)
}

/// APK URL for the last catalog version that was newer than this build.
pub fn cached_apk_url(paths: &AppPaths) -> Result<String, AppError> {
    let disk = load_state(paths);
    let version = disk
        .available_version
        .as_deref()
        .ok_or_else(|| AppError::with_code(ERR_UPDATE_CHECK_FAILED, "no update is available"))?;
    if !is_newer_version(version, installed_version()) {
        return Err(AppError::with_code(
            ERR_UPDATE_CHECK_FAILED,
            "no update is available",
        ));
    }
    android_apk_url(version).ok_or_else(|| {
        AppError::with_code(ERR_UPDATE_CHECK_FAILED, "update version is not a release")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare_ignores_a_v_prefix_and_prerelease_suffix() {
        assert!(is_newer_version("v0.1.16", "0.1.15"));
        assert!(!is_newer_version("0.1.15", "0.1.15"));
        assert!(!is_newer_version("0.1.15-rc.1", "0.1.15"));
        assert!(!is_newer_version("not-a-version", "0.1.15"));
    }

    #[test]
    fn apk_url_is_the_arm64_release_asset() {
        assert_eq!(
            android_apk_url("v0.1.16").as_deref(),
            Some(
                "https://github.com/yilong-musk/ice-box/releases/download/v0.1.16/ice-box_0.1.16_android_arm64.apk"
            )
        );
        assert!(android_apk_url("0.1.16/../../evil").is_none());
        assert!(android_apk_url("0.1").is_none());
        assert!(android_apk_url("").is_none());
    }

    #[test]
    fn catalog_404_is_a_missing_feed() {
        let err = map_catalog_error("fetch failed: GET https://example.test: HTTP 404");
        assert!(err.is_code(ErrorCode::UpdateFeedUnavailable));
        assert!(!err.to_string().contains("example.test"));
    }
}
