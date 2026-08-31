//! `settings.json` load / save (architecture §6.1).

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::atomic::write_json_atomic;
use crate::error::{AppError, ErrorCode};
use crate::listen::is_loopback_host;
use crate::LocalTemplate;

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
    /// (e.g. macOS `utun<N>`) are enforced per compile-time target; further
    /// host checks belong to the platform backend (`ice-tun-sys`, T2).
    pub fn validate(&self) -> Result<(), AppError> {
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
            if !tun_interface_name_valid(name) {
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
fn tun_interface_name_valid(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }
    if name
        .chars()
        .any(|c| c.is_whitespace() || c == '/' || c == '\\')
    {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
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
}

fn default_auto_default_rules() -> bool {
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
            allow_lan: false,
            proxy_mode: ProxyMode::Rule,
            tun: TunSettings::default(),
            auto_default_rules: true,
        }
    }
}

impl AppSettings {
    /// Reject wildcard / non-loopback listens. Does not mutate or write disk.
    pub fn validate(&self) -> Result<(), AppError> {
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
        self.tun.validate()?;
        Ok(())
    }

    pub fn to_local_template(&self) -> LocalTemplate {
        LocalTemplate {
            mixed_listen: self.mixed_listen.clone(),
            mixed_port: self.mixed_port,
            clash_api_listen: self.clash_api_listen.clone(),
            clash_api_port: self.clash_api_port,
            allow_lan: self.allow_lan,
            proxy_mode: self.proxy_mode,
            tun: self.tun.clone(),
        }
    }
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

/// Missing file → architecture §6.1 defaults (does not create the file).
pub fn load_settings(path: &Path) -> Result<AppSettings, AppError> {
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let raw = fs::read_to_string(path).map_err(|e| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("read settings {}: {e}", path.display()),
        )
    })?;
    let settings: AppSettings = serde_json::from_str(&raw)
        .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("parse settings: {e}")))?;
    settings.validate()?;
    Ok(settings)
}

/// Validate then atomically write. Invalid listens are rejected (no disk write).
pub fn save_settings(path: &Path, settings: &AppSettings) -> Result<(), AppError> {
    settings.validate()?;
    write_json_atomic(path, settings).map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_settings_path(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-settings-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("mkdir");
        dir.join("settings.json")
    }

    #[test]
    fn missing_settings_file_returns_architecture_defaults() {
        let path = temp_settings_path("missing");
        assert!(!path.exists());
        let s = load_settings(&path).expect("load");
        let d = AppSettings::default();
        assert_eq!(s.mixed_listen, "127.0.0.1");
        assert_eq!(s.mixed_port, 17890);
        assert_eq!(s.clash_api_listen, "127.0.0.1");
        assert_eq!(s.clash_api_port, 19090);
        assert_eq!(s.selected_tag, None);
        assert_eq!(s.auto_set_system_proxy, default_auto_set_system_proxy());
        assert!(!s.allow_lan);
        assert_eq!(s, d);
        assert!(!path.exists(), "load must not create settings file");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn default_auto_set_system_proxy_matches_real_backends() {
        assert!(
            !default_auto_set_system_proxy(),
            "system proxy is home-button controlled, not auto on Start"
        );
        assert_eq!(
            AppSettings::default().auto_set_system_proxy,
            default_auto_set_system_proxy()
        );
    }

    #[test]
    fn legacy_settings_without_allow_lan_loads_as_false() {
        let path = temp_settings_path("legacy");
        let json = r#"{
            "mixed_listen": "127.0.0.1",
            "mixed_port": 17890,
            "clash_api_listen": "127.0.0.1",
            "clash_api_port": 19090,
            "selected_tag": null,
            "auto_set_system_proxy": true
        }"#;
        fs::write(&path, json).expect("write");
        let s = load_settings(&path).expect("legacy json without allow_lan");
        assert!(!s.allow_lan);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn legacy_settings_without_proxy_mode_loads_as_rule() {
        let path = temp_settings_path("legacy-mode");
        let json = r#"{
            "mixed_listen": "127.0.0.1",
            "mixed_port": 17890,
            "clash_api_listen": "127.0.0.1",
            "clash_api_port": 19090,
            "selected_tag": null,
            "auto_set_system_proxy": true
        }"#;
        fs::write(&path, json).expect("write");
        let s = load_settings(&path).expect("legacy json without proxy_mode");
        assert_eq!(s.proxy_mode, ProxyMode::Rule);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn proxy_mode_round_trips() {
        let path = temp_settings_path("mode-roundtrip");
        save_settings(
            &path,
            &AppSettings {
                proxy_mode: ProxyMode::Global,
                ..AppSettings::default()
            },
        )
        .expect("save global");
        assert_eq!(load_settings(&path).unwrap().proxy_mode, ProxyMode::Global);

        save_settings(
            &path,
            &AppSettings {
                proxy_mode: ProxyMode::Direct,
                ..AppSettings::default()
            },
        )
        .expect("save direct");
        assert_eq!(load_settings(&path).unwrap().proxy_mode, ProxyMode::Direct);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn clash_mode_name_covers_all_supported_modes() {
        assert_eq!(clash_mode_name(ProxyMode::Rule), "Rule");
        assert_eq!(clash_mode_name(ProxyMode::Global), "Global");
        assert_eq!(clash_mode_name(ProxyMode::Direct), "Direct");
    }

    /// The WinInet backend (slice 4b) made the Windows system proxy real, so the flag must
    /// be accepted on every platform (settings files carrying it must load).
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_accepts_auto_set_system_proxy() {
        let path = temp_settings_path("win-proxy");
        let settings = AppSettings {
            auto_set_system_proxy: true,
            ..AppSettings::default()
        };
        save_settings(&path, &settings).expect("win auto proxy accepted");
        let loaded = load_settings(&path).expect("reload");
        assert!(loaded.auto_set_system_proxy);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn reject_unspecified_listen_without_writing() {
        let path = temp_settings_path("bad-listen");
        fs::write(&path, b"keep-me").expect("seed");

        let bad_clash = AppSettings {
            clash_api_listen: "0.0.0.0".into(),
            ..AppSettings::default()
        };
        let err = save_settings(&path, &bad_clash).expect_err("reject clash 0.0.0.0");
        assert_eq!(err.code, "config.invalid");
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep-me");

        let bad_mixed = AppSettings {
            mixed_listen: "0.0.0.0".into(),
            ..AppSettings::default()
        };
        let err = save_settings(&path, &bad_mixed).expect_err("reject mixed 0.0.0.0");
        assert_eq!(err.code, "config.invalid");
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep-me");

        let bad_mixed_lan = AppSettings {
            mixed_listen: "192.168.1.1".into(),
            ..AppSettings::default()
        };
        let err = save_settings(&path, &bad_mixed_lan).expect_err("reject mixed lan");
        assert_eq!(err.code, "config.invalid");
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep-me");

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn allow_lan_relaxes_mixed_listen_validation() {
        let path = temp_settings_path("allow-lan");
        fs::write(&path, b"keep-me").expect("seed");

        let lan = AppSettings {
            allow_lan: true,
            mixed_listen: "0.0.0.0".into(),
            ..AppSettings::default()
        };
        save_settings(&path, &lan).expect("allow_lan accepts any mixed_listen");
        let on_disk = load_settings(&path).expect("reload");
        assert!(on_disk.allow_lan);

        let clash_lan = AppSettings {
            allow_lan: true,
            clash_api_listen: "0.0.0.0".into(),
            ..AppSettings::default()
        };
        let before = fs::read_to_string(&path).unwrap();
        let err = save_settings(&path, &clash_lan).expect_err("clash api stays loopback");
        assert_eq!(err.code, "config.invalid");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            before,
            "no disk write on reject"
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn reject_invalid_ports_without_writing() {
        let path = temp_settings_path("bad-port");
        fs::write(&path, b"keep-me").expect("seed");

        let bad = AppSettings {
            mixed_port: 80,
            ..AppSettings::default()
        };
        let err = save_settings(&path, &bad).expect_err("low port");
        assert_eq!(err.code, "config.invalid");
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep-me");

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn reject_out_of_range_port_in_settings_json() {
        let path = temp_settings_path("high-load");
        let json = r#"{
            "mixed_listen": "127.0.0.1",
            "mixed_port": 70000,
            "clash_api_listen": "127.0.0.1",
            "clash_api_port": 19090,
            "selected_tag": null,
            "auto_set_system_proxy": false
        }"#;
        fs::write(&path, json).expect("write");
        let err = load_settings(&path).expect_err("out of range port");
        assert_eq!(err.code, "config.invalid");
        assert!(err.message.contains("parse settings"));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    // --- TUN settings (slice T1, plan §4.1) ---

    #[test]
    fn legacy_settings_without_tun_loads_disabled_with_locked_defaults() {
        let path = temp_settings_path("legacy-tun");
        let json = r#"{
            "mixed_listen": "127.0.0.1",
            "mixed_port": 17890,
            "clash_api_listen": "127.0.0.1",
            "clash_api_port": 19090,
            "selected_tag": null,
            "auto_set_system_proxy": true
        }"#;
        fs::write(&path, json).expect("write");
        let s = load_settings(&path).expect("legacy json without tun");
        assert!(
            !s.tun.enabled,
            "missing TUN fields must mean disabled (no silent migration)"
        );
        assert_eq!(s.tun, TunSettings::default());
        assert_eq!(s.tun.ipv4_address, "10.0.0.1/30");
        assert_eq!(s.tun.ipv6_address, "fdfe:dcba:9876::1/126");
        assert_eq!(s.tun.mtu, 9000);
        assert_eq!(s.tun.stack, "gvisor");
        assert!(s.tun.auto_route);
        assert!(s.tun.strict_route);
        assert!(s.tun.dns_hijack, "dns hijack is the locked default");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn tun_enabled_round_trips_through_save_and_load() {
        let path = temp_settings_path("tun-roundtrip");
        let settings = AppSettings {
            tun: TunSettings {
                enabled: true,
                interface_name: Some("utun420".into()),
                ..TunSettings::default()
            },
            ..AppSettings::default()
        };
        save_settings(&path, &settings).expect("save tun settings");
        let loaded = load_settings(&path).expect("reload");
        assert!(loaded.tun.enabled);
        assert_eq!(loaded.tun.interface_name.as_deref(), Some("utun420"));
        assert_eq!(loaded.tun, settings.tun);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn tun_validation_rejects_bad_cidrs_without_writing() {
        let path = temp_settings_path("tun-bad-cidr");
        fs::write(&path, b"keep-me").expect("seed");
        let base = || AppSettings {
            tun: TunSettings {
                enabled: true,
                ..TunSettings::default()
            },
            ..AppSettings::default()
        };

        let cases = [
            "10.0.0.1",              // not a CIDR
            "10.0.0.1/",             // empty prefix
            "10.0.0.1/33",           // prefix out of range
            "10.0.0.1/0",            // /0 interface address rejected
            "10.0.0.1/24/x",         // extra segment
            "999.1.1.1/24",          // bad octet
            "fdfe:dcba:9876::1",     // v6 without prefix
            "fdfe:dcba:9876::1/129", // v6 prefix out of range
            "fdfe:dcba:9876::1/0",   // v6 /0 rejected
            "not-an-address/24",     // junk address
        ];
        for cidr in cases {
            let bad = AppSettings {
                tun: TunSettings {
                    ipv4_address: cidr.into(),
                    ..TunSettings::default()
                },
                ..base()
            };
            let err = save_settings(&path, &bad).expect_err("reject bad v4 cidr");
            assert_eq!(err.code, "config.invalid", "case: {cidr}");
            assert_eq!(fs::read_to_string(&path).unwrap(), "keep-me");
        }
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn tun_validation_rejects_bad_ipv6_and_mtu_and_stack() {
        let path = temp_settings_path("tun-bad-rest");
        fs::write(&path, b"keep-me").expect("seed");
        let mut settings = AppSettings {
            tun: TunSettings {
                enabled: true,
                ..TunSettings::default()
            },
            ..AppSettings::default()
        };

        settings.tun.ipv6_address = "10.0.0.1/24".into();
        let err = save_settings(&path, &settings).expect_err("v4 address in v6 field");
        assert_eq!(err.code, "config.invalid");
        assert!(err.message.contains("tun.ipv6_address"));

        settings.tun = TunSettings::default();
        settings.tun.mtu = 576;
        let err = save_settings(&path, &settings).expect_err("mtu below minimum");
        assert_eq!(err.code, "config.invalid");
        assert!(err.message.contains("tun.mtu"));

        settings.tun = TunSettings::default();
        settings.tun.stack = "tap".into();
        let err = save_settings(&path, &settings).expect_err("unknown stack");
        assert_eq!(err.code, "config.invalid");
        assert!(err.message.contains("tun.stack"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep-me");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn tun_validation_rejects_bad_interface_names() {
        let path = temp_settings_path("tun-bad-iface");
        fs::write(&path, b"keep-me").expect("seed");
        let mut settings = AppSettings {
            tun: TunSettings {
                enabled: true,
                ..TunSettings::default()
            },
            ..AppSettings::default()
        };
        for name in [
            "",
            " ",
            "with space",
            "a/b",
            "a\\b",
            "a\nb",
            &"x".repeat(65),
        ] {
            settings.tun.interface_name = Some(name.into());
            let err = save_settings(&path, &settings).expect_err("reject bad interface name");
            assert_eq!(err.code, "config.invalid", "case: {name:?}");
            assert_eq!(fs::read_to_string(&path).unwrap(), "keep-me");
        }
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_tun_interface_name_requires_utun_numeric_suffix() {
        let path = temp_settings_path("tun-macos-iface");
        fs::write(&path, b"keep-me").expect("seed");
        let mut settings = AppSettings {
            tun: TunSettings {
                enabled: true,
                ..TunSettings::default()
            },
            ..AppSettings::default()
        };
        for name in ["tun0", "utun", "utunx", "utun-1", "Utun4"] {
            settings.tun.interface_name = Some(name.into());
            let err = save_settings(&path, &settings).expect_err("reject non-utun<N>");
            assert_eq!(err.code, "config.invalid", "case: {name}");
        }
        for name in ["utun0", "utun420", "utun0007"] {
            settings.tun.interface_name = Some(name.into());
            save_settings(&path, &settings).expect("accept utun<N>");
        }
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_accepts_arbitrary_sane_interface_names() {
        // Platform-exact checks belong to each platform backend (T2); the shared
        // validator only enforces the sanity rules.
        assert!(tun_interface_name_valid("utun420"));
        assert!(tun_interface_name_valid("wintun-ice-box"));
        assert!(tun_interface_name_valid("Tun0"));
        assert!(!tun_interface_name_valid("with space"));
        assert!(!tun_interface_name_valid("a/b"));
    }

    #[test]
    fn tun_defaults_are_locked_and_never_enable_capture() {
        let d = TunSettings::default();
        assert!(!d.enabled);
        assert_eq!(d.ipv4_address, TUN_DEFAULT_IPV4_ADDRESS);
        assert_eq!(d.ipv6_address, TUN_DEFAULT_IPV6_ADDRESS);
        assert_eq!(d.mtu, TUN_DEFAULT_MTU);
        assert_eq!(d.stack, TUN_DEFAULT_STACK);
        assert!(d.auto_route && d.strict_route);
        assert!(d.dns_hijack, "dns hijack is the locked default");
    }
}
