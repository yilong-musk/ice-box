// SPDX-License-Identifier: GPL-3.0-or-later

//! Explicit share payloads. The list API redacts subscription URLs; this path
//! returns the real URL or a portable sing-box document only when the user
//! asks to copy one. Callers must not log the returned text.

use std::fs;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::decode::maybe_decode_base64;
use crate::error::SubscriptionError;
use crate::store::{read_index, read_profile, SubscriptionPaths};
use crate::SubscriptionFormat;

/// Which share payload to build for one subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionShareKind {
    Url,
    Singbox,
}

/// Full subscription URL, or a pretty-printed sing-box document.
pub fn subscription_share_text(
    paths: &SubscriptionPaths,
    id: Uuid,
    kind: SubscriptionShareKind,
) -> Result<String, SubscriptionError> {
    let index = read_index(paths)?;
    let meta = index
        .items
        .iter()
        .find(|item| item.id == id)
        .ok_or(SubscriptionError::NotFound)?;
    match kind {
        SubscriptionShareKind::Url => {
            if meta.source == crate::SubscriptionSource::File {
                return Err(SubscriptionError::FetchFailed(
                    "file subscription has no remote URL".into(),
                ));
            }
            Ok(meta.url.clone())
        }
        SubscriptionShareKind::Singbox => {
            // A sing-box body still has remote rule-sets, endpoints, and DNS
            // outbounds that profile normalisation drops. Prefer that body so
            // the copy is the portable original, not the trimmed profile.
            if meta.format == SubscriptionFormat::SingBox {
                if let Some(text) = singbox_share_from_raw(paths, id)? {
                    return Ok(text);
                }
            }
            let profile = read_profile(paths, id)?;
            ice_config::share_singbox_config(&profile).map_err(map_share_error)
        }
    }
}

fn singbox_share_from_raw(
    paths: &SubscriptionPaths,
    id: Uuid,
) -> Result<Option<String>, SubscriptionError> {
    let path = paths.raw(id);
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)?;
    let decoded = maybe_decode_base64(&raw)?;
    let mut document = match serde_json::from_str::<serde_json::Value>(decoded.as_ref()) {
        Ok(value) if value.is_object() => value,
        Ok(_) => return Ok(None),
        Err(_) => return Ok(None),
    };
    crate::userinfo::strip_provider_info_singbox(&mut document);
    let stripped = serde_json::to_string(&document)
        .map_err(|err| SubscriptionError::ParseFailed(format!("sing-box json: {err}")))?;
    match ice_config::share_singbox_document(&stripped) {
        Ok(text) => Ok(Some(text)),
        Err(ice_config::ConfigError::EmptyOutbounds) => Err(SubscriptionError::EmptyNodes),
        Err(ice_config::ConfigError::Invalid(message))
            if message.starts_with("sing-box json:") || message.contains("must be an object") =>
        {
            Ok(None)
        }
        Err(other) => Err(SubscriptionError::ParseFailed(other.to_string())),
    }
}

fn map_share_error(err: ice_config::ConfigError) -> SubscriptionError {
    match err {
        ice_config::ConfigError::EmptyOutbounds => SubscriptionError::EmptyNodes,
        other => SubscriptionError::ParseFailed(other.to_string()),
    }
}
