// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared DTOs with no `cfg(target_os)`.
//!
//! `AppPaths` is path layout only (`ensure_dirs` is std `create_dir_all`).
//! Listen/SSRF helpers and `AppSettings` (no I/O) are pure. Load/save of
//! `settings.json` stays in `ice-config`. Pid-file and log rotation live in
//! `ice-core`; tracing init lives in the desktop shell.

mod error;
mod listen;
mod paths;
mod platform;
mod settings;
mod ui;

pub use error::{AppError, ErrorCode, TunError};
pub use listen::{is_fake_ip, is_loopback_host, is_restricted_fetch_host, is_restricted_ip};
pub use paths::AppPaths;
pub use platform::HostPlatform;
pub use settings::{
    clash_mode_name, default_auto_set_system_proxy, is_plausible_clash_api_secret,
    tun_interface_name_valid, AppSettings, LanguagePreference, ProxyMode, SettingsPatch,
    TrayDisplayMode, TunSettings, TunSettingsPatch, EXAMPLE_CLASH_API_SECRET,
    TUN_DEFAULT_IPV4_ADDRESS, TUN_DEFAULT_IPV6_ADDRESS, TUN_DEFAULT_MTU, TUN_DEFAULT_STACK,
};
pub use ui::{UiMessage, UI_RAW_KEY};

/// sing-box core version the config generator targets.
///
/// This is `third_party/sing-box/VERSION`, the same pin desktop downloads and
/// `scripts/build-libbox.sh` clones. Following a sing-box stable release means
/// updating that file and `third_party/sing-box/CHECKSUMS.sha256`, then
/// reviewing generated config against the new schema. The libbox build tracks
/// the tag on its own; its feature-tag allowlist changes only when config
/// generation starts emitting a protocol the new core added.
pub const ENGINE_COMPAT_CORE_VERSION: &str =
    strip_trailing_newline(include_str!("../../../third_party/sing-box/VERSION"));

/// Drop one trailing newline. `str::trim` is not const on this toolchain.
const fn strip_trailing_newline(raw: &str) -> &str {
    let bytes = raw.as_bytes();
    let mut end = bytes.len();
    if end > 0 && bytes[end - 1] == b'\n' {
        end -= 1;
    }
    if end > 0 && bytes[end - 1] == b'\r' {
        end -= 1;
    }
    let (head, _) = bytes.split_at(end);
    // `end` stops on ASCII newline bytes, so `head` stays valid UTF-8.
    unsafe { core::str::from_utf8_unchecked(head) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compat_core_version_matches_vendor_pin() {
        let pinned = include_str!("../../../third_party/sing-box/VERSION").trim();
        assert_eq!(ENGINE_COMPAT_CORE_VERSION, pinned);
        assert!(!pinned.is_empty());
    }
}
