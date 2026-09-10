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
    TunSettings, TunSettingsPatch, EXAMPLE_CLASH_API_SECRET, TUN_DEFAULT_IPV4_ADDRESS,
    TUN_DEFAULT_IPV6_ADDRESS, TUN_DEFAULT_MTU, TUN_DEFAULT_STACK,
};
pub use ui::{UiMessage, UI_RAW_KEY};

/// sing-box core version the config generator targets.
///
/// Re-exported by `ice-engine` as the public pin. Bundled desktop binaries
/// (`third_party/sing-box/VERSION`) must match; generated config features
/// are only tested against this version range.
pub const ENGINE_COMPAT_CORE_VERSION: &str = "1.13.19";
