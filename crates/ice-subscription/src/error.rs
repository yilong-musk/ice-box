// SPDX-License-Identifier: GPL-3.0-or-later

//! Subscription errors mapped to architecture §17.

use ice_config::{AppError, ErrorCode, UiMessage};

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
            Self::Io(_) => ErrorCode::SubIo,
            Self::NoActiveSubscription => ErrorCode::SubNotFound,
            Self::ProfileParseFailed(_) => ErrorCode::SubParseFailed,
        }
    }

    /// Display text with any embedded subscription URLs redacted, safe for logs.
    pub fn redacted_display(&self) -> String {
        crate::url::redact_urls_in_text(&self.to_string())
    }

    /// Structured UI copy for `last_error` (FE-5). Detail stays in `params`.
    pub fn ui_message(&self) -> UiMessage {
        let key = match self {
            Self::UnknownFormat => "error.sub.unknown_format",
            Self::EmptyNodes => "error.sub.empty",
            Self::FetchFailed(_) => "error.sub.fetch_failed",
            Self::InvalidSingBox(_) | Self::ParseFailed(_) | Self::Json(_) => {
                "error.sub.parse_failed"
            }
            Self::Io(_) => "error.sub.io",
            Self::NoActiveSubscription => "error.sub.not_found",
            Self::ProfileParseFailed(_) => "error.sub.parse_failed",
        };
        let detail = self.redacted_display();
        UiMessage::new(key).with("detail", detail)
    }
}

impl From<SubscriptionError> for AppError {
    fn from(err: SubscriptionError) -> Self {
        AppError::new(err.code(), err.to_string())
    }
}
