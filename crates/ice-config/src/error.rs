//! Unified IPC / UI error shape: `{ code, message }`.
//!
//! Codes follow architecture §17 (dotted snake_case segments).

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::ConfigError;

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
    #[serde(rename = "sub.fetch_failed")]
    SubFetchFailed,
    #[serde(rename = "sub.unknown_format")]
    SubUnknownFormat,
    #[serde(rename = "sub.parse_failed")]
    SubParseFailed,
    #[serde(rename = "sub.empty")]
    SubEmpty,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CoreNotFound => "core.not_found",
            Self::CoreSpawnFailed => "core.spawn_failed",
            Self::CoreHealthcheckFailed => "core.healthcheck_failed",
            Self::CoreInvalidState => "core.invalid_state",
            Self::ConfigEmptyOutbounds => "config.empty_outbounds",
            Self::ConfigInvalid => "config.invalid",
            Self::ProxyApplyFailed => "proxy.apply_failed",
            Self::ProxyApplyFailedCoreReloaded => "proxy.apply_failed_core_reloaded",
            Self::ProxyRestoreFailed => "proxy.restore_failed",
            Self::SubFetchFailed => "sub.fetch_failed",
            Self::SubUnknownFormat => "sub.unknown_format",
            Self::SubParseFailed => "sub.parse_failed",
            Self::SubEmpty => "sub.empty",
        }
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

    pub fn with_code(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

impl From<ConfigError> for AppError {
    fn from(err: ConfigError) -> Self {
        match &err {
            ConfigError::EmptyOutbounds => {
                AppError::new(ErrorCode::ConfigEmptyOutbounds, err.to_string())
            }
            ConfigError::TunUnavailable(reason) => {
                AppError::with_code("tun.not_supported", reason.clone())
            }
            ConfigError::TunInvalid(_)
            | ConfigError::Invalid(_)
            | ConfigError::RouteInvalid(_)
            | ConfigError::Json(_)
            | ConfigError::Io(_) => AppError::new(ErrorCode::ConfigInvalid, err.to_string()),
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
    fn config_error_maps_to_architecture_codes() {
        let empty: AppError = ConfigError::EmptyOutbounds.into();
        assert_eq!(empty.code, "config.empty_outbounds");

        let invalid: AppError = ConfigError::Invalid("missing inbounds").into();
        assert_eq!(invalid.code, "config.invalid");

        let unavailable: AppError = ConfigError::TunUnavailable("gate pending".into()).into();
        assert_eq!(unavailable.code, "tun.not_supported");
        assert_eq!(unavailable.message, "gate pending");

        let tun_invalid: AppError = ConfigError::TunInvalid("bad cidr".into()).into();
        assert_eq!(tun_invalid.code, "config.invalid");
    }
}
