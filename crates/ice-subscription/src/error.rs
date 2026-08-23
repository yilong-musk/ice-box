//! Subscription errors mapped to architecture §17.

use ice_config::{AppError, ErrorCode};

#[derive(Debug, thiserror::Error)]
pub enum SubscriptionError {
    #[error("unknown subscription format")]
    UnknownFormat,
    #[error("invalid sing-box subscription: {0}")]
    InvalidSingBox(&'static str),
    #[error("subscription contains no usable nodes")]
    EmptyNodes,
    #[error("fetch failed: {0}")]
    FetchFailed(String),
    #[error("parse failed: {0}")]
    ParseFailed(String),
    #[error("no active subscription")]
    NoActiveSubscription,
    #[error("profile parse failed: {0}")]
    ProfileParseFailed(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl SubscriptionError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::UnknownFormat => ErrorCode::SubUnknownFormat,
            Self::EmptyNodes => ErrorCode::SubEmpty,
            Self::FetchFailed(_) => ErrorCode::SubFetchFailed,
            Self::InvalidSingBox(_) | Self::ParseFailed(_) | Self::Json(_) => {
                ErrorCode::SubParseFailed
            }
            Self::Io(_) | Self::NoActiveSubscription => ErrorCode::SubFetchFailed,
            Self::ProfileParseFailed(_) => ErrorCode::SubParseFailed,
        }
    }
}

impl From<SubscriptionError> for AppError {
    fn from(err: SubscriptionError) -> Self {
        AppError::new(err.code(), err.to_string())
    }
}
