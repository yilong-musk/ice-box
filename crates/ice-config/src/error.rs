// SPDX-License-Identifier: GPL-3.0-or-later

//! Map builder failures onto the shared [`AppError`] IPC shape.
//!
//! `ErrorCode` / `AppError` live in `ice-types` (no I/O). This module keeps
//! the `ConfigError` conversion next to the builder error type.

pub use ice_types::{AppError, ErrorCode};

use crate::ConfigError;

impl From<ConfigError> for AppError {
    fn from(err: ConfigError) -> Self {
        match &err {
            ConfigError::EmptyOutbounds => {
                AppError::new(ErrorCode::ConfigEmptyOutbounds, err.to_string())
            }
            ConfigError::TunUnavailable(reason) => {
                AppError::new(ErrorCode::TunNotSupported, reason.clone())
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
    fn config_error_maps_to_architecture_codes() {
        let empty: AppError = ConfigError::EmptyOutbounds.into();
        assert_eq!(empty.code, ErrorCode::ConfigEmptyOutbounds.as_str());

        let invalid: AppError = ConfigError::invalid("missing inbounds").into();
        assert_eq!(invalid.code, ErrorCode::ConfigInvalid.as_str());

        let unavailable: AppError = ConfigError::TunUnavailable("gate pending".into()).into();
        assert_eq!(unavailable.code, ErrorCode::TunNotSupported.as_str());
        assert_eq!(unavailable.message, "gate pending");

        let tun_invalid: AppError = ConfigError::TunInvalid("bad cidr".into()).into();
        assert_eq!(tun_invalid.code, ErrorCode::ConfigInvalid.as_str());
    }
}
