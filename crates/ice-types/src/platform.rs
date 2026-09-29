// SPDX-License-Identifier: GPL-3.0-or-later

//! Host OS as a value, so config/subscription crates stay free of
//! `cfg(target_os)`. The shell / `ice-engine` maps the compile-time target
//! onto this enum.

use serde::{Deserialize, Serialize};

/// Operating system the generated config and subscription DNS shape target.
///
/// Not inferred inside `ice-config` / `ice-subscription`; callers pass it on
/// [`crate::HostPlatform`] fields such as `BuildInput.platform`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HostPlatform {
    #[default]
    #[serde(rename = "macos")]
    MacOs,
    Windows,
    Linux,
    Android,
    Ios,
}

impl HostPlatform {
    pub fn is_windows(self) -> bool {
        matches!(self, Self::Windows)
    }

    pub fn is_macos(self) -> bool {
        matches!(self, Self::MacOs)
    }

    /// Desktop TUN (macOS and Windows) and the mobile VPN (Android and iOS).
    /// Linux has no TUN backend.
    pub fn tun_ready(self) -> bool {
        matches!(
            self,
            Self::MacOs | Self::Windows | Self::Android | Self::Ios
        )
    }

    /// Android and iOS always generate the mobile TUN shape.
    pub fn is_mobile(self) -> bool {
        matches!(self, Self::Android | Self::Ios)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_uses_stable_names() {
        assert_eq!(
            serde_json::to_string(&HostPlatform::MacOs).unwrap(),
            "\"macos\""
        );
        assert_eq!(
            serde_json::to_string(&HostPlatform::Windows).unwrap(),
            "\"windows\""
        );
        let back: HostPlatform = serde_json::from_str("\"macos\"").unwrap();
        assert_eq!(back, HostPlatform::MacOs);
    }

    #[test]
    fn tun_ready_covers_desktop_tun_and_mobile() {
        assert!(HostPlatform::MacOs.tun_ready());
        assert!(HostPlatform::Windows.tun_ready());
        assert!(HostPlatform::Android.tun_ready());
        assert!(HostPlatform::Ios.tun_ready());
        assert!(!HostPlatform::Linux.tun_ready());
        assert!(HostPlatform::Android.is_mobile());
        assert!(HostPlatform::Ios.is_mobile());
        assert!(!HostPlatform::MacOs.is_mobile());
        assert!(!HostPlatform::Linux.is_mobile());
    }
}
