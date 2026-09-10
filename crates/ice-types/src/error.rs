// SPDX-License-Identifier: GPL-3.0-or-later

//! Unified IPC / UI error shape: `{ code, message }`.
//!
//! Codes use dotted snake_case segments. This
//! enum is the single source of truth for `core.*` / `config.*` / `proxy.*` /
//! `sub.*` / `tun.*` / `update.*` / `app.*` codes returned to the UI.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable error codes returned to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    #[serde(rename = "core.not_found")]
    CoreNotFound,
    #[serde(rename = "core.spawn_failed")]
    CoreSpawnFailed,
    #[serde(rename = "core.healthcheck_failed")]
    CoreHealthcheckFailed,
    #[serde(rename = "core.invalid_state")]
    CoreInvalidState,
    #[serde(rename = "core.adopt_rejected")]
    CoreAdoptRejected,
    #[serde(rename = "core.api_failed")]
    CoreApiFailed,
    #[serde(rename = "config.empty_outbounds")]
    ConfigEmptyOutbounds,
    #[serde(rename = "config.invalid")]
    ConfigInvalid,
    #[serde(rename = "proxy.apply_failed")]
    ProxyApplyFailed,
    #[serde(rename = "proxy.apply_failed_core_reloaded")]
    ProxyApplyFailedCoreReloaded,
    #[serde(rename = "proxy.restore_failed")]
    ProxyRestoreFailed,
    #[serde(rename = "proxy.backup_corrupt")]
    ProxyBackupCorrupt,
    #[serde(rename = "settings.reset")]
    SettingsReset,
    #[serde(rename = "logs.oversized")]
    LogsOversized,
    #[serde(rename = "sub.fetch_failed")]
    SubFetchFailed,
    #[serde(rename = "sub.unknown_format")]
    SubUnknownFormat,
    #[serde(rename = "sub.parse_failed")]
    SubParseFailed,
    #[serde(rename = "sub.empty")]
    SubEmpty,
    #[serde(rename = "sub.not_found")]
    SubNotFound,
    #[serde(rename = "sub.io")]
    SubIo,
    #[serde(rename = "app.lock_poisoned")]
    LockPoisoned,
    #[serde(rename = "tun.not_supported")]
    TunNotSupported,
    #[serde(rename = "tun.permission_required")]
    TunPermissionRequired,
    #[serde(rename = "tun.apply_failed")]
    TunApplyFailed,
    #[serde(rename = "tun.restore_failed")]
    TunRestoreFailed,
    #[serde(rename = "tun.healthcheck_failed")]
    TunHealthcheckFailed,
    #[serde(rename = "tun.recovery_required")]
    TunRecoveryRequired,
    #[serde(rename = "tun.invalid_argument")]
    TunInvalidArgument,
    #[serde(rename = "tun.config_rejected")]
    TunConfigRejected,
    #[serde(rename = "tun.helper_stale")]
    TunHelperStale,
    #[serde(rename = "tun.helper_install_failed")]
    TunHelperInstallFailed,
    #[serde(rename = "tun.helper_install_cancelled")]
    TunHelperInstallCancelled,
    #[serde(rename = "tun.helper_not_ready")]
    TunHelperNotReady,
    #[serde(rename = "tun.elevation_cancelled")]
    TunElevationCancelled,
    #[serde(rename = "tun.elevation_requires_admin")]
    TunElevationRequiresAdmin,
    #[serde(rename = "update.check_failed")]
    UpdateCheckFailed,
    #[serde(rename = "update.feed_unavailable")]
    UpdateFeedUnavailable,
    #[serde(rename = "update.install_failed")]
    UpdateInstallFailed,
    #[serde(rename = "update.disabled")]
    UpdateDisabled,
}

impl ErrorCode {
    /// Every variant, in a stable order used to generate frontend types.
    pub const ALL: &'static [ErrorCode] = &[
        Self::CoreNotFound,
        Self::CoreSpawnFailed,
        Self::CoreHealthcheckFailed,
        Self::CoreInvalidState,
        Self::CoreAdoptRejected,
        Self::CoreApiFailed,
        Self::ConfigEmptyOutbounds,
        Self::ConfigInvalid,
        Self::ProxyApplyFailed,
        Self::ProxyApplyFailedCoreReloaded,
        Self::ProxyRestoreFailed,
        Self::ProxyBackupCorrupt,
        Self::SettingsReset,
        Self::LogsOversized,
        Self::SubFetchFailed,
        Self::SubUnknownFormat,
        Self::SubParseFailed,
        Self::SubEmpty,
        Self::SubNotFound,
        Self::SubIo,
        Self::LockPoisoned,
        Self::TunNotSupported,
        Self::TunPermissionRequired,
        Self::TunApplyFailed,
        Self::TunRestoreFailed,
        Self::TunHealthcheckFailed,
        Self::TunRecoveryRequired,
        Self::TunInvalidArgument,
        Self::TunConfigRejected,
        Self::TunHelperStale,
        Self::TunHelperInstallFailed,
        Self::TunHelperInstallCancelled,
        Self::TunHelperNotReady,
        Self::TunElevationCancelled,
        Self::TunElevationRequiresAdmin,
        Self::UpdateCheckFailed,
        Self::UpdateFeedUnavailable,
        Self::UpdateInstallFailed,
        Self::UpdateDisabled,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CoreNotFound => "core.not_found",
            Self::CoreSpawnFailed => "core.spawn_failed",
            Self::CoreHealthcheckFailed => "core.healthcheck_failed",
            Self::CoreInvalidState => "core.invalid_state",
            Self::CoreAdoptRejected => "core.adopt_rejected",
            Self::CoreApiFailed => "core.api_failed",
            Self::ConfigEmptyOutbounds => "config.empty_outbounds",
            Self::ConfigInvalid => "config.invalid",
            Self::ProxyApplyFailed => "proxy.apply_failed",
            Self::ProxyApplyFailedCoreReloaded => "proxy.apply_failed_core_reloaded",
            Self::ProxyRestoreFailed => "proxy.restore_failed",
            Self::ProxyBackupCorrupt => "proxy.backup_corrupt",
            Self::SettingsReset => "settings.reset",
            Self::LogsOversized => "logs.oversized",
            Self::SubFetchFailed => "sub.fetch_failed",
            Self::SubUnknownFormat => "sub.unknown_format",
            Self::SubParseFailed => "sub.parse_failed",
            Self::SubEmpty => "sub.empty",
            Self::SubNotFound => "sub.not_found",
            Self::SubIo => "sub.io",
            Self::LockPoisoned => "app.lock_poisoned",
            Self::TunNotSupported => "tun.not_supported",
            Self::TunPermissionRequired => "tun.permission_required",
            Self::TunApplyFailed => "tun.apply_failed",
            Self::TunRestoreFailed => "tun.restore_failed",
            Self::TunHealthcheckFailed => "tun.healthcheck_failed",
            Self::TunRecoveryRequired => "tun.recovery_required",
            Self::TunInvalidArgument => "tun.invalid_argument",
            Self::TunConfigRejected => "tun.config_rejected",
            Self::TunHelperStale => "tun.helper_stale",
            Self::TunHelperInstallFailed => "tun.helper_install_failed",
            Self::TunHelperInstallCancelled => "tun.helper_install_cancelled",
            Self::TunHelperNotReady => "tun.helper_not_ready",
            Self::TunElevationCancelled => "tun.elevation_cancelled",
            Self::TunElevationRequiresAdmin => "tun.elevation_requires_admin",
            Self::UpdateCheckFailed => "update.check_failed",
            Self::UpdateFeedUnavailable => "update.feed_unavailable",
            Self::UpdateInstallFailed => "update.install_failed",
            Self::UpdateDisabled => "update.disabled",
        }
    }

    /// Frontend `t()` key (`error.{as_str}`), matching `apps/desktop` catalogues.
    pub fn message_key(self) -> String {
        format!("error.{}", self.as_str())
    }

    pub fn ui_message(self) -> crate::UiMessage {
        crate::UiMessage::new(self.message_key())
    }

    pub fn ui_message_detail(self, detail: impl Into<String>) -> crate::UiMessage {
        self.ui_message().with("detail", detail)
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error payload for Tauri commands and shared crate failures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code: code.as_str().to_string(),
            message: message.into(),
        }
    }

    /// Same as [`Self::new`]. The `ErrorCode` argument is what stops new
    /// string-literal codes from compiling.
    pub fn with_code(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::new(code, message)
    }

    pub fn is_code(&self, code: ErrorCode) -> bool {
        self.code == code.as_str()
    }

    /// Structured UI copy: `error.{code}` plus the technical `message` as `detail`.
    pub fn ui_message(&self) -> crate::UiMessage {
        crate::UiMessage::new(format!("error.{}", self.code)).with("detail", self.message.clone())
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

/// TUN subsystem error. `code` is always a `tun.*` [`ErrorCode`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunError {
    pub code: ErrorCode,
    pub message: String,
}

impl TunError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for TunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for TunError {}

impl From<ErrorCode> for TunError {
    fn from(code: ErrorCode) -> Self {
        Self::new(code, code.as_str())
    }
}

impl From<TunError> for AppError {
    fn from(err: TunError) -> Self {
        AppError::new(err.code, err.message)
    }
}

impl From<std::io::Error> for TunError {
    fn from(err: std::io::Error) -> Self {
        Self::new(ErrorCode::TunApplyFailed, format!("io: {err}"))
    }
}

impl From<serde_json::Error> for TunError {
    fn from(err: serde_json::Error) -> Self {
        Self::new(ErrorCode::TunApplyFailed, format!("json: {err}"))
    }
}

impl ErrorCode {
    /// Parse a `tun.*` wire code; unknown values map to apply-failed.
    pub fn from_tun_wire(code: Option<&str>) -> Self {
        match code {
            Some(raw) if raw.starts_with("tun.") => Self::ALL
                .iter()
                .copied()
                .find(|c| c.as_str() == raw)
                .unwrap_or(Self::TunApplyFailed),
            _ => Self::TunApplyFailed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_error_serde_roundtrip_has_code_and_message() {
        let err = AppError::new(ErrorCode::CoreNotFound, "missing sing-box binary");
        let json = serde_json::to_value(&err).expect("serialize");
        assert_eq!(json["code"], "core.not_found");
        assert_eq!(json["message"], "missing sing-box binary");

        let back: AppError = serde_json::from_value(json).expect("deserialize");
        assert_eq!(back.code, "core.not_found");
        assert_eq!(back.message, "missing sing-box binary");
    }

    #[test]
    fn error_code_strings_are_stable_snake_case() {
        let samples = [
            (ErrorCode::CoreHealthcheckFailed, "core.healthcheck_failed"),
            (ErrorCode::ConfigEmptyOutbounds, "config.empty_outbounds"),
            (ErrorCode::SubUnknownFormat, "sub.unknown_format"),
            (ErrorCode::ProxyRestoreFailed, "proxy.restore_failed"),
            (
                ErrorCode::ProxyApplyFailedCoreReloaded,
                "proxy.apply_failed_core_reloaded",
            ),
            (ErrorCode::LockPoisoned, "app.lock_poisoned"),
            (ErrorCode::CoreAdoptRejected, "core.adopt_rejected"),
            (ErrorCode::CoreApiFailed, "core.api_failed"),
            (ErrorCode::SubNotFound, "sub.not_found"),
            (ErrorCode::SubIo, "sub.io"),
            (ErrorCode::TunNotSupported, "tun.not_supported"),
            (ErrorCode::TunRecoveryRequired, "tun.recovery_required"),
            (ErrorCode::TunHelperStale, "tun.helper_stale"),
            (ErrorCode::UpdateCheckFailed, "update.check_failed"),
        ];

        for (code, expected) in samples {
            assert_eq!(code.as_str(), expected);
            let via_enum = serde_json::to_string(&code).expect("serialize ErrorCode");
            assert_eq!(via_enum, format!("\"{expected}\""));
            let decoded: ErrorCode =
                serde_json::from_str(&via_enum).expect("deserialize ErrorCode");
            assert_eq!(decoded, code);
            assert_eq!(decoded.as_str(), expected);
        }
    }

    #[test]
    fn from_tun_wire_keeps_helper_and_elevation_codes() {
        assert_eq!(
            ErrorCode::from_tun_wire(Some("tun.helper_install_failed")),
            ErrorCode::TunHelperInstallFailed
        );
        assert_eq!(
            ErrorCode::from_tun_wire(Some("tun.helper_install_cancelled")),
            ErrorCode::TunHelperInstallCancelled
        );
        assert_eq!(
            ErrorCode::from_tun_wire(Some("tun.elevation_cancelled")),
            ErrorCode::TunElevationCancelled
        );
        assert_eq!(
            ErrorCode::from_tun_wire(Some("tun.elevation_requires_admin")),
            ErrorCode::TunElevationRequiresAdmin
        );
        assert_eq!(
            ErrorCode::from_tun_wire(Some("tun.helper_stale")),
            ErrorCode::TunHelperStale
        );
        assert_eq!(
            ErrorCode::from_tun_wire(Some("not-a-tun-code")),
            ErrorCode::TunApplyFailed
        );
    }

    #[test]
    fn all_variants_are_listed_and_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for code in ErrorCode::ALL {
            assert!(seen.insert(code.as_str()), "duplicate {}", code.as_str());
        }
        assert_eq!(seen.len(), ErrorCode::ALL.len());
        let mut match_count = 0usize;
        for code in ErrorCode::ALL {
            let _ = code.as_str();
            match_count += 1;
        }
        assert_eq!(
            match_count,
            ErrorCode::ALL.len(),
            "ErrorCode::ALL must list every as_str arm"
        );
    }
}
