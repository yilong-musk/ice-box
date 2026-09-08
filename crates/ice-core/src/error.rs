// SPDX-License-Identifier: GPL-3.0-or-later

//! Core errors mapped to architecture §17 codes.

use ice_types::{AppError, ErrorCode, UiMessage};

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    SpawnFailed(String),
    #[error("{0}")]
    HealthcheckFailed(String),
    #[error("{0}")]
    InvalidState(String),
    #[error("refusing to adopt pid {0}: process is not the bundled sing-box")]
    AdoptRejected(u32),
    #[error("clash api {path}: {detail}")]
    ClashApi { path: String, detail: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl CoreError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::NotFound(_) => ErrorCode::CoreNotFound,
            Self::SpawnFailed(_) => ErrorCode::CoreSpawnFailed,
            Self::HealthcheckFailed(_) => ErrorCode::CoreHealthcheckFailed,
            Self::InvalidState(_) => ErrorCode::CoreInvalidState,
            Self::AdoptRejected(_) => ErrorCode::CoreAdoptRejected,
            Self::ClashApi { .. } => ErrorCode::CoreApiFailed,
            Self::Io(_) | Self::Other(_) => ErrorCode::CoreSpawnFailed,
        }
    }

    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::InvalidState(message.into())
    }

    /// Structured UI copy for `CoreState.message` (FE-5).
    pub fn ui_message(&self) -> UiMessage {
        self.code().ui_message_detail(self.to_string())
    }
}

impl From<CoreError> for AppError {
    fn from(err: CoreError) -> Self {
        AppError::new(err.code(), err.to_string())
    }
}
