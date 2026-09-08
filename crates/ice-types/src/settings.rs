// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings DTOs with no I/O (architecture review ARCH-1).
//!
//! Load/save lives in `ice-config`. Validation is pure.

use serde::{Deserialize, Serialize};

use crate::listen::is_loopback_host;
use crate::platform::HostPlatform;
use crate::{AppError, ErrorCode};

/// Routing mode: rule-based routing, all traffic through the selected proxy, or all direct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    /// Route by the active subscription's `route.rules` (default).
    #[default]
    Rule,
    /// Ignore rules; send all traffic through the selected proxy / strategy group.
    Global,
    /// Ignore rules; send all traffic out `direct`.
    Direct,
}

/// UI language preference. `System` follows the OS locale; the frontend
/// resolves it to the closest supported language (`zh` / `en`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LanguagePreference {
    /// Follow the system locale (default).
    #[default]
    System,
    /// Simplified Chinese.
    Zh,
    /// English.
    En,
}

/// Capitalized Clash runtime mode, matching sing-box's case-sensitive `mode-list`
/// membership checks (the pinned 1.13.19 does not accept an emitted `mode_list`).
///
/// sing-box `experimental/clashapi` `NewServer` starts with an empty `mode-list` and
/// prepends `default_mode` when it is missing, so the runtime list is `[<default_mode>]`
/// — a single entry, not `["Rule", "Global", "Direct"]`. `SetMode` checks membership
/// case-sensitively, so a lowercase `"global"` would be silently ignored (and, were the
/// entry present, pollute `GET /configs` `mode-list` with a mixed-case duplicate). The
/// `clash_mode` route rule matches case-insensitively, so routing behaves the same either
/// way; the capitalized form keeps the reported `mode` / `mode-list` clean.
pub fn clash_mode_name(mode: ProxyMode) -> &'static str {
    match mode {
        ProxyMode::Rule => "Rule",
        ProxyMode::Global => "Global",
        ProxyMode::Direct => "Direct",
    }
}

/// Legacy default for `auto_set_system_proxy` in `settings.json`.
///
/// Product: the core follows the app; system proxy is toggled from the home page.
/// Start never applies the OS proxy from this flag. Kept for serde compatibility.
pub const fn default_auto_set_system_proxy() -> bool {
    false
}

/// Locked default TUN adapter IPv4 address (CIDR). Verified live in the T0 spike.
pub const TUN_DEFAULT_IPV4_ADDRESS: &str = "10.0.0.1/30";
/// Locked default TUN adapter IPv6 address (CIDR, ULA).
///
/// Required, not optional (architecture §24.5 point 4): an IPv4-only tun installs no
/// IPv6 routes and silently leaks IPv6. The ULA gateway sits inside the excluded
/// `fc00::/7`, so the adapter stays reachable.
pub const TUN_DEFAULT_IPV6_ADDRESS: &str = "fdfe:dcba:9876::1/126";
/// Locked default MTU (verified live at 9000 in the T0 spike).
pub const TUN_DEFAULT_MTU: u16 = 9000;
/// Locked default stack (first-release default per the T0 spike).
pub const TUN_DEFAULT_STACK: &str = "gvisor";

/// Locked default for `TunSettings::auto_route` (capture all sub-ranges).
pub const fn default_tun_auto_route() -> bool {
    true
}

/// Locked default for `TunSettings::strict_route`.
pub const fn default_tun_strict_route() -> bool {
    true
}

pub fn default_tun_ipv4_address() -> String {
    TUN_DEFAULT_IPV4_ADDRESS.into()
}

pub fn default_tun_ipv6_address() -> String {
    TUN_DEFAULT_IPV6_ADDRESS.into()
}

pub fn default_tun_mtu() -> u16 {
    TUN_DEFAULT_MTU
}

pub fn default_tun_stack() -> String {
    TUN_DEFAULT_STACK.into()
}

/// Validated TUN capture parameters (plan §4.1; defaults locked by the T0 spike).
///
/// Only `enabled` is a user-facing switch. The remaining fields are validated
/// implementation parameters with locked defaults; they are not additional capture
/// modes and are not exposed as free-form UI inputs in the first release. Existing
/// `settings.json` files load unchanged — missing TUN fields mean disabled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TunSettings {
    /// Desired capture backend for the next proxy-service start (plan §2).
    /// This is a *desired* value: the active backend is owned by the runtime
    /// controller and reported separately in status.
    #[serde(default)]
    pub enabled: bool,
    /// Adapter interface name. Optional in settings: the platform backend /
    /// helper may resolve a free name at apply time. When present it must pass
    /// platform validation (macOS requires a `utun<N>` numeric suffix).
    #[serde(default)]
    pub interface_name: Option<String>,
    /// Adapter IPv4 address as CIDR (e.g. `10.0.0.1/30`), never a bare host.
    #[serde(default = "default_tun_ipv4_address")]
    pub ipv4_address: String,
    /// Adapter IPv6 address as CIDR. **Required** (dual-stack lock §24.5.4):
    /// an IPv4-only tun silently leaks IPv6.
    #[serde(default = "default_tun_ipv6_address")]
    pub ipv6_address: String,
    #[serde(default = "default_tun_mtu")]
    pub mtu: u16,
    #[serde(default = "default_tun_auto_route")]
    pub auto_route: bool,
    #[serde(default = "default_tun_strict_route")]
    pub strict_route: bool,
    /// Stack name, one of `gvisor` / `system` / `mixed` (locked by the spike).
    #[serde(default = "default_tun_stack")]
    pub stack: String,
    /// Route DNS through the sing-box DNS engine in TUN mode: the generated
    /// config prepends a `hijack-dns` route rule (port-53 traffic is answered
    /// by the subscription's resolvers instead of a GFW-poisoned system
    /// resolver), and on macOS the backend additionally points the primary
    /// service's DNS at public resolvers so queries on the LAN enter the TUN
    /// (a connected-subnet resolver would bypass it). On by default.
    #[serde(default = "default_tun_dns_hijack")]
    pub dns_hijack: bool,
}

fn default_tun_dns_hijack() -> bool {
    true
}

impl Default for TunSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            interface_name: None,
            ipv4_address: TUN_DEFAULT_IPV4_ADDRESS.into(),
            ipv6_address: TUN_DEFAULT_IPV6_ADDRESS.into(),
            mtu: TUN_DEFAULT_MTU,
            auto_route: true,
            strict_route: true,
            stack: TUN_DEFAULT_STACK.into(),
            dns_hijack: true,
        }
    }
}

impl TunSettings {
    /// Validate addresses, prefixes, MTU, stack, and interface name without
    /// mutating or writing disk (plan §4.1). Platform-exact interface rules
    /// (e.g. macOS `utun<N>`) are enforced for [`HostPlatform::MacOs`]; further
    /// host checks belong to the platform backend (`ice-tun-sys`, T2).
    pub fn validate(&self) -> Result<(), AppError> {
        self.validate_for(HostPlatform::Linux)
    }

    pub fn validate_for(&self, platform: HostPlatform) -> Result<(), AppError> {
        validate_cidr("tun.ipv4_address", &self.ipv4_address, false)?;
        validate_cidr("tun.ipv6_address", &self.ipv6_address, true)?;
        if !(1280..=TUN_DEFAULT_MTU).contains(&self.mtu) {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                format!(
                    "tun.mtu must be in 1280..={TUN_DEFAULT_MTU}, got {}",
                    self.mtu
                ),
            ));
        }
        if !matches!(self.stack.as_str(), "gvisor" | "system" | "mixed") {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                format!(
                    "tun.stack must be one of gvisor/system/mixed, got {}",
                    self.stack
                ),
            ));
        }
        if let Some(name) = &self.interface_name {
            if !tun_interface_name_valid(name, platform.is_macos()) {
                return Err(AppError::new(
                    ErrorCode::ConfigInvalid,
                    format!("tun.interface_name is invalid: {name}"),
                ));
            }
        }
        Ok(())
    }
}

/// A CIDR is `address/prefix`; the address must parse for the expected family
/// (v4 or v6) and the prefix must be in `1..=32` (IPv4) / `1..=128` (IPv6).
/// A `/0` interface address is rejected: an adapter cannot own an entire
/// address family.
fn validate_cidr(field: &str, cidr: &str, ipv6: bool) -> Result<(), AppError> {
    let (addr, prefix) = cidr.split_once('/').ok_or_else(|| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("{field} must be a CIDR (address/prefix), got {cidr}"),
        )
    })?;
    let prefix: u32 = prefix.parse().map_err(|_| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("{field} has a non-numeric prefix: {cidr}"),
        )
    })?;
    let parsed: Result<(), _> = if ipv6 {
        addr.parse::<std::net::Ipv6Addr>().map(|_| ())
    } else {
        addr.parse::<std::net::Ipv4Addr>().map(|_| ())
    };
    parsed.map_err(|_| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!(
                "{field} has an invalid {} address: {cidr}",
                if ipv6 { "IPv6" } else { "IPv4" }
            ),
        )
    })?;
    let max = if ipv6 { 128 } else { 32 };
    if prefix == 0 || prefix > max {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            format!("{field} prefix must be in 1..={max}, got {prefix}"),
        ));
    }
    Ok(())
}

/// Shared interface-name sanity rules plus per-platform locks.
///
/// macOS (locked by the T0 spike): sing-tun parses the name with
/// `fmt.Sscanf("utun%d")`, so a bare `utun` is FATAL and only `utun<N>` works.
pub fn tun_interface_name_valid(name: &str, macos: bool) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }
    if name
        .chars()
        .any(|c| c.is_whitespace() || c == '/' || c == '\\')
    {
        return false;
    }
    if macos {
        if !name.starts_with("utun") {
            return false;
        }
        let digits = &name[4..];
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
    }
    true
}

/// Application settings (not the sing-box runtime config).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSettings {
    pub mixed_listen: String,
    pub mixed_port: u16,
    pub clash_api_listen: String,
    pub clash_api_port: u16,
    pub selected_tag: Option<String>,
    pub auto_set_system_proxy: bool,
    /// Last user-desired proxy-service state (Home power button).
    ///
    /// Written on a successful start / stop of capture; quit must not clear it.
    /// Launch restores capture when this is true. Missing field → false so
    /// existing `settings.json` files keep the previous "core only" launch.
    #[serde(default)]
    pub proxy_service_enabled: bool,
    /// When true, the mixed inbound binds `0.0.0.0` so LAN devices can use the proxy.
    /// Defaults to false for existing `settings.json` files (`#[serde(default)]`).
    #[serde(default)]
    pub allow_lan: bool,
    /// Routing mode; defaults to `rule` for existing `settings.json` files.
    #[serde(default)]
    pub proxy_mode: ProxyMode,
    /// TUN capture parameters. Defaults to disabled for existing `settings.json`
    /// files; no settings migration ever enables TUN implicitly (plan §2.6).
    #[serde(default)]
    pub tun: TunSettings,
    /// When true, subscriptions whose body carries no routing rules get the
    /// built-in split-routing defaults (private IPs / China direct, rest via
    /// the selected node) plus a matching DNS split. Defaults to on for
    /// existing `settings.json` files.
    #[serde(default = "default_auto_default_rules")]
    pub auto_default_rules: bool,
    /// UI language; `system` (follow OS locale) for existing `settings.json`
    /// files.
    #[serde(default)]
    pub language: LanguagePreference,
    /// Background app-update checks and the sidebar indicator. Defaults to on
    /// for existing `settings.json` files; the Settings page can turn it off.
    #[serde(default = "default_check_app_updates")]
    pub check_app_updates: bool,
    /// Logs page shows every parsed line when true. Display-only; default
    /// off (connections and important events). Missing field → false.
    #[serde(default)]
    pub log_debug: bool,
}

fn default_auto_default_rules() -> bool {
    true
}

fn default_check_app_updates() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            mixed_listen: "127.0.0.1".into(),
            mixed_port: 17890,
            clash_api_listen: "127.0.0.1".into(),
            clash_api_port: 19090,
            selected_tag: None,
            auto_set_system_proxy: default_auto_set_system_proxy(),
            proxy_service_enabled: false,
            allow_lan: false,
            proxy_mode: ProxyMode::Rule,
            tun: TunSettings::default(),
            auto_default_rules: true,
            language: LanguagePreference::System,
            check_app_updates: true,
            log_debug: false,
        }
    }
}

impl AppSettings {
    /// Reject wildcard / non-loopback listens. Does not mutate or write disk.
    pub fn validate(&self) -> Result<(), AppError> {
        self.validate_for(HostPlatform::Linux)
    }

    pub fn validate_for(&self, platform: HostPlatform) -> Result<(), AppError> {
        validate_listen_addr("clash_api_listen", &self.clash_api_listen)?;
        if !is_loopback_host(&self.clash_api_listen) {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                format!(
                    "clash_api_listen must be a loopback address, got {}",
                    self.clash_api_listen
                ),
            ));
        }
        // With allow_lan the mixed inbound binds 0.0.0.0 at build time, so the stored
        // mixed_listen is only meaningful when allow_lan is off.
        if !self.allow_lan {
            validate_listen_addr("mixed_listen", &self.mixed_listen)?;
            if !is_loopback_host(&self.mixed_listen) {
                return Err(AppError::new(
                    ErrorCode::ConfigInvalid,
                    format!(
                        "mixed_listen must be a loopback address, got {}",
                        self.mixed_listen
                    ),
                ));
            }
        }
        if self.mixed_port < 1024 || self.clash_api_port < 1024 {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                "ports must be in 1024..=65535",
            ));
        }
        if self.mixed_port == self.clash_api_port {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                "mixed_port must differ from clash_api_port",
            ));
        }
        self.tun.validate_for(platform)?;
        Ok(())
    }

    /// Apply a partial IPC patch. Omitted fields keep `self`.
    ///
    /// Settings UI omits `proxy_service_enabled` (Home start/stop owns it).
    /// Pass `Some` only from the dedicated on/off path.
    pub fn apply_patch(&self, patch: &SettingsPatch) -> AppSettings {
        let mut next = self.clone();
        if let Some(v) = patch.mixed_listen.clone() {
            next.mixed_listen = v;
        }
        if let Some(v) = patch.mixed_port {
            next.mixed_port = v;
        }
        if let Some(v) = patch.clash_api_listen.clone() {
            next.clash_api_listen = v;
        }
        if let Some(v) = patch.clash_api_port {
            next.clash_api_port = v;
        }
        if let Some(v) = patch.selected_tag.clone() {
            next.selected_tag = v;
        }
        if let Some(v) = patch.auto_set_system_proxy {
            next.auto_set_system_proxy = v;
        }
        if let Some(v) = patch.proxy_service_enabled {
            next.proxy_service_enabled = v;
        }
        if let Some(v) = patch.allow_lan {
            next.allow_lan = v;
        }
        if let Some(v) = patch.proxy_mode {
            next.proxy_mode = v;
        }
        if let Some(tun) = &patch.tun {
            next.tun = tun.apply(&self.tun);
        }
        if let Some(v) = patch.auto_default_rules {
            next.auto_default_rules = v;
        }
        if let Some(v) = patch.language {
            next.language = v;
        }
        if let Some(v) = patch.check_app_updates {
            next.check_app_updates = v;
        }
        if let Some(v) = patch.log_debug {
            next.log_debug = v;
        }
        next
    }
}

/// Partial `settings.json` write (ORCH-3). Every field is optional; `null`
/// on `selected_tag` / TUN `interface_name` clears the value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mixed_listen: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mixed_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clash_api_listen: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clash_api_port: Option<u16>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_double_option"
    )]
    pub selected_tag: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_set_system_proxy: Option<bool>,
    /// Home start/stop owns this flag. Settings omits it; `Some` is for
    /// dedicated callers that must persist on/off without a full snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_service_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_lan: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_mode: Option<ProxyMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tun: Option<TunSettingsPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_default_rules: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<LanguagePreference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_app_updates: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_debug: Option<bool>,
}

/// Nested TUN patch; omitted fields keep the current `TunSettings`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TunSettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_double_option"
    )]
    pub interface_name: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipv4_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipv6_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtu: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_route: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict_route: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns_hijack: Option<bool>,
}

impl TunSettingsPatch {
    pub fn apply(&self, base: &TunSettings) -> TunSettings {
        TunSettings {
            enabled: self.enabled.unwrap_or(base.enabled),
            interface_name: match &self.interface_name {
                Some(v) => v.clone(),
                None => base.interface_name.clone(),
            },
            ipv4_address: self
                .ipv4_address
                .clone()
                .unwrap_or_else(|| base.ipv4_address.clone()),
            ipv6_address: self
                .ipv6_address
                .clone()
                .unwrap_or_else(|| base.ipv6_address.clone()),
            mtu: self.mtu.unwrap_or(base.mtu),
            auto_route: self.auto_route.unwrap_or(base.auto_route),
            strict_route: self.strict_route.unwrap_or(base.strict_route),
            stack: self.stack.clone().unwrap_or_else(|| base.stack.clone()),
            dns_hijack: self.dns_hijack.unwrap_or(base.dns_hijack),
        }
    }
}

/// Distinguish omitted (`None` / keep) from JSON `null` (`Some(None)` / clear).
fn deserialize_double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::deserialize(deserializer)?))
}

fn is_unspecified(addr: &str) -> bool {
    matches!(addr, "0.0.0.0" | "::" | "[::]")
}

fn validate_listen_addr(field: &str, addr: &str) -> Result<(), AppError> {
    if is_unspecified(addr) {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            format!("{field} must not be {addr}; use 127.0.0.1"),
        ));
    }
    Ok(())
}
