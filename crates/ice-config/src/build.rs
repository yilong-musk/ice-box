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
use crate::{tun_gate_for, BuildInput, CaptureIntent, HostPlatform, LocalTemplate};

/// Connection routes are INFO. The Logs page filters to those plus
/// important events unless debug mode is on; the 20 MiB cap bounds disk.
const GENERATED_LOG_LEVEL: &str = "info";

pub fn build_direct_only_config(
    template: &LocalTemplate,
    capture_intent: CaptureIntent,
    platform: HostPlatform,
) -> Result<Value, ConfigError> {
    validate_template(template)?;
    if capture_intent == CaptureIntent::Tun {
        validate_tun_capture(template, platform)?;
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
        rules.extend(tun_reserved_rules(&template.tun, platform));
    }
    rules.push(json!({ "clash_mode": "global", "outbound": "direct" }));
    rules.push(json!({ "clash_mode": "direct", "outbound": "direct" }));
    let route = json!({
        "final": "direct",
        "auto_detect_interface": true,
        "rules": rules,
        "default_domain_resolver": minimal_default_domain_resolver(platform),
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
        "log": { "level": GENERATED_LOG_LEVEL, "timestamp": true },
        "dns": minimal_dns_block(platform),
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
    if capture_intent == CaptureIntent::Tun {
        validate_tun_capture(&input.template, input.platform)?;
    }

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
        if capture_intent == CaptureIntent::Tun {
            // Reserved bypass rules precede `clash_mode` (T0 lock, §24.5.6):
            // the control path, private/loopback/link-local/multicast
            // destinations, and the TUN endpoint are never captured or
            // sniffed, even in Global/Direct mode.
            final_rules.extend(tun_reserved_rules(&input.template.tun, input.platform));
        }
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

    // sing-box 1.12+: domain addresses must be resolved via a domain resolver.
    // macOS: a `local` DNS server (always present for the minimal block, and
    // ensured by the clash parser for domain nameservers / fake-ip filters)
    // backs the OS resolver. Windows: `local` re-enters the TUN, so the route
    // default points at the DNS `final` tag — guaranteed TCP-capable by the
    // Windows emission (no UDP upstreams, no fakeip).
    if input.platform.is_windows() {
        if let Some(final_tag) = dns_final_tag(&dns) {
            route
                .as_object_mut()
                .unwrap()
                .insert("default_domain_resolver".into(), json!(final_tag));
        }
    } else if dns_has_local_server(&dns) {
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

    let experimental = json!({
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
    });

    let config = RuntimeConfig {
        log: json!({ "level": GENERATED_LOG_LEVEL, "timestamp": true }),
        dns,
        inbounds,
        outbounds,
        route,
        experimental,
    };

    validate_runtime_config_for_intent(&config, capture_intent)?;
    Ok(config)
}

/// Gate + TUN parameter validation shared by both builders. `Tun` configs must
/// not be generated on a platform whose T0 gate is not green, and the emitted
/// inbound needs a valid explicit interface name (locked macOS schema).
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
    if template.tun.interface_name.is_none() {
        return Err(ConfigError::TunInvalid(
            "tun.interface_name is required to generate a Tun config (platform backend resolves a free name before generation)"
                .into(),
        ));
    }
    Ok(())
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

/// Reserved bypass route rules for a `Tun` config (locked in architecture
/// §24.5.6 and `docs/tun.md`). Order is fixed: control path and local traffic
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
    // domain-matching rule (T0 spike §1.1).
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
        .ok_or_else(|| ConfigError::invalid("/: root must be an object"))?;
    let inbounds = obj
        .get("inbounds")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ConfigError::invalid("/inbounds: missing or not an array"))?;
    let outbounds = obj
        .get("outbounds")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ConfigError::invalid("/outbounds: missing or not an array"))?;
    validate_config_parts(
        inbounds,
        outbounds.iter(),
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
    let bytes = fs::read(bak_path)?;
    write_bytes_atomic(config_path, &bytes)?;
    Ok(true)
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
