// SPDX-License-Identifier: GPL-3.0-or-later

//! Build and validate the final sing-box JSON config.
//!
//! Settings load/save stay here (I/O). Shared DTOs (`AppError`, `AppPaths`,
//! `AppSettings`, listen checks) live in `ice-types`. Pid-file and log
//! rotation live in `ice-core`; tracing init lives in the desktop shell.

mod atomic;
mod build;
mod error;
mod profile;
mod redact;
mod rule_overrides;
mod runtime_config;
mod selections;
mod settings;

pub use atomic::{write_bytes_atomic, write_json_atomic};
pub use build::{
    build_direct_only_config, build_runtime_config, config_to_pretty_json,
    invalidate_geoip_code_cache, minimal_dns_block, restore_runtime_config_from_bak,
    tun_dns_hijack_rule, tun_reserved_rules, validate_config, validate_config_for_intent,
    validate_template, write_runtime_config_bytes, write_runtime_config_file, ConfigError,
};
pub use error::{AppError, ErrorCode};
pub use ice_types::{
    is_fake_ip, is_loopback_host, is_restricted_fetch_host, is_restricted_ip, AppPaths,
    HostPlatform, UiMessage, ENGINE_COMPAT_CORE_VERSION,
};
pub use profile::{NormalizedProfile, NormalizedRoute, ProfileParseStats};
pub use redact::{redact_config_json, redact_config_str};
pub use rule_overrides::{
    load_rule_overrides, rule_fingerprint, rule_matches_fingerprint, rule_type_of,
    save_rule_overrides, RuleOverrides, RULE_TYPE_KEYS,
};
pub use runtime_config::RuntimeConfig;
pub use selections::{
    apply_group_selections, load_group_selections, save_group_selections, GroupSelections,
};
pub use settings::{
    clash_mode_name, default_auto_set_system_proxy, load_settings, load_settings_detailed,
    save_settings, save_settings_for, set_proxy_service_enabled, set_proxy_service_enabled_for,
    tun_interface_name_valid, AppSettings, LanguagePreference, LoadSettingsOutcome, ProxyMode,
    SettingsPatch, TunSettings, TunSettingsPatch, TUN_DEFAULT_IPV4_ADDRESS,
    TUN_DEFAULT_IPV6_ADDRESS, TUN_DEFAULT_MTU, TUN_DEFAULT_STACK,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

/// The runtime capture intent for a generated config (`docs/tun.md`).
///
/// Supplied explicitly by orchestration; never inferred from `tun.enabled`
/// alone. `Diagnostic` is the default and matches the pre-TUN behavior exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureIntent {
    /// Mixed inbound only. Used by automatic core start and a stopped proxy
    /// service; never contains a TUN inbound.
    #[default]
    Diagnostic,
    /// Mixed plus TUN inbounds, with the reserved bypass rules first. Used only
    /// during a TUN capture transition and while TUN is active.
    Tun,
}

/// TUN gate status for the current platform (`docs/tun.md`).
///
/// `ready == false` means this platform must never generate or activate a TUN
/// config; the stable reason feeds `tun_available=false` in status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TunGate {
    pub ready: bool,
    pub reason: Option<&'static str>,
}

/// Test-only override: host-free controller tests inject fake backends on
/// every CI host; forcing the gate green lets them generate Tun configs on
/// non-macOS runners. Gated so production builds do not export it.
#[cfg(any(test, feature = "test-hooks"))]
static TEST_TUN_GATE_READY: std::sync::OnceLock<()> = std::sync::OnceLock::new();

/// Test-only escape hatch for host-free controller tests (see [`tun_gate`]).
#[cfg(any(test, feature = "test-hooks"))]
pub fn force_tun_gate_ready() {
    let _ = TEST_TUN_GATE_READY.set(());
}

/// T0 gate for an explicit [`HostPlatform`]. macOS and Windows are green
/// (`macos_tun_ready` / `windows_tun_ready`); other platforms are out of
/// scope for the first release.
///
/// Host-free controller tests force the gate green via
/// `force_tun_gate_ready` (`test-hooks` / `cfg(test)` only).
pub fn tun_gate_for(platform: HostPlatform) -> TunGate {
    #[cfg(any(test, feature = "test-hooks"))]
    if TEST_TUN_GATE_READY.get().is_some() {
        return TunGate {
            ready: true,
            reason: None,
        };
    }
    if platform.tun_ready() {
        TunGate {
            ready: true,
            reason: None,
        }
    } else {
        TunGate {
            ready: false,
            reason: Some("tun.unsupportedPlatform"),
        }
    }
}

/// Local template knobs that wrap subscription-derived outbounds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalTemplate {
    pub mixed_listen: String,
    pub mixed_port: u16,
    pub clash_api_listen: String,
    pub clash_api_port: u16,
    /// When true the mixed inbound binds `0.0.0.0` (LAN sharing).
    pub allow_lan: bool,
    /// Routing mode applied at build time (rule / global / direct).
    #[serde(default)]
    pub proxy_mode: ProxyMode,
    /// Validated TUN capture parameters. The TUN inbound is emitted only when
    /// the build intent is [`CaptureIntent::Tun`], never from `tun.enabled`
    /// alone.
    #[serde(default)]
    pub tun: TunSettings,
}

impl Default for LocalTemplate {
    fn default() -> Self {
        Self {
            mixed_listen: "127.0.0.1".into(),
            mixed_port: 17890,
            clash_api_listen: "127.0.0.1".into(),
            clash_api_port: 19090,
            allow_lan: false,
            proxy_mode: ProxyMode::Rule,
            tun: TunSettings::default(),
        }
    }
}

impl From<&AppSettings> for LocalTemplate {
    fn from(settings: &AppSettings) -> Self {
        Self {
            mixed_listen: settings.mixed_listen.clone(),
            mixed_port: settings.mixed_port,
            clash_api_listen: settings.clash_api_listen.clone(),
            clash_api_port: settings.clash_api_port,
            allow_lan: settings.allow_lan,
            proxy_mode: settings.proxy_mode,
            tun: settings.tun.clone(),
        }
    }
}

/// Normalized outbound produced by ice-subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedOutbound {
    pub tag: String,
    /// Raw sing-box outbound object (already in sing-box shape).
    ///
    /// `Arc` so profile clones (list/UI) do not deep-copy the JSON tree
    /// (PERF-1). Runtime emission serializes each `Arc` by reference.
    #[serde(
        serialize_with = "serialize_arc_json",
        deserialize_with = "deserialize_arc_json"
    )]
    pub outbound: Arc<Value>,
}

impl NormalizedOutbound {
    pub fn new(tag: impl Into<String>, outbound: Value) -> Self {
        Self {
            tag: tag.into(),
            outbound: Arc::new(outbound),
        }
    }

    /// Clone-on-write access to the outbound JSON (PERF-1).
    pub fn outbound_mut(&mut self) -> &mut Value {
        Arc::make_mut(&mut self.outbound)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildInput {
    pub template: LocalTemplate,
    /// Shared with the desktop profile cache so Apply does not clone a
    /// multi-MB `NormalizedProfile` (SUB-6).
    #[serde(
        serialize_with = "crate::serialize_arc_profile",
        deserialize_with = "crate::deserialize_arc_profile"
    )]
    pub profile: Arc<NormalizedProfile>,
    /// Optional selected outbound / selector tag.
    pub selected_tag: Option<String>,
    /// Directory containing bundled `geoip-{code}.srs` rule-set files (app resources).
    /// GEOIP rules whose code has no file here are dropped at build time.
    pub geoip_rule_set_dir: Option<PathBuf>,
    /// Persisted per-group member selections, applied as selector `default`s.
    #[serde(default)]
    pub group_selections: GroupSelections,
    /// Persisted rule overrides: disabled rules are dropped, custom rules prepended.
    #[serde(default)]
    pub rule_overrides: RuleOverrides,
    /// Runtime capture intent: `Tun` adds the TUN inbound + reserved bypass
    /// rules; `Diagnostic` keeps the Mixed-only shape. Never inferred from
    /// `tun.enabled` alone (`docs/tun.md`).
    #[serde(default)]
    pub capture_intent: CaptureIntent,
    /// Target OS for DNS / TUN reserved-rule emission. Never inferred from
    /// the compile-time target inside this crate.
    #[serde(default)]
    pub platform: HostPlatform,
}

fn serialize_arc_profile<S: serde::Serializer>(
    profile: &Arc<NormalizedProfile>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    profile.as_ref().serialize(serializer)
}

fn deserialize_arc_profile<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Arc<NormalizedProfile>, D::Error> {
    NormalizedProfile::deserialize(deserializer).map(Arc::new)
}

fn serialize_arc_json<S: serde::Serializer>(
    value: &Arc<Value>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    value.as_ref().serialize(serializer)
}

fn deserialize_arc_json<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Arc<Value>, D::Error> {
    Value::deserialize(deserializer).map(Arc::new)
}

/// Legacy helper: build from flat node list (tests / fallback).
pub fn build_input_from_nodes(
    template: LocalTemplate,
    outbounds: Vec<NormalizedOutbound>,
    selected_tag: Option<String>,
) -> BuildInput {
    BuildInput {
        template,
        profile: Arc::new(NormalizedProfile::from_nodes_only(outbounds)),
        selected_tag,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Diagnostic,
        platform: HostPlatform::MacOs,
    }
}
