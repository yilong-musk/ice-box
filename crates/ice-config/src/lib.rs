//! Build and validate the final sing-box JSON config.
//!
//! Also hosts shared DTOs / helpers: `AppError`, paths, settings, atomic IO, pid.

mod atomic;
mod error;
mod listen;
mod logging;
mod paths;
mod pid;
mod profile;
mod redact;
mod rule_overrides;
mod selections;
mod settings;

pub use atomic::{write_bytes_atomic, write_json_atomic};
pub use error::{AppError, ErrorCode};
pub use listen::{is_fake_ip, is_loopback_host, is_restricted_fetch_host, is_restricted_ip};
pub use logging::init_logging;
pub use paths::AppPaths;
pub use pid::{clear_pid, parse_pid_contents, purge_invalid_pid_file, read_pid, write_pid};
pub use profile::{NormalizedProfile, NormalizedRoute, ProfileParseStats};
pub use redact::{redact_config_json, redact_config_str};
pub use rule_overrides::{
    load_rule_overrides, rule_fingerprint, rule_type_of, save_rule_overrides, RuleOverrides,
    RULE_TYPE_KEYS,
};
pub use selections::{
    apply_group_selections, load_group_selections, save_group_selections, GroupSelections,
};
pub use settings::{
    clash_mode_name, default_auto_set_system_proxy, load_settings, save_settings, AppSettings,
    ProxyMode, TunSettings, TUN_DEFAULT_IPV4_ADDRESS, TUN_DEFAULT_IPV6_ADDRESS, TUN_DEFAULT_MTU,
    TUN_DEFAULT_STACK,
};

/// sing-box core version the config generator targets (architecture §12 / §22).
///
/// Bundled desktop binaries (`third_party/sing-box/VERSION`) must match this pin;
/// generated config features (e.g. rule options, removed in sing-box 1.13) are
/// only tested against this version range.
pub const ENGINE_COMPAT_CORE_VERSION: &str = "1.13.19";

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// The runtime capture intent for a generated config (plan §4.1).
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

/// TUN T0 gate status for the current platform (plan §3.2, §5 T1).
///
/// `ready == false` means this platform must never generate or activate a TUN
/// config; the stable reason feeds `tun_available=false` in status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TunGate {
    pub ready: bool,
    pub reason: Option<&'static str>,
}

/// Compile-time T0 gate per platform. macOS is green (`macos_tun_ready` — live
/// spike passed); Windows is pending (`windows_tun_ready` — host spike not yet
/// run); other platforms are out of scope for the first release.
///
/// Test-only override: the desktop crate's host-free controller tests run on
/// every CI host and inject fake backends; forcing the gate green there lets
/// them generate Tun configs on non-macOS runners. Production code never
/// calls [`force_tun_gate_ready`].
static TEST_TUN_GATE_READY: std::sync::OnceLock<()> = std::sync::OnceLock::new();

/// Test-only escape hatch for host-free controller tests (see [`tun_gate`]).
pub fn force_tun_gate_ready() {
    let _ = TEST_TUN_GATE_READY.set(());
}

pub fn tun_gate() -> TunGate {
    if TEST_TUN_GATE_READY.get().is_some() {
        return TunGate {
            ready: true,
            reason: None,
        };
    }
    #[cfg(target_os = "macos")]
    {
        TunGate {
            ready: true,
            reason: None,
        }
    }
    #[cfg(target_os = "windows")]
    {
        TunGate {
            ready: false,
            reason: Some("Windows TUN gate pending (windows_tun_ready): WinTUN/UAC spike not run"),
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        TunGate {
            ready: false,
            reason: Some("TUN is supported on macOS and Windows only in the first release"),
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

/// Normalized outbound produced by ice-subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedOutbound {
    pub tag: String,
    /// Raw sing-box outbound object (already in sing-box shape).
    pub outbound: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildInput {
    pub template: LocalTemplate,
    pub profile: NormalizedProfile,
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
    /// `tun.enabled` alone (plan §4.1).
    #[serde(default)]
    pub capture_intent: CaptureIntent,
}

/// Legacy helper: build from flat node list (tests / fallback).
pub fn build_input_from_nodes(
    template: LocalTemplate,
    outbounds: Vec<NormalizedOutbound>,
    selected_tag: Option<String>,
) -> BuildInput {
    BuildInput {
        template,
        profile: NormalizedProfile::from_nodes_only(outbounds),
        selected_tag,
        geoip_rule_set_dir: None,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent: CaptureIntent::Diagnostic,
    }
}

/// Minimal config for running with no subscription: only builtin `direct` /
/// `block` outbounds, every route final is `direct`. Lets a first-run user start
/// the core (system proxy, inbound) before importing any subscription; importing
/// one later hot-reloads to the real config.
///
/// The capture intent is honored: a `Tun` intent adds the TUN inbound and the
/// reserved bypass rules (a no-node profile must not silently downgrade a
/// requested `Tun` intent to Mixed-only — plan §4.2.6).
pub fn build_direct_only_config(
    template: &LocalTemplate,
    capture_intent: CaptureIntent,
) -> Result<Value, ConfigError> {
    validate_template(template)?;
    if capture_intent == CaptureIntent::Tun {
        validate_tun_capture(template)?;
    }

    let outbounds = vec![
        json!({"type": "direct", "tag": "direct"}),
        json!({"type": "block", "tag": "block"}),
    ];

    // Slice 4c: keep the `clash_mode` rules so a later reload to a real config keeps the
    // mode switch wired; without proxy outbounds every mode routes direct anyway. A `Tun`
    // intent prepends the reserved bypass rules so the control path stays direct even in
    // direct-only fallback.
    let mut rules = Vec::new();
    if capture_intent == CaptureIntent::Tun {
        rules.extend(tun_reserved_rules());
    }
    rules.push(json!({ "clash_mode": "global", "outbound": "direct" }));
    rules.push(json!({ "clash_mode": "direct", "outbound": "direct" }));
    let route = json!({
        "final": "direct",
        "auto_detect_interface": true,
        "rules": rules,
        "default_domain_resolver": "local",
    });

    let mut inbounds = vec![json!({
        "type": "mixed",
        "tag": "mixed-in",
        "listen": if template.allow_lan {
            "0.0.0.0"
        } else {
            template.mixed_listen.as_str()
        },
        "listen_port": template.mixed_port,
    })];
    if capture_intent == CaptureIntent::Tun {
        inbounds.push(tun_inbound(&template.tun));
    }

    let config = json!({
        "log": { "level": "info", "timestamp": true },
        "dns": minimal_dns_block(),
        "inbounds": inbounds,
        "outbounds": outbounds,
        "route": route,
        "experimental": {
            "clash_api": {
                "external_controller": format!(
                    "{}:{}",
                    template.clash_api_listen, template.clash_api_port
                ),
                // NOTE: `mode_list` must NOT be emitted — the pinned sing-box 1.13.19
                // rejects it ("unknown field"). The runtime mode-list is `[<default_mode>]`
                // only, so a PATCH to another mode is silently ignored and mode switching
                // always takes the rebuild + reload path (see `orchestrate_set_proxy_mode`).
                "default_mode": clash_mode_name(template.proxy_mode),
            }
        }
    });

    validate_config_for_intent(&config, capture_intent)?;
    Ok(config)
}

/// Validate listen ports before build (architecture §12.3).
pub fn validate_template(template: &LocalTemplate) -> Result<(), ConfigError> {
    if template.mixed_port < 1024 || template.clash_api_port < 1024 {
        return Err(ConfigError::Invalid("port must be in 1024..=65535"));
    }
    if template.mixed_port == template.clash_api_port {
        return Err(ConfigError::Invalid(
            "mixed port must differ from clash api port",
        ));
    }
    // With allow_lan the mixed inbound binds 0.0.0.0; the stored mixed_listen only
    // matters for loopback mode.
    if !template.allow_lan && !is_loopback_host(&template.mixed_listen) {
        return Err(ConfigError::Invalid(
            "mixed_listen must be a loopback address",
        ));
    }
    if !is_loopback_host(&template.clash_api_listen) {
        return Err(ConfigError::Invalid(
            "clash_api_listen must be a loopback address",
        ));
    }
    Ok(())
}

/// Minimal DNS block locked for v1 (sing-box 1.13+ local resolver).
pub fn minimal_dns_block() -> Value {
    json!({
        "servers": [
            {
                "type": "local",
                "tag": "local"
            }
        ],
        "final": "local"
    })
}

/// Whether the DNS block contains a `local`-tagged server (required for
/// `route.default_domain_resolver` references).
fn dns_has_local_server(dns: &Value) -> bool {
    dns.get("servers")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .any(|s| s.get("tag").and_then(|v| v.as_str()) == Some("local"))
        })
        .unwrap_or(false)
}

/// Merge template + subscription profile into a sing-box config object.
pub fn build_runtime_config(input: &BuildInput) -> Result<Value, ConfigError> {
    validate_template(&input.template)?;
    let capture_intent = input.capture_intent;
    if capture_intent == CaptureIntent::Tun {
        validate_tun_capture(&input.template)?;
    }

    if input.profile.nodes.is_empty() {
        return Err(ConfigError::EmptyOutbounds);
    }

    let mut tag_set: std::collections::HashSet<String> =
        input.profile.all_tags().into_iter().collect();
    let mut outbounds: Vec<Value> = Vec::new();

    for node in &input.profile.nodes {
        let mut ob = node.outbound.clone();
        if let Some(obj) = ob.as_object_mut() {
            obj.insert("tag".into(), Value::String(node.tag.clone()));
        }
        outbounds.push(ob);
    }

    for group in &input.profile.groups {
        let mut ob = group.outbound.clone();
        if let Some(obj) = ob.as_object_mut() {
            obj.insert("tag".into(), Value::String(group.tag.clone()));
        }
        outbounds.push(ob);
    }

    ensure_builtin_outbound(
        &mut outbounds,
        &mut tag_set,
        "direct",
        json!({"type": "direct", "tag": "direct"}),
    );
    ensure_builtin_outbound(
        &mut outbounds,
        &mut tag_set,
        "block",
        json!({"type": "block", "tag": "block"}),
    );

    let fallback = input
        .profile
        .default_outbound
        .clone()
        .filter(|t| tag_set.contains(t))
        .or_else(|| input.profile.groups.first().map(|g| g.tag.clone()))
        .or_else(|| input.profile.nodes.first().map(|n| n.tag.clone()))
        .unwrap_or_else(|| "direct".into());

    let selected = match &input.selected_tag {
        Some(sel) if tag_set.contains(sel) => sel.clone(),
        _ => fallback.clone(),
    };

    // Apply selected default on top-level selector when applicable.
    // A selector default must be one of its members and never itself.
    for ob in &mut outbounds {
        if ob.get("tag").and_then(|v| v.as_str()) == Some(selected.as_str())
            && ob.get("type").and_then(|v| v.as_str()) == Some("selector")
        {
            let members: Vec<&str> = ob
                .get("outbounds")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            if members.contains(&selected.as_str()) {
                ob.as_object_mut()
                    .unwrap()
                    .insert("default".into(), Value::String(selected.clone()));
            }
        }
    }

    // Persisted per-group selections win over subscription defaults (and the
    // selected-tag default above) for selector groups.
    apply_group_selections(&mut outbounds, &input.group_selections);

    // v1 fallback: no groups → inject flat proxy selector
    if input.profile.groups.is_empty() {
        let node_tags: Vec<String> = input.profile.nodes.iter().map(|n| n.tag.clone()).collect();
        let proxy_default = if node_tags.iter().any(|t| t == &selected) {
            selected.clone()
        } else {
            node_tags[0].clone()
        };
        outbounds.push(json!({
            "type": "selector",
            "tag": "proxy",
            "outbounds": node_tags,
            "default": proxy_default,
        }));
        tag_set.insert("proxy".into());
    }

    // Routing mode (Slice 4c): the generated config always carries the full rule set plus
    // two `clash_mode` rules prepended first. The runtime mode is switched live via Clash
    // API `PATCH /configs` (no rebuild / reload / restart), so `route.final` stays at the
    // rule-mode value in every mode — the `clash_mode` rules short-circuit before it.
    //
    // `<global target>` is the same outbound `ProxyMode::Global` routed `final` to before
    // (the injected `proxy` selector when the profile has no groups, else the top
    // group / fallback), so homepage node selection keeps working in global mode.
    let global_target = if input.profile.groups.is_empty() {
        "proxy".to_string()
    } else {
        fallback.clone()
    };
    let route_final =
        if input.profile.route.final_outbound == "proxy" && input.profile.groups.is_empty() {
            "proxy".to_string()
        } else {
            input.profile.route.final_outbound.clone()
        };

    let mut route = json!({
        "final": route_final,
        "auto_detect_interface": true,
    });

    // Disabled (fingerprint-matched) subscription rules are dropped; custom rules are
    // prepended after the `clash_mode` rules so a custom / subscription rule can never
    // win over the active runtime mode (e.g. a custom `direct` rule in global mode).
    let (final_rules, rule_sets): (Vec<Value>, Vec<Value>) = {
        let mut final_rules: Vec<Value> = Vec::new();
        if capture_intent == CaptureIntent::Tun {
            // Reserved bypass rules precede `clash_mode` (T0 lock, §24.5.6): the control
            // path, private/loopback/link-local/multicast destinations, and the TUN
            // endpoint are never captured or sniffed, even in Global/Direct mode.
            final_rules.extend(tun_reserved_rules());
        }
        final_rules.push(json!({ "clash_mode": "global", "outbound": global_target }));
        final_rules.push(json!({ "clash_mode": "direct", "outbound": "direct" }));
        let enabled_sub_rules: Vec<Value> = input
            .profile
            .route
            .rules
            .iter()
            .filter(|r| !input.rule_overrides.is_disabled(&rule_fingerprint(r)))
            .cloned()
            .collect();
        let (sub_rules, sub_sets) = expand_geoip_rules(
            &enabled_sub_rules,
            &input.profile.route.rule_sets,
            input.geoip_rule_set_dir.as_deref(),
        );
        let sub_set_tags: std::collections::HashSet<&str> = sub_sets
            .iter()
            .filter_map(|s| s.get("tag").and_then(|v| v.as_str()))
            .collect();
        // Custom rules persist globally (data-dir `rules.json`) and survive
        // subscription switches, but their `outbound` / `rule_set` references may
        // not exist in the *new* active subscription. Skip those rules instead of
        // failing the whole build, so switching subscriptions can never break
        // Apply / Start; the rule stays persisted and resumes as soon as its
        // references exist again.
        let (custom_rules, dropped_custom): (Vec<Value>, Vec<String>) = input
            .rule_overrides
            .custom
            .iter()
            .filter(|r| !input.rule_overrides.is_disabled(&rule_fingerprint(r)))
            .fold(
                (Vec::new(), Vec::new()),
                |(mut usable, mut dropped), rule| {
                    if custom_rule_is_usable(rule, &tag_set, &sub_set_tags) {
                        usable.push(rule.clone());
                    } else {
                        dropped.push(serde_json::to_string(rule).unwrap_or_default());
                    }
                    (usable, dropped)
                },
            );
        if !dropped_custom.is_empty() {
            tracing::warn!(
                items = %dropped_custom.join(","),
                "custom rules reference outbounds / rule-sets missing from the active subscription; skipped"
            );
        }
        // Custom rules are expanded the same way (they are persisted verbatim, so a
        // rule written before the add-time validation may still carry `geoip`), and
        // `geosite` is dropped in both paths.
        let (custom_rules, all_sets) = expand_geoip_rules(
            &custom_rules,
            &sub_sets,
            input.geoip_rule_set_dir.as_deref(),
        );
        final_rules.extend(custom_rules);
        final_rules.extend(sub_rules);
        (final_rules, all_sets)
    };
    if !final_rules.is_empty() {
        route
            .as_object_mut()
            .unwrap()
            .insert("rules".into(), Value::Array(final_rules));
    }
    if !rule_sets.is_empty() {
        route
            .as_object_mut()
            .unwrap()
            .insert("rule_set".into(), Value::Array(rule_sets));
    }

    let mut dns = input.profile.dns.clone().unwrap_or_else(minimal_dns_block);

    // Legacy profiles parsed before the `dns.listen` removal may still carry the
    // internal listen key; strip it defensively so cached profiles keep building.
    if let Some(dns_obj) = dns.as_object_mut() {
        dns_obj.remove("__ice_dns_listen");
    }

    // sing-box 1.12+: domain addresses must be resolved via a domain resolver. When a
    // `local` DNS server is present (always true for the minimal block, and ensured by
    // the clash parser for domain nameservers / fake-ip filters), wire it up as the
    // route default so outbound dials can resolve hosts.
    if dns_has_local_server(&dns) {
        route
            .as_object_mut()
            .unwrap()
            .insert("default_domain_resolver".into(), json!("local"));
    }

    validate_route_refs(&route, &tag_set)?;

    let mut inbounds = vec![json!({
        "type": "mixed",
        "tag": "mixed-in",
        "listen": if input.template.allow_lan {
            "0.0.0.0"
        } else {
            input.template.mixed_listen.as_str()
        },
        "listen_port": input.template.mixed_port,
    })];
    if capture_intent == CaptureIntent::Tun {
        inbounds.push(tun_inbound(&input.template.tun));
    }

    let config = json!({
        "log": { "level": "info", "timestamp": true },
        "dns": dns,
        "inbounds": inbounds,
        "outbounds": outbounds,
        "route": route,
        "experimental": {
            "clash_api": {
                "external_controller": format!(
                    "{}:{}",
                    input.template.clash_api_listen, input.template.clash_api_port
                ),
                // Slice 4c: runtime mode switch surface. `default_mode` is baked from
                // settings.proxy_mode and restored on every apply/restart because the
                // config is rebuilt on apply. `experimental.cache_file` must stay OFF so
                // the cached mode cannot override `default_mode` on restart.
                // NOTE: `mode_list` must NOT be emitted — the pinned sing-box 1.13.19
                // rejects it ("unknown field"). The runtime mode-list is `[<default_mode>]`
                // only, so a PATCH to another mode is silently ignored and mode switching
                // always takes the rebuild + reload path (see `orchestrate_set_proxy_mode`).
                "default_mode": clash_mode_name(input.template.proxy_mode),
            }
        }
    });

    validate_config_for_intent(&config, capture_intent)?;
    Ok(config)
}

/// Gate + TUN parameter validation shared by both builders. `Tun` configs must
/// not be generated on a platform whose T0 gate is not green, and the emitted
/// inbound needs a valid explicit interface name (locked macOS schema).
fn validate_tun_capture(template: &LocalTemplate) -> Result<(), ConfigError> {
    let gate = tun_gate();
    if !gate.ready {
        return Err(ConfigError::TunUnavailable(
            gate.reason
                .unwrap_or("TUN unavailable on this platform")
                .to_string(),
        ));
    }
    template
        .tun
        .validate()
        .map_err(|e| ConfigError::TunInvalid(e.message))?;
    if template.tun.interface_name.is_none() {
        return Err(ConfigError::TunInvalid(
            "tun.interface_name is required to generate a Tun config (platform backend resolves a free name before generation)"
                .into(),
        ));
    }
    Ok(())
}

/// Reserved bypass route rules for a `Tun` config (T0 spike §5, locked in
/// architecture §24.5.6). Order is fixed: control path and local traffic are
/// never captured or sniffed.
pub fn tun_reserved_rules() -> Vec<Value> {
    vec![
        json!({ "process_name": ["ice-box", "sing-box"], "outbound": "direct" }),
        json!({ "ip_is_private": true, "outbound": "direct" }),
        json!({
            "ip_cidr": [
                "127.0.0.0/8", "::1/128", "169.254.0.0/16",
                "224.0.0.0/4", "ff00::/8"
            ],
            "outbound": "direct"
        }),
        // The sniff action at this pin never rewrites destinations; the sniffed
        // domain lands in `metadata.Domain`, so sniff must precede every
        // domain-matching rule (T0 spike §1.1).
        json!({ "action": "sniff" }),
    ]
}

/// The locked TUN inbound shape for the bundled sing-box 1.13.19 (T0 spike §5):
/// dual-stack `address` list, sub-range auto_route, and the fixed
/// `route_exclude_address` / `loopback_address` sets. `interface_name` is
/// required at build time (validated by [`validate_tun_capture`]).
fn tun_inbound(tun: &TunSettings) -> Value {
    let mut inbound = json!({
        "type": "tun",
        "tag": "tun-in",
        "interface_name": tun.interface_name,
        "address": [tun.ipv4_address, tun.ipv6_address],
        "mtu": tun.mtu,
        "auto_route": tun.auto_route,
        "strict_route": tun.strict_route,
        "stack": tun.stack,
        "route_exclude_address": [
            "192.168.0.0/16", "10.0.0.0/8", "172.16.0.0/12",
            "127.0.0.0/8", "169.254.0.0/16", "224.0.0.0/4",
            "fe80::/10", "fc00::/7"
        ],
        "loopback_address": ["127.0.0.1", "::1"],
    });
    // `interface_name` is Some here by construction; keep the key absent if a
    // future caller relaxes the requirement.
    if tun.interface_name.is_none() {
        inbound
            .as_object_mut()
            .expect("tun inbound is an object")
            .remove("interface_name");
    }
    inbound
}

/// Expand profile `geoip` rules into local rule-set references (sing-box 1.13 removed the
/// `geoip` rule option). Rules whose `geoip-{code}.srs` file is missing from
/// `geoip_rule_set_dir` are dropped (counted via tracing warn) instead of failing the build.
/// Rules using the removed `geosite` option are dropped the same way.
fn expand_geoip_rules(
    rules: &[Value],
    rule_sets: &[Value],
    geoip_rule_set_dir: Option<&Path>,
) -> (Vec<Value>, Vec<Value>) {
    let mut kept = Vec::with_capacity(rules.len());
    let mut sets = rule_sets.to_vec();
    let mut dropped: Vec<String> = Vec::new();

    for rule in rules {
        // sing-box 1.13 also removed the `geosite` rule option; emitting it verbatim
        // would make the core exit FATAL on reload.
        if rule.get("geosite").is_some() {
            dropped.push("geosite".into());
            continue;
        }
        let Some(codes) = rule.get("geoip").and_then(|v| v.as_array()) else {
            kept.push(rule.clone());
            continue;
        };
        let codes: Vec<&str> = codes.iter().filter_map(|c| c.as_str()).collect();

        let mut resolvable = true;
        for code in &codes {
            let path = geoip_rule_set_dir
                .map(|dir| dir.join(format!("geoip-{code}.srs")))
                .filter(|p| p.is_file());
            if path.is_none() {
                dropped.push(code.to_string());
                resolvable = false;
                break;
            }
        }
        if !resolvable {
            continue;
        }

        let dir = geoip_rule_set_dir.expect("resolvable implies dir");
        for code in &codes {
            let tag = format!("geoip-{code}");
            if !sets
                .iter()
                .any(|e| e.get("tag").and_then(|v| v.as_str()) == Some(tag.as_str()))
            {
                sets.push(json!({
                    "type": "local",
                    "tag": tag,
                    "format": "binary",
                    "path": dir.join(format!("geoip-{code}.srs")),
                }));
            }
        }
        let mut converted = rule.clone();
        let obj = converted.as_object_mut().expect("geoip rule is object");
        obj.remove("geoip");
        obj.insert(
            "rule_set".into(),
            Value::Array(
                codes
                    .iter()
                    .map(|c| Value::String(format!("geoip-{c}")))
                    .collect(),
            ),
        );
        kept.push(converted);
    }

    if !dropped.is_empty() {
        dropped.sort();
        dropped.dedup();
        tracing::warn!(
            items = %dropped.join(","),
            "GEOIP rule-set files missing or removed geoip/geosite matchers; rules dropped"
        );
    }
    (kept, sets)
}

fn ensure_builtin_outbound(
    outbounds: &mut Vec<Value>,
    tags: &mut std::collections::HashSet<String>,
    tag: &str,
    value: Value,
) {
    if !tags.contains(tag) {
        tags.insert(tag.to_string());
        outbounds.push(value);
    }
}

fn validate_route_refs(
    route: &Value,
    tags: &std::collections::HashSet<String>,
) -> Result<(), ConfigError> {
    if let Some(final_tag) = route.get("final").and_then(|v| v.as_str()) {
        if final_tag != "direct" && final_tag != "block" && !tags.contains(final_tag) {
            return Err(ConfigError::RouteInvalid(format!(
                "route.final references unknown outbound: {final_tag}"
            )));
        }
    }
    if let Some(rules) = route.get("rules").and_then(|v| v.as_array()) {
        for rule in rules {
            if let Some(out) = rule.get("outbound").and_then(|v| v.as_str()) {
                if out != "direct" && out != "block" && !tags.contains(out) {
                    return Err(ConfigError::RouteInvalid(format!(
                        "route rule references unknown outbound: {out}"
                    )));
                }
            }
        }
    }
    if let Some(sets) = route.get("rule_set").and_then(|v| v.as_array()) {
        let set_tags: std::collections::HashSet<&str> = sets
            .iter()
            .filter_map(|e| e.get("tag").and_then(|v| v.as_str()))
            .collect();
        if let Some(rules) = route.get("rules").and_then(|v| v.as_array()) {
            for rule in rules {
                if let Some(refs) = rule.get("rule_set").and_then(|v| v.as_array()) {
                    for r in refs {
                        if let Some(t) = r.as_str() {
                            if !set_tags.contains(t) {
                                return Err(ConfigError::RouteInvalid(format!(
                                    "route rule references unknown rule_set: {t}"
                                )));
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// True when a custom rule only references outbounds / rule-sets that exist in
/// the current build; unknown references would otherwise fail route validation
/// and block Apply / Start after a subscription switch.
fn custom_rule_is_usable(
    rule: &Value,
    outbound_tags: &std::collections::HashSet<String>,
    rule_set_tags: &std::collections::HashSet<&str>,
) -> bool {
    if let Some(out) = rule.get("outbound").and_then(|v| v.as_str()) {
        if !outbound_tags.contains(out) {
            return false;
        }
    }
    if let Some(refs) = rule.get("rule_set").and_then(|v| v.as_array()) {
        for r in refs {
            if let Some(t) = r.as_str() {
                if !rule_set_tags.contains(t) {
                    return false;
                }
            }
        }
    }
    true
}

pub fn validate_config(config: &Value) -> Result<(), ConfigError> {
    let obj = config
        .as_object()
        .ok_or(ConfigError::Invalid("root must be an object"))?;
    if !obj.contains_key("inbounds") {
        return Err(ConfigError::Invalid("missing inbounds"));
    }
    if !obj.contains_key("outbounds") {
        return Err(ConfigError::Invalid("missing outbounds"));
    }
    Ok(())
}

/// Structural intent validation (plan §4.2.7): a `Diagnostic` config must never
/// contain a TUN inbound, and a `Tun` activation config must carry exactly one
/// TUN inbound plus the Mixed inbound. A Mixed-only config is never accepted as
/// a TUN activation config.
pub fn validate_config_for_intent(
    config: &Value,
    intent: CaptureIntent,
) -> Result<(), ConfigError> {
    validate_config(config)?;
    let inbounds = config
        .get("inbounds")
        .and_then(|v| v.as_array())
        .ok_or(ConfigError::Invalid("missing inbounds array"))?;
    let tun_count = inbounds
        .iter()
        .filter(|i| i.get("type").and_then(|v| v.as_str()) == Some("tun"))
        .count();
    let mixed_count = inbounds
        .iter()
        .filter(|i| i.get("type").and_then(|v| v.as_str()) == Some("mixed"))
        .count();
    match intent {
        CaptureIntent::Diagnostic => {
            if tun_count != 0 {
                return Err(ConfigError::Invalid(
                    "Diagnostic config must not contain a tun inbound",
                ));
            }
        }
        CaptureIntent::Tun => {
            if tun_count != 1 {
                return Err(ConfigError::Invalid(
                    "Tun config must contain exactly one tun inbound",
                ));
            }
            if mixed_count != 1 {
                return Err(ConfigError::Invalid(
                    "Tun config must keep the mixed inbound (diagnostic access)",
                ));
            }
        }
    }
    Ok(())
}

pub fn config_to_pretty_json(config: &Value) -> Result<String, ConfigError> {
    Ok(serde_json::to_string_pretty(config)?)
}

/// Write `config.json`, moving any previous file to `config.json.bak`.
pub fn write_runtime_config_file(
    config_path: &Path,
    bak_path: &Path,
    config: &Value,
) -> Result<(), ConfigError> {
    let rendered = config_to_pretty_json(config)?;
    write_runtime_config_bytes(config_path, bak_path, &rendered)
}

/// Write pre-rendered config text to `config.json`, moving any previous file
/// to `config.json.bak`. Callers that already serialized for change detection
/// skip a second serialization.
pub fn write_runtime_config_bytes(
    config_path: &Path,
    bak_path: &Path,
    rendered: &str,
) -> Result<(), ConfigError> {
    if config_path.exists() {
        if let Some(parent) = bak_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(config_path, bak_path)?;
    }
    write_bytes_atomic(config_path, rendered.as_bytes())
}

/// Restore `config.json` from `config.json.bak` after a failed reload (architecture §8.3).
/// Returns `true` when the backup file existed and was copied.
pub fn restore_runtime_config_from_bak(
    config_path: &Path,
    bak_path: &Path,
) -> Result<bool, ConfigError> {
    if !bak_path.is_file() {
        return Ok(false);
    }
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(bak_path, config_path)?;
    Ok(true)
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("no outbounds to build config from")]
    EmptyOutbounds,
    #[error("invalid config: {0}")]
    Invalid(&'static str),
    #[error("invalid route: {0}")]
    RouteInvalid(String),
    #[error("tun unavailable: {0}")]
    TunUnavailable(String),
    #[error("invalid tun settings: {0}")]
    TunInvalid(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod build_tests {
    use super::*;

    fn socks(tag: &str) -> NormalizedOutbound {
        NormalizedOutbound {
            tag: tag.into(),
            outbound: json!({
                "type": "socks",
                "tag": tag,
                "server": "127.0.0.1",
                "server_port": 1080
            }),
        }
    }

    /// Extract the two prepended `clash_mode` rules as (mode, outbound) pairs, in order.
    fn clash_rules(cfg: &Value) -> Vec<(&str, &str)> {
        cfg["route"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| {
                let mode = r.get("clash_mode").and_then(|v| v.as_str())?;
                let outbound = r.get("outbound").and_then(|v| v.as_str())?;
                Some((mode, outbound))
            })
            .collect()
    }

    #[test]
    fn direct_only_config_has_builtin_outbounds_and_direct_final() {
        let cfg = build_direct_only_config(&LocalTemplate::default(), CaptureIntent::Diagnostic)
            .expect("build");
        let outbounds = cfg["outbounds"].as_array().unwrap();
        let tags: Vec<&str> = outbounds.iter().filter_map(|o| o["tag"].as_str()).collect();
        assert_eq!(tags, ["direct", "block"]);
        assert_eq!(cfg["route"]["final"], "direct");
        assert_eq!(cfg["inbounds"][0]["type"], "mixed");
        assert_eq!(cfg["inbounds"][0]["listen_port"], 17890);
        assert_eq!(
            clash_rules(&cfg),
            [("global", "direct"), ("direct", "direct")],
            "mode switching stays wired; every mode routes direct"
        );
        assert_eq!(cfg["experimental"]["clash_api"]["default_mode"], "Rule");
    }

    #[test]
    fn direct_only_config_validates_template() {
        let invalid = LocalTemplate {
            mixed_port: 80,
            ..LocalTemplate::default()
        };
        assert!(build_direct_only_config(&invalid, CaptureIntent::Diagnostic).is_err());
    }

    #[test]
    fn g5_8_selected_tag_missing_falls_back_to_first() {
        let cfg = build_runtime_config(&build_input_from_nodes(
            LocalTemplate::default(),
            vec![socks("a"), socks("b")],
            Some("gone".into()),
        ))
        .expect("build");
        let selector = cfg["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["tag"] == "proxy")
            .unwrap();
        assert_eq!(selector["default"], "a");
    }

    #[test]
    fn group_selection_overrides_selector_default_in_built_config() {
        let profile = NormalizedProfile {
            nodes: vec![socks("a"), socks("b")],
            groups: vec![NormalizedOutbound {
                tag: "Proxies".into(),
                outbound: json!({
                    "type": "selector",
                    "tag": "Proxies",
                    "outbounds": ["a", "b"],
                    "default": "a",
                }),
            }],
            route: Default::default(),
            dns: None,
            default_outbound: Some("Proxies".into()),
            parse_stats: ProfileParseStats::default(),
        };
        let mut selections = GroupSelections::new();
        selections.insert("Proxies".into(), "b".into());
        let cfg = build_runtime_config(&BuildInput {
            template: LocalTemplate::default(),
            profile,
            selected_tag: Some("Proxies".into()),
            geoip_rule_set_dir: None,
            group_selections: selections,
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Diagnostic,
        })
        .expect("build");
        let group = cfg["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["tag"] == "Proxies")
            .unwrap();
        assert_eq!(group["default"], "b");
    }

    #[test]
    fn g5_9_port_validation() {
        let t = LocalTemplate {
            mixed_port: 80,
            ..LocalTemplate::default()
        };
        assert!(matches!(
            validate_template(&t),
            Err(ConfigError::Invalid(_))
        ));

        let t = LocalTemplate {
            mixed_port: 19090,
            clash_api_port: 19090,
            ..LocalTemplate::default()
        };
        assert!(matches!(
            validate_template(&t),
            Err(ConfigError::Invalid(_))
        ));

        let err = build_runtime_config(&build_input_from_nodes(
            LocalTemplate {
                mixed_port: 80,
                ..LocalTemplate::default()
            },
            vec![socks("a")],
            None,
        ))
        .expect_err("low port");
        assert!(matches!(err, ConfigError::Invalid(_)));

        let err = build_runtime_config(&build_input_from_nodes(
            LocalTemplate {
                mixed_listen: "192.168.1.1".into(),
                ..LocalTemplate::default()
            },
            vec![socks("a")],
            None,
        ))
        .expect_err("non-loopback mixed");
        assert!(matches!(err, ConfigError::Invalid(_)));

        let err = build_runtime_config(&build_input_from_nodes(
            LocalTemplate {
                clash_api_listen: "0.0.0.0".into(),
                ..LocalTemplate::default()
            },
            vec![socks("a")],
            None,
        ))
        .expect_err("non-loopback clash api");
        assert!(matches!(err, ConfigError::Invalid(_)));
    }

    #[test]
    fn g5_10_empty_outbounds() {
        let err = build_runtime_config(&BuildInput {
            template: LocalTemplate::default(),
            profile: NormalizedProfile::from_nodes_only(vec![]),
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Diagnostic,
        })
        .expect_err("empty");
        assert!(matches!(err, ConfigError::EmptyOutbounds));
    }

    #[test]
    fn allow_lan_binds_0_0_0_0_and_no_dns_inbound() {
        let cfg = build_runtime_config(&build_input_from_nodes(
            LocalTemplate {
                allow_lan: true,
                ..LocalTemplate::default()
            },
            vec![socks("a")],
            None,
        ))
        .unwrap();

        assert_eq!(cfg["inbounds"][0]["listen"], "0.0.0.0");
        assert_eq!(cfg["inbounds"].as_array().unwrap().len(), 1);
        assert_eq!(cfg["route"]["default_domain_resolver"], "local");
    }

    #[test]
    fn dns_listen_key_dropped_without_dns_inbound() {
        let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
        let mut dns = json!({
            "servers": [{ "type": "local", "tag": "local" }],
            "final": "local",
        });
        dns.as_object_mut().unwrap().insert(
            "__ice_dns_listen".into(),
            json!({ "listen": "127.0.0.1", "listen_port": 7874 }),
        );
        profile.dns = Some(dns);

        let cfg = build_runtime_config(&BuildInput {
            template: LocalTemplate::default(),
            profile,
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Diagnostic,
        })
        .unwrap();

        assert_eq!(cfg["inbounds"].as_array().unwrap().len(), 1);
        assert_eq!(cfg["inbounds"][0]["listen"], "127.0.0.1");
        assert!(
            cfg["dns"].get("__ice_dns_listen").is_none(),
            "internal dns listen key must not leak into runtime config"
        );
        assert_eq!(cfg["route"]["default_domain_resolver"], "local");
    }

    fn profile_with_geoip(codes: &[&str]) -> NormalizedProfile {
        let nodes = vec![socks("a")];
        let mut profile = NormalizedProfile::from_nodes_only(nodes);
        profile.route.rules = codes
            .iter()
            .map(|c| {
                json!({
                    "geoip": [c],
                    "outbound": "direct",
                })
            })
            .collect();
        profile
    }

    #[test]
    fn geoip_rules_expand_to_local_rule_sets_when_files_present() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-geoip-ok-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("geoip-cn.srs"), b"srs").unwrap();

        let cfg = build_runtime_config(&BuildInput {
            template: LocalTemplate::default(),
            profile: profile_with_geoip(&["cn"]),
            selected_tag: None,
            geoip_rule_set_dir: Some(dir.clone()),
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Diagnostic,
        })
        .unwrap();

        let geoip_rule = cfg["route"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r.get("rule_set").is_some())
            .expect("geoip rule survives behind the clash_mode rules");
        assert_eq!(geoip_rule["rule_set"][0], "geoip-cn");
        assert!(
            geoip_rule.get("geoip").is_none(),
            "geoip option must be removed"
        );
        let set = &cfg["route"]["rule_set"][0];
        assert_eq!(set["type"], "local");
        assert_eq!(set["format"], "binary");
        assert_eq!(
            set["path"],
            serde_json::Value::String(dir.join("geoip-cn.srs").to_string_lossy().into())
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn geoip_rules_dropped_when_rule_set_file_missing() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-geoip-missing-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();

        let cfg = build_runtime_config(&BuildInput {
            template: LocalTemplate::default(),
            profile: profile_with_geoip(&["kz"]),
            selected_tag: None,
            geoip_rule_set_dir: Some(dir.clone()),
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Diagnostic,
        })
        .unwrap();

        let rules = cfg["route"]["rules"].as_array().unwrap();
        assert_eq!(
            rules.len(),
            2,
            "only the two clash_mode rules survive; the unresolvable geoip rule is dropped"
        );
        assert!(
            rules.iter().all(|r| r.get("rule_set").is_none()),
            "no rule_set references for dropped codes"
        );
        assert!(
            cfg["route"].get("rule_set").is_none(),
            "no rule_set entries for dropped codes"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn custom_geoip_expanded_and_geosite_dropped_at_build() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-custom-geo-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("geoip-cn.srs"), b"srs").unwrap();

        let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
        profile.route.rules = vec![
            json!({ "domain_suffix": ["keep.com"], "outbound": "direct" }),
            json!({ "geosite": ["google"], "outbound": "direct" }),
        ];
        let mut overrides = RuleOverrides::default();
        // A custom geoip rule persisted before add-time validation existed.
        overrides
            .custom
            .push(json!({ "geoip": ["cn"], "outbound": "direct" }));
        // A custom geosite rule is dropped too.
        overrides
            .custom
            .push(json!({ "geosite": ["netflix"], "outbound": "direct" }));

        let cfg = build_runtime_config(&BuildInput {
            template: LocalTemplate::default(),
            profile,
            selected_tag: None,
            geoip_rule_set_dir: Some(dir.clone()),
            group_selections: GroupSelections::new(),
            rule_overrides: overrides,
            capture_intent: CaptureIntent::Diagnostic,
        })
        .unwrap();

        let rules = cfg["route"]["rules"].as_array().unwrap();
        assert_eq!(
            rules.len(),
            4,
            "2 clash_mode rules + custom geoip + subscription domain rule survive"
        );
        let all = serde_json::to_string(rules).unwrap();
        assert!(
            !all.contains("geosite"),
            "geosite rules must be dropped: {all}"
        );
        assert!(
            cfg["route"]["rule_set"]
                .as_array()
                .is_some_and(|s| s.iter().any(|e| e["tag"] == "geoip-cn")),
            "custom geoip must materialize its rule-set"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dns_block_present() {
        let cfg = build_runtime_config(&build_input_from_nodes(
            LocalTemplate::default(),
            vec![socks("a")],
            None,
        ))
        .unwrap();
        assert_eq!(cfg["dns"]["final"], "local");
        assert_eq!(cfg["dns"]["servers"][0]["type"], "local");
    }

    #[test]
    fn disabled_rules_dropped_and_custom_rules_prepended() {
        let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
        profile.route.rules = vec![
            json!({ "domain_suffix": ["keep.com"], "outbound": "direct" }),
            json!({ "domain_suffix": ["drop.com"], "outbound": "direct" }),
        ];
        let drop_fp = rule_fingerprint(&profile.route.rules[1]);
        let mut overrides = RuleOverrides::default();
        overrides.set_disabled(drop_fp, true);
        overrides.custom.push(json!({
            "domain_suffix": ["custom.com"],
            "outbound": "block",
        }));
        overrides
            .custom
            .push(json!({ "domain": ["off.com"], "outbound": "direct" }));
        let off_fp = rule_fingerprint(&overrides.custom[1]);
        overrides.set_disabled(off_fp, true);

        let cfg = build_runtime_config(&BuildInput {
            template: LocalTemplate::default(),
            profile,
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: overrides,
            capture_intent: CaptureIntent::Diagnostic,
        })
        .unwrap();

        let rules = cfg["route"]["rules"].as_array().unwrap();
        let tags: Vec<String> = rules
            .iter()
            .filter(|r| r.get("domain_suffix").is_some() || r.get("domain").is_some())
            .map(|r| {
                r["domain_suffix"][0]
                    .as_str()
                    .or_else(|| r["domain"][0].as_str())
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            tags,
            ["custom.com", "keep.com"],
            "clash_mode rules first, then custom, disabled dropped"
        );
        assert!(!tags.iter().any(|t| t == "drop.com"));
        assert!(!tags.iter().any(|t| t == "off.com"));
    }

    #[test]
    fn custom_rules_with_unknown_outbound_skipped_not_fatal() {
        let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
        profile.route.rules = vec![json!({
            "domain_suffix": ["keep.com"],
            "outbound": "direct",
        })];
        let mut overrides = RuleOverrides::default();
        overrides.custom.push(json!({
            "domain_suffix": ["ghost.com"],
            "outbound": "ghost-node",
        }));
        overrides.custom.push(json!({
            "domain_suffix": ["ok.com"],
            "outbound": "a",
        }));

        let cfg = build_runtime_config(&BuildInput {
            template: LocalTemplate::default(),
            profile,
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: overrides,
            capture_intent: CaptureIntent::Diagnostic,
        })
        .unwrap();

        let rules = cfg["route"]["rules"].as_array().unwrap();
        let kept: Vec<String> = rules
            .iter()
            .filter(|r| r.get("domain_suffix").is_some())
            .map(|r| r["domain_suffix"][0].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            kept,
            ["ok.com", "keep.com"],
            "custom rule referencing a missing outbound is skipped, not fatal"
        );
    }

    #[test]
    fn custom_rules_with_unknown_rule_set_skipped_keeps_existing() {
        let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
        profile.route.rule_sets = vec![json!({ "tag": "geoip-cn", "type": "remote" })];
        let mut overrides = RuleOverrides::default();
        overrides.custom.push(json!({
            "rule_set": ["geoip-cn"],
            "outbound": "direct",
        }));
        overrides.custom.push(json!({
            "rule_set": ["geoip-us"],
            "outbound": "direct",
        }));

        let cfg = build_runtime_config(&BuildInput {
            template: LocalTemplate::default(),
            profile,
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: overrides,
            capture_intent: CaptureIntent::Diagnostic,
        })
        .unwrap();

        let rules = cfg["route"]["rules"].as_array().unwrap();
        let kept: Vec<String> = rules
            .iter()
            .filter_map(|r| {
                r.get("rule_set")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .collect();
        assert_eq!(
            kept,
            ["geoip-cn"],
            "custom rule referencing a missing rule-set is skipped, not fatal"
        );
    }

    #[test]
    fn subscription_rule_with_unknown_outbound_still_fails() {
        let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
        profile.route.rules = vec![json!({
            "domain_suffix": ["bad.com"],
            "outbound": "ghost-node",
        })];
        let err = build_runtime_config(&BuildInput {
            template: LocalTemplate::default(),
            profile,
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Diagnostic,
        })
        .expect_err("subscription rule with unknown outbound must still fail");
        assert!(matches!(err, ConfigError::RouteInvalid(_)));
    }

    #[test]
    fn proxy_mode_rule_keeps_rules_and_subscription_final() {
        let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
        profile.route.final_outbound = "direct".into();
        profile.route.rules = vec![
            json!({ "domain_suffix": ["keep.com"], "outbound": "direct" }),
            json!({ "domain_suffix": ["proxy.com"], "outbound": "a" }),
        ];
        let cfg = build_runtime_config(&BuildInput {
            template: LocalTemplate::default(),
            profile,
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Diagnostic,
        })
        .unwrap();
        assert_eq!(cfg["route"]["final"], "direct");
        let rules = cfg["route"]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 4, "2 clash_mode rules + 2 subscription rules");
        assert_eq!(
            clash_rules(&cfg),
            [("global", "proxy"), ("direct", "direct")]
        );
    }

    #[test]
    fn proxy_mode_global_keeps_rules_with_clash_mode_global_target() {
        let profile = NormalizedProfile {
            nodes: vec![socks("a"), socks("b")],
            groups: vec![NormalizedOutbound {
                tag: "Proxies".into(),
                outbound: json!({
                    "type": "selector",
                    "tag": "Proxies",
                    "outbounds": ["a", "b"],
                    "default": "a",
                }),
            }],
            route: NormalizedRoute {
                final_outbound: "direct".into(),
                rules: vec![
                    json!({ "domain_suffix": ["keep.com"], "outbound": "direct" }),
                    json!({ "domain_suffix": ["proxy.com"], "outbound": "a" }),
                ],
                rule_sets: vec![json!({"type": "local", "tag": "set-a"})],
            },
            dns: None,
            default_outbound: Some("Proxies".into()),
            parse_stats: ProfileParseStats::default(),
        };
        let cfg = build_runtime_config(&BuildInput {
            template: LocalTemplate {
                proxy_mode: ProxyMode::Global,
                ..LocalTemplate::default()
            },
            profile,
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Diagnostic,
        })
        .unwrap();
        assert_eq!(
            cfg["route"]["final"], "direct",
            "final stays at rule-mode value"
        );
        assert_eq!(
            clash_rules(&cfg),
            [("global", "Proxies"), ("direct", "direct")],
            "global clash rule targets the top group so node selection keeps working"
        );
        assert!(
            cfg["route"]["rule_set"]
                .as_array()
                .is_some_and(|s| s.iter().any(|e| e["tag"] == "set-a")),
            "global mode must keep rule_sets"
        );
    }

    #[test]
    fn proxy_mode_global_without_groups_uses_injected_proxy_selector() {
        let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
        profile.route.final_outbound = "direct".into();
        let cfg = build_runtime_config(&BuildInput {
            template: LocalTemplate {
                proxy_mode: ProxyMode::Global,
                ..LocalTemplate::default()
            },
            profile,
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Diagnostic,
        })
        .unwrap();
        assert_eq!(cfg["route"]["final"], "direct");
        assert_eq!(
            clash_rules(&cfg)[0],
            ("global", "proxy"),
            "flat profiles route global through the injected proxy selector"
        );
        assert!(
            cfg["outbounds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|o| o["tag"] == "proxy" && o["type"] == "selector"),
            "injected proxy selector must exist"
        );
    }

    #[test]
    fn proxy_mode_direct_keeps_rules_with_clash_mode_direct() {
        let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
        profile.route.final_outbound = "a".into();
        profile.route.rules = vec![json!({ "domain_suffix": ["keep.com"], "outbound": "a" })];
        let cfg = build_runtime_config(&BuildInput {
            template: LocalTemplate {
                proxy_mode: ProxyMode::Direct,
                ..LocalTemplate::default()
            },
            profile,
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Diagnostic,
        })
        .unwrap();
        assert_eq!(cfg["route"]["final"], "a", "final stays at rule-mode value");
        assert_eq!(
            clash_rules(&cfg)[1],
            ("direct", "direct"),
            "direct clash rule routes everything direct"
        );
    }

    #[test]
    fn clash_api_block_carries_default_mode_in_all_modes() {
        for mode in [ProxyMode::Rule, ProxyMode::Global, ProxyMode::Direct] {
            let cfg = build_runtime_config(&build_input_from_nodes(
                LocalTemplate {
                    proxy_mode: mode,
                    ..LocalTemplate::default()
                },
                vec![socks("a")],
                None,
            ))
            .unwrap();
            let api = &cfg["experimental"]["clash_api"];
            assert_eq!(api["default_mode"], clash_mode_name(mode));
            assert!(
                api.get("mode_list").is_none(),
                "mode_list must not be emitted (rejected by pinned sing-box 1.13.19)"
            );
            assert!(
                cfg["experimental"].get("cache_file").is_none(),
                "cache_file must stay disabled (Slice 4c lock)"
            );
        }
    }

    #[test]
    fn restore_runtime_config_from_bak_copies_previous() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-config-bak-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let config = dir.join("config.json");
        let bak = dir.join("config.json.bak");
        fs::write(&config, b"new-bad").unwrap();
        fs::write(&bak, b"old-good").unwrap();

        assert!(restore_runtime_config_from_bak(&config, &bak).unwrap());
        assert_eq!(fs::read_to_string(&config).unwrap(), "old-good");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_runtime_config_from_bak_missing_returns_false() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-config-no-bak-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let config = dir.join("config.json");
        fs::write(&config, b"keep").unwrap();

        assert!(!restore_runtime_config_from_bak(&config, &dir.join("config.json.bak")).unwrap());
        assert_eq!(fs::read_to_string(&config).unwrap(), "keep");

        let _ = fs::remove_dir_all(&dir);
    }

    // --- Slice T1: CaptureIntent, TUN config generation, structural intent checks ---

    #[test]
    fn tun_gate_status_is_stable_per_platform() {
        let gate = tun_gate();
        #[cfg(target_os = "macos")]
        {
            assert!(gate.ready, "macos_tun_ready is green after the T0 spike");
            assert_eq!(gate.reason, None);
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert!(
                !gate.ready,
                "TUN must stay fail-closed off-macOS until its gate is green"
            );
            assert!(gate.reason.is_some());
        }
    }

    /// TUN parameters with an explicit interface name (required at build time).
    #[cfg(target_os = "macos")]
    fn tun_template() -> LocalTemplate {
        LocalTemplate {
            tun: TunSettings {
                enabled: true,
                interface_name: Some("utun420".into()),
                ..TunSettings::default()
            },
            ..LocalTemplate::default()
        }
    }

    #[test]
    fn diagnostic_intent_never_emits_tun_inbound_even_when_tun_enabled() {
        let template = LocalTemplate {
            tun: TunSettings {
                enabled: true,
                ..TunSettings::default()
            },
            ..LocalTemplate::default()
        };
        // A requested Tun config is rejected by the gate / interface-name checks on
        // non-green platforms, but Diagnostic must build identically everywhere.
        let cfg = build_runtime_config(&BuildInput {
            template: template.clone(),
            profile: NormalizedProfile::from_nodes_only(vec![socks("a")]),
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Diagnostic,
        })
        .expect("diagnostic build");
        assert_eq!(
            cfg["inbounds"].as_array().unwrap().len(),
            1,
            "tun.enabled=true must not add a tun inbound under Diagnostic intent"
        );
        assert_eq!(cfg["inbounds"][0]["type"], "mixed");

        let direct = build_direct_only_config(&template, CaptureIntent::Diagnostic)
            .expect("diagnostic direct-only");
        assert_eq!(direct["inbounds"].as_array().unwrap().len(), 1);
        validate_config_for_intent(&cfg, CaptureIntent::Diagnostic).expect("structural check");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn tun_config_has_both_inbounds_and_locked_shape() {
        let cfg = build_runtime_config(&BuildInput {
            template: tun_template(),
            profile: NormalizedProfile::from_nodes_only(vec![socks("a")]),
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Tun,
        })
        .expect("tun build");
        validate_config_for_intent(&cfg, CaptureIntent::Tun).expect("structural check");

        let inbounds = cfg["inbounds"].as_array().unwrap();
        assert_eq!(inbounds.len(), 2, "mixed + tun");
        assert_eq!(inbounds[0]["type"], "mixed");
        assert_eq!(inbounds[0]["tag"], "mixed-in");

        let tun = &inbounds[1];
        assert_eq!(tun["type"], "tun");
        assert_eq!(tun["tag"], "tun-in");
        assert_eq!(tun["interface_name"], "utun420");
        assert_eq!(tun["address"][0], "10.0.0.1/30");
        assert_eq!(tun["address"][1], "fdfe:dcba:9876::1/126");
        assert_eq!(tun["mtu"], 9000);
        assert_eq!(tun["auto_route"], true);
        assert_eq!(tun["strict_route"], true);
        assert_eq!(tun["stack"], "gvisor");
        let excludes: Vec<&str> = tun["route_exclude_address"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            excludes,
            [
                "192.168.0.0/16",
                "10.0.0.0/8",
                "172.16.0.0/12",
                "127.0.0.0/8",
                "169.254.0.0/16",
                "224.0.0.0/4",
                "fe80::/10",
                "fc00::/7"
            ],
            "route_exclude_address must match the locked T0 shape"
        );
        assert_eq!(
            tun["loopback_address"],
            serde_json::json!(["127.0.0.1", "::1"])
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn tun_config_reserved_rules_precede_clash_mode_and_sniff_precedes_domain_rules() {
        let mut profile = NormalizedProfile::from_nodes_only(vec![socks("a")]);
        profile.route.rules = vec![
            json!({ "domain_suffix": ["keep.com"], "outbound": "direct" }),
            json!({ "domain_suffix": ["proxy.com"], "outbound": "a" }),
        ];
        let cfg = build_runtime_config(&BuildInput {
            template: tun_template(),
            profile,
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Tun,
        })
        .expect("tun build");
        let rules = cfg["route"]["rules"].as_array().unwrap();
        assert_eq!(
            rules.len(),
            8,
            "4 reserved + 2 clash_mode + 2 subscription rules"
        );

        assert_eq!(rules[0]["process_name"][0], "ice-box");
        assert_eq!(rules[0]["outbound"], "direct");
        assert_eq!(rules[1]["ip_is_private"], true);
        assert_eq!(rules[1]["outbound"], "direct");
        assert_eq!(rules[2]["ip_cidr"][0], "127.0.0.0/8");
        assert_eq!(rules[3]["action"], "sniff");
        assert_eq!(rules[4]["clash_mode"], "global");
        assert_eq!(rules[5]["clash_mode"], "direct");
        assert_eq!(rules[6]["domain_suffix"][0], "keep.com");
        assert_eq!(rules[7]["domain_suffix"][0], "proxy.com");

        // Global mode must never bypass the reserved rules: the clash_mode rule
        // still targets the proxy while the control path stays direct.
        assert_eq!(rules[4]["outbound"], "proxy");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn tun_config_works_for_every_proxy_mode_and_direct_only_keeps_tun() {
        for mode in [ProxyMode::Rule, ProxyMode::Global, ProxyMode::Direct] {
            let template = LocalTemplate {
                proxy_mode: mode,
                ..tun_template()
            };
            let cfg = build_runtime_config(&BuildInput {
                template: template.clone(),
                profile: NormalizedProfile::from_nodes_only(vec![socks("a")]),
                selected_tag: None,
                geoip_rule_set_dir: None,
                group_selections: GroupSelections::new(),
                rule_overrides: RuleOverrides::default(),
                capture_intent: CaptureIntent::Tun,
            })
            .expect("tun build per mode");
            validate_config_for_intent(&cfg, CaptureIntent::Tun).expect("structural check");
            assert_eq!(
                cfg["experimental"]["clash_api"]["default_mode"],
                clash_mode_name(mode)
            );

            let direct = build_direct_only_config(&template, CaptureIntent::Tun)
                .expect("tun direct-only per mode");
            validate_config_for_intent(&direct, CaptureIntent::Tun)
                .expect("direct-only Tun keeps the tun inbound");
            let rules = direct["route"]["rules"].as_array().unwrap();
            assert_eq!(rules[0]["process_name"][0], "ice-box");
            assert_eq!(
                clash_rules(&direct),
                [("global", "direct"), ("direct", "direct")]
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn tun_config_requires_interface_name_at_build_time() {
        let template = LocalTemplate {
            tun: TunSettings {
                enabled: true,
                interface_name: None,
                ..TunSettings::default()
            },
            ..LocalTemplate::default()
        };
        let err = build_runtime_config(&BuildInput {
            template: template.clone(),
            profile: NormalizedProfile::from_nodes_only(vec![socks("a")]),
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Tun,
        })
        .expect_err("interface name required");
        assert!(matches!(err, ConfigError::TunInvalid(_)));
        assert!(build_direct_only_config(&template, CaptureIntent::Tun).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn tun_config_rejects_invalid_mtu_and_address_at_build_time() {
        let bad_mtu = LocalTemplate {
            tun: TunSettings {
                mtu: 576,
                interface_name: Some("utun420".into()),
                ..TunSettings::default()
            },
            ..tun_template()
        };
        let err = build_runtime_config(&BuildInput {
            template: bad_mtu,
            profile: NormalizedProfile::from_nodes_only(vec![socks("a")]),
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Tun,
        })
        .expect_err("bad mtu");
        assert!(matches!(err, ConfigError::TunInvalid(_)));

        let bad_addr = LocalTemplate {
            tun: TunSettings {
                ipv6_address: "10.0.0.1/24".into(),
                interface_name: Some("utun420".into()),
                ..TunSettings::default()
            },
            ..tun_template()
        };
        assert!(build_direct_only_config(&bad_addr, CaptureIntent::Tun).is_err());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn tun_intent_is_rejected_on_platforms_without_a_green_gate() {
        // Windows gate pending / Linux out of scope: Tun generation must fail
        // closed with the stable unavailable error, never emit a tun inbound.
        let template = LocalTemplate {
            tun: TunSettings {
                enabled: true,
                interface_name: Some("utun420".into()),
                ..TunSettings::default()
            },
            ..LocalTemplate::default()
        };
        let err = build_runtime_config(&BuildInput {
            template: template.clone(),
            profile: NormalizedProfile::from_nodes_only(vec![socks("a")]),
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Tun,
        })
        .expect_err("tun gate not green");
        assert!(matches!(err, ConfigError::TunUnavailable(_)));
        assert!(build_direct_only_config(&template, CaptureIntent::Tun).is_err());
    }

    #[test]
    fn validate_config_for_intent_rejects_intent_mismatch_everywhere() {
        // Platform-neutral structural checks: hand-built JSON, no builder gate.
        let mixed_only = json!({
            "inbounds": [ { "type": "mixed", "tag": "mixed-in" } ],
            "outbounds": [ { "type": "direct", "tag": "direct" } ],
        });
        let mixed_tun = json!({
            "inbounds": [
                { "type": "mixed", "tag": "mixed-in" },
                { "type": "tun", "tag": "tun-in" }
            ],
            "outbounds": [ { "type": "direct", "tag": "direct" } ],
        });
        let tun_only = json!({
            "inbounds": [ { "type": "tun", "tag": "tun-in" } ],
            "outbounds": [ { "type": "direct", "tag": "direct" } ],
        });
        let two_tun = json!({
            "inbounds": [
                { "type": "mixed", "tag": "mixed-in" },
                { "type": "tun", "tag": "tun-in" },
                { "type": "tun", "tag": "tun-in-2" }
            ],
            "outbounds": [ { "type": "direct", "tag": "direct" } ],
        });

        validate_config_for_intent(&mixed_only, CaptureIntent::Diagnostic).expect("mixed-only");
        validate_config_for_intent(&mixed_tun, CaptureIntent::Tun).expect("mixed+tun");

        assert!(
            matches!(
                validate_config_for_intent(&mixed_tun, CaptureIntent::Diagnostic),
                Err(ConfigError::Invalid(_))
            ),
            "Diagnostic must never carry a tun inbound"
        );
        assert!(
            matches!(
                validate_config_for_intent(&mixed_only, CaptureIntent::Tun),
                Err(ConfigError::Invalid(_))
            ),
            "a Mixed-only config must never be handed to a TUN activation"
        );
        assert!(
            matches!(
                validate_config_for_intent(&tun_only, CaptureIntent::Tun),
                Err(ConfigError::Invalid(_))
            ),
            "Tun config must keep the mixed inbound"
        );
        assert!(
            matches!(
                validate_config_for_intent(&two_tun, CaptureIntent::Tun),
                Err(ConfigError::Invalid(_))
            ),
            "exactly one tun inbound"
        );
    }

    #[test]
    fn build_input_serde_preserves_capture_intent_and_defaults_to_diagnostic() {
        let value = serde_json::to_value(BuildInput {
            template: LocalTemplate::default(),
            profile: NormalizedProfile::from_nodes_only(vec![socks("a")]),
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Tun,
        })
        .expect("serialize");
        assert_eq!(value["capture_intent"], "tun");

        let legacy = serde_json::json!({
            "template": LocalTemplate::default(),
            "profile": NormalizedProfile::from_nodes_only(vec![socks("a")]),
            "selected_tag": null,
            "geoip_rule_set_dir": null,
            "group_selections": {},
            "rule_overrides": {},
        });
        let parsed: BuildInput = serde_json::from_value(legacy).expect("legacy input");
        assert_eq!(parsed.capture_intent, CaptureIntent::Diagnostic);
        assert_eq!(parsed.template.tun, TunSettings::default());
    }
}
