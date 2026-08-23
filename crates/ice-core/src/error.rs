//! Core errors mapped to architecture §17 codes.

use ice_config::{AppError, ErrorCode};

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
            Self::Io(_) | Self::Other(_) => ErrorCode::CoreSpawnFailed,
        }
    }

    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::InvalidState(message.into())
    }
}

impl From<CoreError> for AppError {
    fn from(err: CoreError) -> Self {
        AppError::new(err.code(), err.to_string())
    }
}
