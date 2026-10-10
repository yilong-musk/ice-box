// SPDX-License-Identifier: GPL-3.0-or-later

//! Explicit share payloads. The list API redacts subscription URLs; this path
//! returns the real URL or a portable sing-box document only when the user
//! asks to copy one. Callers must not log the returned text.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::SubscriptionError;
use crate::store::{read_index, read_profile, SubscriptionPaths};

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
        SubscriptionShareKind::Url => Ok(meta.url.clone()),
        SubscriptionShareKind::Singbox => {
            let profile = read_profile(paths, id)?;
            ice_config::share_singbox_config(&profile).map_err(map_share_error)
        }
    }
}

fn map_share_error(err: ice_config::ConfigError) -> SubscriptionError {
    match err {
        ice_config::ConfigError::EmptyOutbounds => SubscriptionError::EmptyNodes,
        other => SubscriptionError::ParseFailed(other.to_string()),
    }
}
