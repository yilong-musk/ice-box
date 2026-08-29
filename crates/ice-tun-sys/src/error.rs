//! Stable TUN error codes (`tun.*`), following the dotted snake_case contract
//! of `ice_config::ErrorCode` (architecture §17). Codes are the stable IPC
//! identity; the message is for humans.

use std::fmt;

/// Stable error codes for the TUN capture subsystem (plan §4.5 / §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunErrorCode {
    /// Platform has no usable TUN backend (unsupported host).
    NotSupported,
    /// The operation needs privileges the app does not have (macOS root,
    /// Windows admin). Never retried automatically.
    PermissionRequired,
    /// Applying capture failed; nothing beyond the journal is claimed.
    ApplyFailed,
    /// Restoring owned capture resources failed.
    RestoreFailed,
    /// The adapter / route / DNS health checks did not agree.
    HealthcheckFailed,
    /// Cleanup could not be verified; capture is fail-closed until an
    /// explicit recovery attempt succeeds.
    RecoveryRequired,
}

impl TunErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotSupported => "tun.not_supported",
            Self::PermissionRequired => "tun.permission_required",
            Self::ApplyFailed => "tun.apply_failed",
            Self::RestoreFailed => "tun.restore_failed",
            Self::HealthcheckFailed => "tun.healthcheck_failed",
            Self::RecoveryRequired => "tun.recovery_required",
        }
    }
}

impl fmt::Display for TunErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{code}: {message}")]
pub struct TunError {
    pub code: TunErrorCode,
    pub message: String,
}

impl TunError {
    pub fn new(code: TunErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<serde_json::Error> for TunError {
    fn from(err: serde_json::Error) -> Self {
        Self::new(TunErrorCode::ApplyFailed, format!("journal json: {err}"))
    }
}

impl From<std::io::Error> for TunError {
    fn from(err: std::io::Error) -> Self {
        Self::new(TunErrorCode::ApplyFailed, format!("journal io: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tun_error_codes_are_stable_and_dotted() {
        assert_eq!(TunErrorCode::NotSupported.as_str(), "tun.not_supported");
        assert_eq!(
            TunErrorCode::PermissionRequired.as_str(),
            "tun.permission_required"
        );
        assert_eq!(TunErrorCode::ApplyFailed.as_str(), "tun.apply_failed");
        assert_eq!(TunErrorCode::RestoreFailed.as_str(), "tun.restore_failed");
        assert_eq!(
            TunErrorCode::HealthcheckFailed.as_str(),
            "tun.healthcheck_failed"
        );
        assert_eq!(
            TunErrorCode::RecoveryRequired.as_str(),
            "tun.recovery_required"
        );
    }
}
