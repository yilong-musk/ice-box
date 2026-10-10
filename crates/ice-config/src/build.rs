// SPDX-License-Identifier: GPL-3.0-or-later

//! Merge local template + subscription profile into a sing-box JSON config.

use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::atomic::write_bytes_atomic;
use crate::is_loopback_host;
use crate::runtime_config::{tagged_outbound, RuntimeConfig};
use crate::selections::apply_group_selections;
use crate::settings::{clash_mode_name, TunSettings};
use crate::{
    tun_gate_for, BuildInput, CaptureIntent, HostPlatform, LocalTemplate, NormalizedProfile,
};
use ice_types::is_plausible_clash_api_secret;

/// Connection routes are INFO. The Logs page filters to those plus
/// important events unless debug mode is on; the 20 MiB cap bounds disk.
const GENERATED_LOG_LEVEL: &str = "info";

/// Clash API listen + Bearer secret. `mode_list` must not be emitted — the
/// pinned sing-box 1.13.19 rejects it ("unknown field").
fn clash_api_block(template: &LocalTemplate) -> Value {
    json!({
        "external_controller": format!(
            "{}:{}",
            template.clash_api_listen, template.clash_api_port
        ),
        "secret": template.clash_api_secret,
        "default_mode": clash_mode_name(template.proxy_mode),
    })
}

pub fn build_direct_only_config(
    template: &LocalTemplate,
    capture_intent: CaptureIntent,
    platform: HostPlatform,
) -> Result<Value, ConfigError> {
    build_direct_only_config_with(template, capture_intent, platform, None, None)
}

/// Direct-only config with the mobile extras desktop callers leave unset.
///
/// `tun_exclude_package` and `shared_root` are ignored unless `platform` is
/// Android or iOS.
fn build_direct_only_config_with(
    template: &LocalTemplate,
    capture_intent: CaptureIntent,
    platform: HostPlatform,
    tun_exclude_package: Option<&str>,
    shared_root: Option<&Path>,
) -> Result<Value, ConfigError> {
    validate_template(template)?;
    prepare_capture(template, platform, capture_intent)?;

    let outbounds = vec![
        json!({"type": "direct", "tag": "direct"}),
        json!({"type": "block", "tag": "block"}),
    ];

    // Slice 4c: keep the `clash_mode` rules so a later reload to a real config keeps the
    // mode switch wired; without proxy outbounds every mode routes direct anyway. A desktop
    // `Tun` intent, and every mobile platform, prepend reserved bypass rules so the control
    // path stays direct even in direct-only fallback.
    let mut rules = Vec::new();
    prepend_capture_rules(&mut rules, &template.tun, platform, capture_intent);
    rules.push(json!({ "clash_mode": "global", "outbound": "direct" }));
    rules.push(json!({ "clash_mode": "direct", "outbound": "direct" }));
    let route = json!({
        "final": "direct",
        "auto_detect_interface": true,
        "rules": rules,
        "default_domain_resolver": minimal_default_domain_resolver(platform),
    });

    let inbounds = capture_inbounds(template, platform, capture_intent, tun_exclude_package)?;
    let mut log = json!({ "level": GENERATED_LOG_LEVEL, "timestamp": true });
    let mut experimental = json!({
        "clash_api": clash_api_block(template),
    });
    apply_shared_root(&mut log, &mut experimental, platform, shared_root);

    let config = json!({
        "log": log,
        "dns": minimal_dns_block(platform),
        "inbounds": inbounds,
        "outbounds": outbounds,
        "route": route,
        "experimental": experimental,
    });

    finish_value(&config, platform, capture_intent)?;
    Ok(config)
}

/// Mobile TUN config.
///
/// Desktop platforms are rejected. An empty profile falls back to the
/// direct-only mobile config, matching the desktop Start path. `shared_root`
/// may be absent; the caller that owns the shared directory sets it.
pub fn build_mobile_config(input: &BuildInput) -> Result<Value, ConfigError> {
    if !input.platform.is_mobile() {
        return Err(ConfigError::invalid(
            "mobile config requires HostPlatform::Android or HostPlatform::Ios",
        ));
    }
    if input.profile.nodes.is_empty() {
        return build_direct_only_config_with(
            &input.template,
            input.capture_intent,
            input.platform,
            input.tun_exclude_package.as_deref(),
            input.shared_root.as_deref(),
        );
    }
    Ok(build_runtime_config(input)?.to_json_value()?)
}

/// Validate listen ports before build.
pub fn validate_template(template: &LocalTemplate) -> Result<(), ConfigError> {
    if template.mixed_port < 1024 || template.clash_api_port < 1024 {
        return Err(ConfigError::invalid("port must be in 1024..=65535"));
    }
    if template.mixed_port == template.clash_api_port {
        return Err(ConfigError::invalid(
            "mixed port must differ from clash api port",
        ));
    }
    // With allow_lan the mixed inbound binds 0.0.0.0; the stored mixed_listen only
    // matters for loopback mode.
    if !template.allow_lan && !is_loopback_host(&template.mixed_listen) {
        return Err(ConfigError::invalid(
            "mixed_listen must be a loopback address",
        ));
    }
    if !is_loopback_host(&template.clash_api_listen) {
        return Err(ConfigError::invalid(
            "clash_api_listen must be a loopback address",
        ));
    }
    if !is_plausible_clash_api_secret(&template.clash_api_secret) {
        return Err(ConfigError::invalid(
            "clash_api_secret must be 16..=128 ASCII alphanumeric characters",
        ));
    }
    Ok(())
}

/// Minimal DNS block locked for v1 (sing-box 1.13+ local resolver).
///
/// Windows (`docs/tun.md`): the OS-resolver-backed `local` server must not
/// be used — its queries dial the TUN peer and re-enter the engine — and UDP
/// upstreams are captured by the core's own TUN. The minimal block instead
/// ships two DoT resolvers and `ipv4_only` (the IPv6 path is broken, #4178).
pub fn minimal_dns_block(platform: HostPlatform) -> Value {
    if platform.is_windows() {
        json!({
            "servers": [
                { "type": "tls", "tag": "dns-remote-0", "server": "223.5.5.5", "server_port": 853 },
                { "type": "tls", "tag": "dns-remote-1", "server": "119.29.29.29", "server_port": 853 }
            ],
            "final": "dns-remote-0",
            "strategy": "ipv4_only",
        })
    } else {
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

/// The DNS `final` tag, when present (the Windows route default).
fn dns_final_tag(dns: &Value) -> Option<String> {
    dns.get("final")
        .and_then(|v| v.as_str())
        .map(|tag| tag.to_string())
}

/// Whether the DNS server carries `tag`.
fn dns_server_has_tag(server: &Value, tag: &str) -> bool {
    server.get("tag").and_then(Value::as_str) == Some(tag)
}

/// Whether the DNS server is dialed through a proxy outbound: any `detour`
/// other than `direct` (a missing `detour` dials directly).
fn dns_server_has_proxy_dependency(server: &Value) -> bool {
    match server.get("detour") {
        None => false,
        Some(detour) => detour.as_str() != Some("direct"),
    }
}

/// Whether the DNS server answers arbitrary names from an upstream resolver.
///
/// The other allowed types answer from somewhere else — `local` (the OS
/// resolver, which on Windows re-enters the TUN), `fakeip` (synthetic
/// addresses) and `rcode` (a fixed error) — so none of them can resolve a
/// node's server domain as `route.default_domain_resolver`.
fn dns_server_resolves_upstream(server: &Value) -> bool {
    matches!(
        server.get("type").and_then(Value::as_str),
        Some("tls" | "https" | "h3" | "tcp" | "udp" | "quic")
    )
}

/// The Windows `route.default_domain_resolver` tag.
///
/// Windows has no `local` DNS server, so domain addresses in the config — the
/// node server domains above all — are resolved through a tagged DNS server.
/// That resolver must not depend on a proxy outbound: resolving a server
/// domain through a proxied upstream loops (the proxy dial needs that domain
/// resolved first) and sing-box reports
/// `DNS query loopback in transport[<tag>]`.
///
/// Returns the `final` tag when its server dials directly — the status quo —
/// otherwise the first tagged server that both dials directly and resolves
/// upstream names (the injected block's `cn-dns`). A directly-dialable server
/// of a type that cannot resolve them is skipped rather than handed the node's
/// server domain: the fallback exists to keep the resolution working, and
/// `local` / `fakeip` / `rcode` would not. When no such server exists the
/// resolver is not emitted at all, with a warning: the route default would
/// fall into the `final` tag, the loop this function exists to keep out, so
/// the key stays unset and sing-box keeps its own resolution. `None` when the
/// block has no `final` tag either.
fn windows_default_domain_resolver(dns: &Value) -> Option<String> {
    let final_tag = dns_final_tag(dns)?;
    let Some(servers) = dns.get("servers").and_then(|v| v.as_array()) else {
        return Some(final_tag);
    };
    let final_server = servers.iter().find(|s| dns_server_has_tag(s, &final_tag));
    // A `final` tag matching no server (a degenerate block; `dns_block_is_usable`
    // replaces those before this runs) keeps the pre-fix behaviour.
    let final_is_direct = match final_server {
        None => true,
        Some(server) => !dns_server_has_proxy_dependency(server),
    };
    if final_is_direct {
        return Some(final_tag);
    }
    let direct_tag = servers.iter().find_map(|s| {
        let tag = s.get("tag").and_then(Value::as_str)?;
        (dns_server_resolves_upstream(s) && !dns_server_has_proxy_dependency(s)).then_some(tag)
    });
    match direct_tag {
        Some(tag) => Some(tag.to_string()),
        None => {
            tracing::warn!(
                final_tag = %final_tag,
                "no directly-dialable upstream DNS server; the Windows resolver stays unset"
            );
            None
        }
    }
}

fn dns_block_is_usable(dns: &Value) -> bool {
    let Some(servers) = dns.get("servers").and_then(|v| v.as_array()) else {
        return false;
    };
    if servers.is_empty() {
        return false;
    }
    match dns_final_tag(dns) {
        Some(final_tag) => servers
            .iter()
            .any(|s| s.get("tag").and_then(Value::as_str) == Some(final_tag.as_str())),
        None => true,
    }
}

/// The route `default_domain_resolver` matching [`minimal_dns_block`]: `local`
/// everywhere, the DoT `final` tag on Windows.
fn minimal_default_domain_resolver(platform: HostPlatform) -> String {
    if platform.is_windows() {
        "dns-remote-0".to_string()
    } else {
        "local".to_string()
    }
}

/// Merge template + subscription profile into a sing-box config object.
pub fn build_runtime_config(input: &BuildInput) -> Result<RuntimeConfig, ConfigError> {
    validate_template(&input.template)?;
    let capture_intent = input.capture_intent;
    prepare_capture(&input.template, input.platform, capture_intent)?;

    if input.profile.nodes.is_empty() {
        return Err(ConfigError::EmptyOutbounds);
    }

    let mut tag_set: std::collections::HashSet<String> =
        input.profile.all_tags().into_iter().collect();
    let mut outbounds: Vec<Arc<Value>> = Vec::new();

    for node in &input.profile.nodes {
        outbounds.push(tagged_outbound(&node.outbound, &node.tag));
    }

    for group in &input.profile.groups {
        outbounds.push(tagged_outbound(&group.outbound, &group.tag));
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
        let is_selected_selector = ob.get("tag").and_then(|v| v.as_str())
            == Some(selected.as_str())
            && ob.get("type").and_then(|v| v.as_str()) == Some("selector");
        if !is_selected_selector {
            continue;
        }
        let members: Vec<String> = ob
            .get("outbounds")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if members.iter().any(|m| m == &selected) {
            Arc::make_mut(ob)
                .as_object_mut()
                .unwrap()
                .insert("default".into(), Value::String(selected.clone()));
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
        outbounds.push(Arc::new(json!({
            "type": "selector",
            "tag": "proxy",
            "outbounds": node_tags,
            "default": proxy_default,
        })));
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
        // Reserved bypass rules precede `clash_mode` (`docs/tun.md`): the
        // control path stays direct even in
        // Global mode. Desktop emits them only for a Tun intent; mobile
        // always does.
        prepend_capture_rules(
            &mut final_rules,
            &input.template.tun,
            input.platform,
            capture_intent,
        );
        final_rules.push(json!({ "clash_mode": "global", "outbound": global_target }));
        final_rules.push(json!({ "clash_mode": "direct", "outbound": "direct" }));
        let enabled_sub_rules: Vec<Value> = input
            .profile
            .route
            .rules
            .iter()
            .filter(|r| !input.rule_overrides.is_rule_disabled(r))
            .cloned()
            .collect();
        let (mut sub_rules, mut sub_sets) = expand_geoip_rules(
            &enabled_sub_rules,
            &input.profile.route.rule_sets,
            input.geoip_rule_set_dir.as_deref(),
        );
        ice_config_guard::retain_local_rule_sets(&mut sub_sets);
        let sub_set_tags: std::collections::HashSet<&str> = sub_sets
            .iter()
            .filter_map(|s| s.get("tag").and_then(|v| v.as_str()))
            .collect();
        sub_rules.retain(|rule| rule_set_refs_are_known(rule, &sub_set_tags));
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
            .filter(|r| !input.rule_overrides.is_rule_disabled(r))
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
        let (custom_rules, mut all_sets) = expand_geoip_rules(
            &custom_rules,
            &sub_sets,
            input.geoip_rule_set_dir.as_deref(),
        );
        ice_config_guard::retain_local_rule_sets(&mut all_sets);
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

    let mut dns = input
        .profile
        .dns
        .clone()
        .unwrap_or_else(|| minimal_dns_block(input.platform));

    // Legacy profiles parsed before the `dns.listen` removal may still carry the
    // internal listen key; strip it defensively so cached profiles keep building.
    if let Some(dns_obj) = dns.as_object_mut() {
        dns_obj.remove("__ice_dns_listen");
    }
    ice_config_guard::retain_allowed_dns_servers(&mut dns);
    if !dns_block_is_usable(&dns) {
        dns = minimal_dns_block(input.platform);
    }

    // sing-box 1.12+: domain addresses must be resolved via a domain resolver.
    // macOS: a `local` DNS server (always present for the minimal block, and
    // ensured by the clash parser for domain nameservers / fake-ip filters)
    // backs the OS resolver. Windows: `local` re-enters the TUN, so the route
    // default points at a directly-dialable DNS server instead — the `final`
    // tag when it is not detoured, else the first directly-dialable tagged
    // server — and stays unset when every server is detoured. A proxy-detoured
    // resolver (the injected `remote-dns`) would loop: the proxy dial itself
    // needs the node's server domain resolved first.
    if input.platform.is_windows() {
        if let Some(resolver) = windows_default_domain_resolver(&dns) {
            route
                .as_object_mut()
                .unwrap()
                .insert("default_domain_resolver".into(), json!(resolver));
        }
    } else if dns_has_local_server(&dns) {
        route
            .as_object_mut()
            .unwrap()
            .insert("default_domain_resolver".into(), json!("local"));
    }

    validate_route_refs(&route, &tag_set)?;

    let endpoints = split_wireguard_endpoints(&mut outbounds)?;
    let inbounds = capture_inbounds(
        &input.template,
        input.platform,
        capture_intent,
        input.tun_exclude_package.as_deref(),
    )?;
    let mut experimental = json!({
        "clash_api": clash_api_block(&input.template),
    });
    let mut log = json!({ "level": GENERATED_LOG_LEVEL, "timestamp": true });
    apply_shared_root(
        &mut log,
        &mut experimental,
        input.platform,
        input.shared_root.as_deref(),
    );

    let config = RuntimeConfig {
        log,
        dns,
        inbounds,
        outbounds,
        endpoints,
        route,
        experimental,
    };

    finish_runtime(&config, input.platform, capture_intent)?;
    Ok(config)
}

fn emit_tun(platform: HostPlatform, intent: CaptureIntent) -> bool {
    platform.is_mobile() || intent == CaptureIntent::Tun
}

fn prepare_capture(
    template: &LocalTemplate,
    platform: HostPlatform,
    intent: CaptureIntent,
) -> Result<(), ConfigError> {
    if emit_tun(platform, intent) {
        validate_tun_capture(template, platform)?;
    }
    Ok(())
}

/// Gate + TUN parameter validation shared by both builders. Desktop `Tun`
/// configs must not be generated on a platform whose T0 gate is not green,
/// and the emitted inbound needs a valid explicit interface name (locked
/// macOS schema). Mobile omits `interface_name`: libbox hands the TUN options
/// to the platform VPN.
fn validate_tun_capture(
    template: &LocalTemplate,
    platform: HostPlatform,
) -> Result<(), ConfigError> {
    let gate = tun_gate_for(platform);
    if !gate.ready {
        return Err(ConfigError::TunUnavailable(
            gate.reason
                .unwrap_or("TUN unavailable on this platform")
                .to_string(),
        ));
    }
    template
        .tun
        .validate_for(platform)
        .map_err(|e| ConfigError::TunInvalid(e.message))?;
    if !platform.is_mobile() && template.tun.interface_name.is_none() {
        return Err(ConfigError::TunInvalid(
            "tun.interface_name is required to generate a Tun config (platform backend resolves a free name before generation)"
                .into(),
        ));
    }
    Ok(())
}

fn prepend_capture_rules(
    rules: &mut Vec<Value>,
    tun: &TunSettings,
    platform: HostPlatform,
    intent: CaptureIntent,
) {
    if platform.is_mobile() {
        rules.extend(mobile_reserved_rules());
    } else if intent == CaptureIntent::Tun {
        rules.extend(tun_reserved_rules(tun, platform));
    }
}

/// Reserved rules for a mobile VPN.
///
/// Sniff runs first so later domain rules see a domain. Private, link-local,
/// and multicast destinations stay on the physical network, ahead of DNS
/// hijack, so a LAN resolver, captive portal, or printer is not captured in
/// global mode. Public DNS is still hijacked. macOS `process_name` rules and
/// the Windows peer-reject / UDP-443 rules are desktop TUN only.
fn mobile_reserved_rules() -> Vec<Value> {
    vec![
        json!({ "action": "sniff" }),
        json!({ "ip_is_private": true, "outbound": "direct" }),
        json!({
            "ip_cidr": [
                "127.0.0.0/8", "::1/128", "169.254.0.0/16",
                "224.0.0.0/4", "ff00::/8",
                "fe80::/10", "fc00::/7",
            ],
            "outbound": "direct"
        }),
        json!({ "protocol": "dns", "action": "hijack-dns" }),
    ]
}

fn mixed_inbound(template: &LocalTemplate) -> Value {
    json!({
        "type": "mixed",
        "tag": "mixed-in",
        "listen": if template.allow_lan {
            "0.0.0.0"
        } else {
            template.mixed_listen.as_str()
        },
        "listen_port": template.mixed_port,
    })
}

fn capture_inbounds(
    template: &LocalTemplate,
    platform: HostPlatform,
    intent: CaptureIntent,
    exclude_package: Option<&str>,
) -> Result<Vec<Value>, ConfigError> {
    let mut inbounds = Vec::new();
    if !platform.is_mobile() {
        inbounds.push(mixed_inbound(template));
    }
    if emit_tun(platform, intent) {
        if platform.is_mobile() {
            inbounds.push(mobile_tun_inbound(
                &template.tun,
                platform,
                exclude_package,
            )?);
        } else {
            inbounds.push(tun_inbound(&template.tun));
        }
    }
    Ok(inbounds)
}

fn apply_shared_root(
    log: &mut Value,
    experimental: &mut Value,
    platform: HostPlatform,
    shared_root: Option<&Path>,
) {
    if !platform.is_mobile() {
        return;
    }
    let Some(root) = shared_root.filter(|path| !path.as_os_str().is_empty()) else {
        return;
    };
    if let Some(obj) = log.as_object_mut() {
        obj.insert("output".into(), json!(root.join("sing-box.log")));
    }
    if let Some(obj) = experimental.as_object_mut() {
        obj.insert(
            "cache_file".into(),
            json!({
                "enabled": true,
                "path": root.join("cache.db"),
            }),
        );
    }
}

fn finish_value(
    config: &Value,
    platform: HostPlatform,
    intent: CaptureIntent,
) -> Result<(), ConfigError> {
    if platform.is_mobile() {
        let inbounds = config
            .get("inbounds")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ConfigError::invalid("/inbounds: missing inbounds array"))?;
        validate_mobile_inbounds(inbounds)?;
        validate_config(config)
    } else {
        validate_config_for_intent(config, intent)
    }
}

fn finish_runtime(
    config: &RuntimeConfig,
    platform: HostPlatform,
    intent: CaptureIntent,
) -> Result<(), ConfigError> {
    if platform.is_mobile() {
        validate_mobile_inbounds(&config.inbounds)?;
        validate_runtime_config(config)
    } else {
        validate_runtime_config_for_intent(config, intent)
    }
}

fn validate_mobile_inbounds(inbounds: &[Value]) -> Result<(), ConfigError> {
    let tun_count = inbounds
        .iter()
        .filter(|inbound| inbound.get("type").and_then(|v| v.as_str()) == Some("tun"))
        .count();
    let mixed_count = inbounds
        .iter()
        .filter(|inbound| inbound.get("type").and_then(|v| v.as_str()) == Some("mixed"))
        .count();
    if tun_count != 1 {
        return Err(ConfigError::invalid(
            "mobile config must contain exactly one tun inbound",
        ));
    }
    if mixed_count != 0 {
        return Err(ConfigError::invalid(
            "mobile config must not contain a mixed inbound",
        ));
    }
    Ok(())
}

/// Address used when a stored WireGuard node has no `local_address`.
/// sing-box 1.13 requires the endpoint `address` field.
const WIREGUARD_DEFAULT_ADDRESS: &str = "10.0.0.2/32";

fn split_wireguard_endpoints(outbounds: &mut Vec<Arc<Value>>) -> Result<Vec<Value>, ConfigError> {
    let mut kept = Vec::with_capacity(outbounds.len());
    let mut endpoints = Vec::new();
    for outbound in outbounds.iter() {
        if outbound.get("type").and_then(|v| v.as_str()) == Some("wireguard") {
            endpoints.push(wireguard_endpoint(outbound.as_ref())?);
        } else {
            kept.push(Arc::clone(outbound));
        }
    }
    *outbounds = kept;
    Ok(endpoints)
}

/// Rewrite the subscription parser's pre-1.13 WireGuard outbound into a
/// sing-box 1.13 endpoint. Selector member tags are left in place: the core
/// registers endpoints as outbounds.
fn wireguard_endpoint(outbound: &Value) -> Result<Value, ConfigError> {
    let tag = outbound
        .get("tag")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if tag.is_empty() {
        return Err(ConfigError::invalid("wireguard endpoint is missing a tag"));
    }
    let private_key = outbound
        .get("private_key")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if private_key.is_empty() {
        return Err(ConfigError::invalid(format!(
            "wireguard endpoint {tag} is missing private_key"
        )));
    }
    if outbound
        .get("peers")
        .and_then(|v| v.as_array())
        .is_some_and(|peers| !peers.is_empty())
    {
        let mut endpoint = outbound.clone();
        if let Some(obj) = endpoint.as_object_mut() {
            for key in [
                "server",
                "server_port",
                "peer_public_key",
                "local_address",
                "preshared_key",
            ] {
                obj.remove(key);
            }
            obj.insert("type".into(), json!("wireguard"));
            if !obj.contains_key("address") {
                obj.insert("address".into(), wireguard_address(outbound));
            }
        }
        return Ok(endpoint);
    }

    let server = outbound
        .get("server")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let port = outbound
        .get("server_port")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if server.is_empty() || !(1..=65535).contains(&port) {
        return Err(ConfigError::invalid(format!(
            "wireguard endpoint {tag} is missing a peer server"
        )));
    }
    let public_key = outbound
        .get("peer_public_key")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if public_key.is_empty() {
        return Err(ConfigError::invalid(format!(
            "wireguard endpoint {tag} is missing peer_public_key"
        )));
    }
    let mut peer = json!({
        "address": server,
        "port": port,
        "public_key": public_key,
        "allowed_ips": ["0.0.0.0/0", "::/0"],
    });
    if let Some(preshared) = outbound
        .get("preshared_key")
        .and_then(|v| v.as_str())
        .filter(|value| !value.is_empty())
    {
        peer["pre_shared_key"] = json!(preshared);
    }
    if let Some(reserved) = outbound.get("reserved") {
        peer["reserved"] = reserved.clone();
    }
    let mut endpoint = json!({
        "type": "wireguard",
        "tag": tag,
        "private_key": private_key,
        "address": wireguard_address(outbound),
        "peers": [peer],
    });
    if let Some(mtu) = outbound.get("mtu") {
        endpoint["mtu"] = mtu.clone();
    }
    Ok(endpoint)
}

fn wireguard_address(outbound: &Value) -> Value {
    match outbound.get("local_address") {
        Some(Value::Array(items)) if !items.is_empty() => Value::Array(items.clone()),
        Some(Value::String(addr)) if !addr.is_empty() => json!([addr]),
        _ => json!([WIREGUARD_DEFAULT_ADDRESS]),
    }
}

fn android_package_name_ok(name: &str) -> bool {
    let mut parts = name.split('.');
    let Some(first) = parts.next() else {
        return false;
    };
    if !android_ident(first) {
        return false;
    }
    let mut more = false;
    for part in parts {
        if !android_ident(part) {
            return false;
        }
        more = true;
    }
    more && name.len() <= 255
}

fn android_ident(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Same physical-network exclusions as the desktop TUN inbound. libbox turns
/// this into Android `VpnService.Builder.excludeRoute`, so those prefixes
/// never enter the tunnel.
const TUN_ROUTE_EXCLUDE_ADDRESS: &[&str] = &[
    "192.168.0.0/16",
    "10.0.0.0/8",
    "172.16.0.0/12",
    "127.0.0.0/8",
    "169.254.0.0/16",
    "224.0.0.0/4",
    "fe80::/10",
    "fc00::/7",
];

fn mobile_tun_inbound(
    tun: &TunSettings,
    platform: HostPlatform,
    exclude_package: Option<&str>,
) -> Result<Value, ConfigError> {
    let package = android_exclude_package(platform, exclude_package)?;
    let mut inbound = json!({
        "type": "tun",
        "tag": "tun-in",
        "address": [tun.ipv4_address, tun.ipv6_address],
        "mtu": tun.mtu,
        "auto_route": tun.auto_route,
        "strict_route": tun.strict_route,
        "stack": tun.stack,
        "route_exclude_address": TUN_ROUTE_EXCLUDE_ADDRESS,
    });
    if let Some(package) = package {
        inbound["exclude_package"] = json!([package]);
    }
    Ok(inbound)
}

fn android_exclude_package(
    platform: HostPlatform,
    package: Option<&str>,
) -> Result<Option<&str>, ConfigError> {
    if platform != HostPlatform::Android {
        return Ok(None);
    }
    let Some(package) = package.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if !android_package_name_ok(package) {
        return Err(ConfigError::invalid(format!(
            "tun_exclude_package is not an Android package name: {package}"
        )));
    }
    Ok(Some(package))
}

/// DNS hijack rule for a `Tun` config: port-53 traffic is diverted into the
/// sing-box DNS engine so answers come from the subscription's resolvers
/// (DoH / DoT / fake-ip) instead of a GFW-poisoned system resolver.
///
/// This replaces the TUN inbound `dns_hijack` field, which the pinned
/// sing-box 1.13.19 rejects as an unknown field (the feature moved to route
/// rule actions in sing-box 1.9).
pub fn tun_dns_hijack_rule() -> Value {
    json!({ "port": [53], "action": "hijack-dns" })
}

/// Windows TUN hijack: IPv4 destination port 53 only.
///
/// IPv6 capture is unusable on this pin (#4178); hijacking IPv6 `:53` feeds
/// broken packets into the DNS engine (`bad rdata` / `buffer size too small`).
/// Those packets hit the peer-reject rule instead.
fn windows_tun_dns_hijack_rule() -> Value {
    json!({ "ip_version": 4, "port": [53], "action": "hijack-dns" })
}

/// Reserved bypass route rules for a `Tun` config (locked in `docs/tun.md`).
/// Order is fixed: control path and local traffic
/// are never captured or sniffed.
///
/// macOS / generic shape: the `hijack-dns` rule is inserted directly after
/// the `process_name` rule when `dns_hijack` is set — the elevated core's
/// *own* DNS dials (outbound server hostnames, DoH host resolution) match
/// `process_name` and stay direct, so they never re-enter the DNS engine and
/// receive fake-ip answers; client DNS queries (browser etc.) fall through to
/// the hijack rule and are answered by the engine.
///
/// Windows shape (locked in `docs/tun.md`): IPv4 port-53 `hijack-dns` must
/// be the **first** rule — `{ "protocol": "dns" }` does not fire in time on
/// Windows (1.13 regression, #3878) — followed by the process bypass, sniff,
/// and the peer-reject rule (the TUN sub-ranges, dropped before
/// `ip_is_private` can route the TUN peer into `direct` and re-enter the
/// core; #4455 self-loop). A second `{ "protocol": "dns" }` hijack after
/// sniff is not emitted: sniff false-positives plus UDP source-port reuse
/// (#4199) would send STUN / mDNS / LLMNR into the DNS engine.
///
/// UDP user traffic (QUIC/HTTP3) is broken under the Windows shape — the
/// core's UDP outbound is captured by its own TUN (`docs/tun.md`). UDP 443 is
/// rejected with the default method (ICMP port unreachable), not
/// `outbound: block` / `method: drop`: a silent black hole leaves Chrome/Edge
/// parked on HTTP/3 (YouTube avatars on `yt3.ggpht.com` /
/// `lh3.googleusercontent.com`, Telegram Web) until the QUIC timer expires, and
/// some subresources never fall back. ICMP makes the browser fail the h3 probe
/// immediately and reuse the proven IPv4 TCP path.
pub fn tun_reserved_rules(tun: &TunSettings, platform: HostPlatform) -> Vec<Value> {
    tun_reserved_rules_for(tun, platform.is_windows())
}

/// Platform-selectable reserved-rule builder so the Windows shape is testable
/// on any host.
fn tun_reserved_rules_for(tun: &TunSettings, windows: bool) -> Vec<Value> {
    if windows {
        let mut rules = vec![windows_tun_dns_hijack_rule()];
        rules.push(json!({ "process_name": ["ice-box", "sing-box"], "outbound": "direct" }));
        rules.push(json!({ "action": "sniff" }));
        let mut tun_cidrs = Vec::new();
        if let Some(cidr) = tun_network_cidr(&tun.ipv4_address) {
            tun_cidrs.push(json!(cidr));
        }
        if let Some(cidr) = tun_network_cidr(&tun.ipv6_address) {
            tun_cidrs.push(json!(cidr));
        }
        rules.push(json!({
            "ip_cidr": tun_cidrs,
            "action": "reject",
            "method": "drop",
        }));
        rules.push(json!({
            "network": "udp",
            "port": [443],
            "action": "reject",
        }));
        rules.push(json!({ "ip_is_private": true, "outbound": "direct" }));
        rules.push(json!({
            "ip_cidr": [
                "127.0.0.0/8", "::1/128", "169.254.0.0/16",
                "224.0.0.0/4", "ff00::/8"
            ],
            "outbound": "direct"
        }));
        return rules;
    }

    let mut rules = vec![json!({ "process_name": ["ice-box", "sing-box"], "outbound": "direct" })];
    if tun.dns_hijack {
        rules.push(tun_dns_hijack_rule());
    }
    rules.push(json!({ "ip_is_private": true, "outbound": "direct" }));
    rules.push(json!({
        "ip_cidr": [
            "127.0.0.0/8", "::1/128", "169.254.0.0/16",
            "224.0.0.0/4", "ff00::/8"
        ],
        "outbound": "direct"
    }));
    // The sniff action at this pin never rewrites destinations; the sniffed
    // domain lands in `metadata.Domain`, so sniff must precede every
    // domain-matching rule (`docs/tun.md`).
    rules.push(json!({ "action": "sniff" }));
    rules
}

/// The network CIDR of a `host/prefix` address (host bits zeroed): the TUN
/// sub-range that must never be dialed back into the core.
fn tun_network_cidr(address: &str) -> Option<String> {
    let (addr, prefix_str) = address.split_once('/')?;
    let prefix: u8 = prefix_str.parse().ok()?;
    if let Ok(v4) = addr.parse::<std::net::Ipv4Addr>() {
        if prefix > 32 {
            return None;
        }
        let mask = if prefix == 0 {
            0
        } else {
            u32::MAX << (32 - prefix)
        };
        Some(format!(
            "{}/{}",
            std::net::Ipv4Addr::from(u32::from(v4) & mask),
            prefix
        ))
    } else if let Ok(v6) = addr.parse::<std::net::Ipv6Addr>() {
        if prefix > 128 {
            return None;
        }
        let mask = if prefix == 0 {
            0
        } else {
            u128::MAX << (128 - prefix)
        };
        Some(format!(
            "{}/{}",
            std::net::Ipv6Addr::from(u128::from(v6) & mask),
            prefix
        ))
    } else {
        None
    }
}

/// The locked TUN inbound shape for the bundled sing-box 1.13.19 (`docs/tun.md`):
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
        "route_exclude_address": TUN_ROUTE_EXCLUDE_ADDRESS,
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
type GeoipCodeCache = Option<(PathBuf, Option<SystemTime>, usize, HashSet<String>)>;

fn geoip_code_cache() -> &'static Mutex<GeoipCodeCache> {
    static CACHE: Mutex<GeoipCodeCache> = Mutex::new(None);
    &CACHE
}

fn geoip_codes_present(dir: &Path) -> HashSet<String> {
    let mtime = fs::metadata(dir).and_then(|m| m.modified()).ok();
    let count = fs::read_dir(dir)
        .map(|entries| entries.count())
        .unwrap_or(0);
    let mut cache = geoip_code_cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some((cached_dir, cached_mtime, cached_count, codes)) = cache.as_ref() {
        if cached_dir == dir && *cached_mtime == mtime && *cached_count == count {
            return codes.clone();
        }
    }
    let mut codes = HashSet::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(code) = name
                .strip_prefix("geoip-")
                .and_then(|rest| rest.strip_suffix(".srs"))
            {
                codes.insert(code.to_string());
            }
        }
    }
    *cache = Some((dir.to_path_buf(), mtime, count, codes.clone()));
    codes
}

/// Drop the GeoIP directory listing cache (call after copying rule-sets).
pub fn invalidate_geoip_code_cache() {
    geoip_code_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
}

fn expand_geoip_rules(
    rules: &[Value],
    rule_sets: &[Value],
    geoip_rule_set_dir: Option<&Path>,
) -> (Vec<Value>, Vec<Value>) {
    let mut kept = Vec::with_capacity(rules.len());
    let mut sets = rule_sets.to_vec();
    let mut dropped: Vec<String> = Vec::new();
    let present = geoip_rule_set_dir.map(geoip_codes_present);

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
            if present.as_ref().is_none_or(|set| !set.contains(*code)) {
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
    outbounds: &mut Vec<Arc<Value>>,
    tags: &mut std::collections::HashSet<String>,
    tag: &str,
    value: Value,
) {
    if !tags.contains(tag) {
        tags.insert(tag.to_string());
        outbounds.push(Arc::new(value));
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
    rule_set_refs_are_known(rule, rule_set_tags)
}

fn rule_set_refs_are_known(rule: &Value, rule_set_tags: &std::collections::HashSet<&str>) -> bool {
    let Some(refs) = rule.get("rule_set").and_then(|v| v.as_array()) else {
        return true;
    };
    refs.iter()
        .all(|r| r.as_str().is_some_and(|t| rule_set_tags.contains(t)))
}

pub fn validate_config(config: &Value) -> Result<(), ConfigError> {
    let obj = config
        .as_object()
        .ok_or_else(|| ConfigError::invalid("/: root must be an object"))?;
    let inbounds = obj
        .get("inbounds")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ConfigError::invalid("/inbounds: missing or not an array"))?;
    let outbounds = obj
        .get("outbounds")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ConfigError::invalid("/outbounds: missing or not an array"))?;
    let endpoints = match config.get("endpoints") {
        None => &[][..],
        Some(value) => value
            .as_array()
            .ok_or_else(|| ConfigError::invalid("/endpoints: must be an array"))?,
    };
    validate_config_parts(
        inbounds,
        outbounds.iter(),
        endpoints,
        config.pointer("/route/final").and_then(|v| v.as_str()),
        config.pointer("/route/rules").and_then(|v| v.as_array()),
        config
            .pointer("/experimental/clash_api/external_controller")
            .and_then(|v| v.as_str()),
    )
}

pub fn validate_runtime_config(config: &RuntimeConfig) -> Result<(), ConfigError> {
    validate_config_parts(
        &config.inbounds,
        config.outbounds.iter().map(|o| o.as_ref()),
        &config.endpoints,
        config.route.get("final").and_then(|v| v.as_str()),
        config.route.get("rules").and_then(|v| v.as_array()),
        config
            .experimental
            .pointer("/clash_api/external_controller")
            .and_then(|v| v.as_str()),
    )
}

fn validate_config_parts<'a, I>(
    inbounds: &[Value],
    outbounds: I,
    endpoints: &[Value],
    route_final: Option<&str>,
    route_rules: Option<&Vec<Value>>,
    clash_controller: Option<&str>,
) -> Result<(), ConfigError>
where
    I: IntoIterator<Item = &'a Value>,
{
    if inbounds.is_empty() {
        return Err(ConfigError::invalid("/inbounds: must be non-empty"));
    }
    let mut inbound_tags = std::collections::HashSet::new();
    let mut mixed_wildcard = false;
    for (idx, inbound) in inbounds.iter().enumerate() {
        let tag = inbound
            .get("tag")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if tag.is_empty() {
            return Err(ConfigError::invalid(format!(
                "/inbounds/{idx}/tag: missing"
            )));
        }
        if !inbound_tags.insert(tag.to_string()) {
            return Err(ConfigError::invalid(format!(
                "/inbounds/{idx}/tag: duplicate tag {tag}"
            )));
        }
        if inbound.get("type").and_then(|v| v.as_str()) == Some("mixed")
            && inbound.get("listen").and_then(|v| v.as_str()) == Some("0.0.0.0")
        {
            mixed_wildcard = true;
        }
    }

    let outbounds: Vec<&Value> = outbounds.into_iter().collect();
    if outbounds.is_empty() {
        return Err(ConfigError::invalid("/outbounds: must be non-empty"));
    }
    let mut outbound_tags = std::collections::HashSet::new();
    for (idx, outbound) in outbounds.iter().enumerate() {
        let tag = outbound
            .get("tag")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if tag.is_empty() {
            return Err(ConfigError::invalid(format!(
                "/outbounds/{idx}/tag: missing"
            )));
        }
        if !outbound_tags.insert(tag.to_string()) {
            return Err(ConfigError::invalid(format!(
                "/outbounds/{idx}/tag: duplicate tag {tag}"
            )));
        }
    }
    for (idx, endpoint) in endpoints.iter().enumerate() {
        let tag = endpoint
            .get("tag")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if tag.is_empty() {
            return Err(ConfigError::invalid(format!(
                "/endpoints/{idx}/tag: missing"
            )));
        }
        if !outbound_tags.insert(tag.to_string()) {
            return Err(ConfigError::invalid(format!(
                "/endpoints/{idx}/tag: duplicate tag {tag}"
            )));
        }
    }

    if let Some(final_tag) = route_final {
        if !outbound_tags.contains(final_tag) {
            return Err(ConfigError::invalid(format!(
                "/route/final: unknown outbound {final_tag}"
            )));
        }
    }
    if let Some(rules) = route_rules {
        for (idx, rule) in rules.iter().enumerate() {
            if let Some(out) = rule.get("outbound").and_then(|v| v.as_str()) {
                if !outbound_tags.contains(out) {
                    return Err(ConfigError::invalid(format!(
                        "/route/rules/{idx}/outbound: unknown outbound {out}"
                    )));
                }
            }
        }
    }

    for (idx, outbound) in outbounds.iter().enumerate() {
        let ty = outbound.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if !matches!(ty, "selector" | "urltest") {
            continue;
        }
        let Some(members) = outbound.get("outbounds").and_then(|v| v.as_array()) else {
            continue;
        };
        for (j, member) in members.iter().enumerate() {
            let Some(tag) = member.as_str() else {
                continue;
            };
            if !outbound_tags.contains(tag) {
                return Err(ConfigError::invalid(format!(
                    "/outbounds/{idx}/outbounds/{j}: unknown outbound {tag}"
                )));
            }
        }
    }

    if let Some(controller) = clash_controller {
        let host = controller
            .rsplit_once(':')
            .map(|(h, _)| h)
            .unwrap_or(controller);
        let host = host.trim().trim_matches(|c| c == '[' || c == ']');
        if !mixed_wildcard && !is_loopback_host(host) {
            return Err(ConfigError::invalid(format!(
                "/experimental/clash_api/external_controller: must be loopback, got {controller}"
            )));
        }
    }
    Ok(())
}

fn validate_intent_inbounds(inbounds: &[Value], intent: CaptureIntent) -> Result<(), ConfigError> {
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
                return Err(ConfigError::invalid(
                    "Diagnostic config must not contain a tun inbound",
                ));
            }
        }
        CaptureIntent::Tun => {
            if tun_count != 1 {
                return Err(ConfigError::invalid(
                    "Tun config must contain exactly one tun inbound",
                ));
            }
            if mixed_count != 1 {
                return Err(ConfigError::invalid(
                    "Tun config must keep the mixed inbound (diagnostic access)",
                ));
            }
        }
    }
    Ok(())
}

/// Structural intent validation (`docs/tun.md`): a `Diagnostic` config must never
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
        .ok_or_else(|| ConfigError::invalid("/inbounds: missing inbounds array"))?;
    validate_intent_inbounds(inbounds, intent)
}

pub fn validate_runtime_config_for_intent(
    config: &RuntimeConfig,
    intent: CaptureIntent,
) -> Result<(), ConfigError> {
    validate_runtime_config(config)?;
    validate_intent_inbounds(&config.inbounds, intent)
}

pub fn config_to_pretty_json<T: serde::Serialize>(config: &T) -> Result<String, ConfigError> {
    Ok(serde_json::to_string_pretty(config)?)
}

/// Write `config.json`, moving any previous file to `config.json.bak`.
pub fn write_runtime_config_file<T: serde::Serialize>(
    config_path: &Path,
    bak_path: &Path,
    config: &T,
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
        let bytes = fs::read(config_path)?;
        write_bytes_atomic(bak_path, &bytes)?;
    }
    write_bytes_atomic(config_path, rendered.as_bytes())
}

/// Restore `config.json` from `config.json.bak` after a failed reload.
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
    let bytes = fs::read(bak_path)?;
    write_bytes_atomic(config_path, &bytes)?;
    Ok(true)
}

/// Portable sing-box document for one subscription.
///
/// Outbounds, WireGuard endpoints, route, and DNS travel with the copy.
/// Local inbounds, the Clash API, and log paths stay behind: they belong to
/// this install. `geosite` and `geoip` rules are omitted because sing-box
/// 1.13 rejects those fields, and the bundled replacements are files on this
/// machine. Credentials are left intact so the copy can be imported elsewhere.
pub fn share_singbox_config(profile: &NormalizedProfile) -> Result<String, ConfigError> {
    if profile.nodes.is_empty() {
        return Err(ConfigError::EmptyOutbounds);
    }

    let mut tag_set: HashSet<String> = profile.all_tags().into_iter().collect();
    let mut outbounds: Vec<Arc<Value>> = Vec::new();
    for node in &profile.nodes {
        outbounds.push(tagged_outbound(&node.outbound, &node.tag));
    }
    for group in &profile.groups {
        outbounds.push(tagged_outbound(&group.outbound, &group.tag));
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
    if profile.groups.is_empty() {
        let node_tags: Vec<String> = profile.nodes.iter().map(|n| n.tag.clone()).collect();
        let proxy_default = node_tags[0].clone();
        outbounds.push(Arc::new(json!({
            "type": "selector",
            "tag": "proxy",
            "outbounds": node_tags,
            "default": proxy_default,
        })));
        tag_set.insert("proxy".into());
    }

    let final_outbound = {
        let candidate = profile.route.final_outbound.as_str();
        if candidate == "direct" || candidate == "block" || tag_set.contains(candidate) {
            candidate.to_string()
        } else if tag_set.contains("proxy") {
            "proxy".to_string()
        } else {
            "direct".to_string()
        }
    };

    let set_tags: HashSet<&str> = profile
        .route
        .rule_sets
        .iter()
        .filter_map(|set| set.get("tag").and_then(|value| value.as_str()))
        .collect();
    let rules: Vec<Value> = profile
        .route
        .rules
        .iter()
        .filter(|rule| share_rule_is_portable(rule, &tag_set, &set_tags))
        .cloned()
        .collect();

    let mut route = json!({ "final": final_outbound });
    if !rules.is_empty() {
        route
            .as_object_mut()
            .expect("route object")
            .insert("rules".into(), Value::Array(rules));
    }
    if !profile.route.rule_sets.is_empty() {
        route.as_object_mut().expect("route object").insert(
            "rule_set".into(),
            Value::Array(profile.route.rule_sets.clone()),
        );
    }

    let dns = if let Some(raw) = &profile.dns {
        let mut dns = raw.clone();
        if let Some(obj) = dns.as_object_mut() {
            obj.remove("__ice_dns_listen");
        }
        ice_config_guard::retain_allowed_dns_servers(&mut dns);
        if dns_block_is_usable(&dns) {
            if dns_has_local_server(&dns) {
                route
                    .as_object_mut()
                    .expect("route object")
                    .insert("default_domain_resolver".into(), json!("local"));
            }
            Some(dns)
        } else {
            None
        }
    } else {
        None
    };

    validate_route_refs(&route, &tag_set)?;
    let endpoints = split_wireguard_endpoints(&mut outbounds)?;

    let mut root = serde_json::Map::new();
    if let Some(dns) = dns {
        root.insert("dns".into(), dns);
    }
    root.insert(
        "outbounds".into(),
        Value::Array(
            outbounds
                .into_iter()
                .map(|item| item.as_ref().clone())
                .collect(),
        ),
    );
    if !endpoints.is_empty() {
        root.insert("endpoints".into(), Value::Array(endpoints));
    }
    root.insert("route".into(), route);
    Ok(serde_json::to_string_pretty(&Value::Object(root))?)
}

/// Rules that sing-box 1.13 can load without this machine's bundled files.
fn share_rule_is_portable(rule: &Value, tags: &HashSet<String>, set_tags: &HashSet<&str>) -> bool {
    if rule.get("geosite").is_some() || rule.get("geoip").is_some() {
        return false;
    }
    if let Some(outbound) = rule.get("outbound").and_then(|value| value.as_str()) {
        if outbound != "direct" && outbound != "block" && !tags.contains(outbound) {
            return false;
        }
    }
    rule_set_refs_are_known(rule, set_tags)
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("no outbounds to build config from")]
    EmptyOutbounds,
    #[error("invalid config: {0}")]
    Invalid(String),
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

impl ConfigError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}

#[cfg(test)]
pub(crate) fn build_runtime_json(input: &BuildInput) -> Result<Value, ConfigError> {
    Ok(build_runtime_config(input)?.to_json_value()?)
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod build_tests;
