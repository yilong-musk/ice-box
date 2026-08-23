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
pub use settings::{load_settings, save_settings, AppSettings, ProxyMode};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

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
    }
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

    let route_final = match input.template.proxy_mode {
        ProxyMode::Rule => {
            if input.profile.route.final_outbound == "proxy" && input.profile.groups.is_empty() {
                "proxy".to_string()
            } else {
                input.profile.route.final_outbound.clone()
            }
        }
        // Global: ignore rules, send everything through the selected proxy / top group.
        // With no groups the injected `proxy` selector is the natural target so homepage
        // node selection keeps working.
        ProxyMode::Global => {
            if input.profile.groups.is_empty() {
                "proxy".to_string()
            } else {
                fallback.clone()
            }
        }
        // Direct: ignore rules, send everything out `direct`.
        ProxyMode::Direct => "direct".to_string(),
    };

    let mut route = json!({
        "final": route_final,
        "auto_detect_interface": true,
    });
    // Disabled (fingerprint-matched) subscription rules are dropped; custom rules are
    // prepended so they take priority over subscription rules. In global / direct mode
    // all rules are stripped (route rules and rule_sets).
    let rule_mode = matches!(input.template.proxy_mode, ProxyMode::Rule);
    let mut final_rules: Vec<Value> = Vec::new();
    let mut rule_sets: Vec<Value> = Vec::new();
    if rule_mode {
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
        let custom_rules: Vec<Value> = input
            .rule_overrides
            .custom
            .iter()
            .filter(|r| !input.rule_overrides.is_disabled(&rule_fingerprint(r)))
            .cloned()
            .collect();
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
        rule_sets = all_sets;
    }
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

    let inbounds = vec![json!({
        "type": "mixed",
        "tag": "mixed-in",
        "listen": if input.template.allow_lan {
            "0.0.0.0"
        } else {
            input.template.mixed_listen.as_str()
        },
        "listen_port": input.template.mixed_port,
    })];

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
            }
        }
    });

    validate_config(&config)?;
    Ok(config)
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

pub fn config_to_pretty_json(config: &Value) -> Result<String, ConfigError> {
    Ok(serde_json::to_string_pretty(config)?)
}

/// Write `config.json`, moving any previous file to `config.json.bak`.
pub fn write_runtime_config_file(
    config_path: &Path,
    bak_path: &Path,
    config: &Value,
) -> Result<(), ConfigError> {
    if config_path.exists() {
        if let Some(parent) = bak_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(config_path, bak_path)?;
    }
    write_json_atomic(config_path, config)?;
    Ok(())
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
        })
        .unwrap();

        let rule = &cfg["route"]["rules"][0];
        assert_eq!(rule["rule_set"][0], "geoip-cn");
        assert!(rule.get("geoip").is_none(), "geoip option must be removed");
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
        })
        .unwrap();

        assert!(
            cfg["route"]
                .get("rules")
                .and_then(|v| v.as_array())
                .is_none_or(|r| r.is_empty()),
            "unresolvable geoip rule must be dropped, not fail the build"
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
        })
        .unwrap();

        let rules = cfg["route"]["rules"].as_array().unwrap();
        assert_eq!(
            rules.len(),
            2,
            "custom geoip + subscription domain rule survive"
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
        })
        .unwrap();

        let rules = cfg["route"]["rules"].as_array().unwrap();
        let tags: Vec<String> = rules
            .iter()
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
            "custom first, disabled dropped"
        );
        assert!(!tags.iter().any(|t| t == "drop.com"));
        assert!(!tags.iter().any(|t| t == "off.com"));
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
        })
        .unwrap();
        assert_eq!(cfg["route"]["final"], "direct");
        assert_eq!(cfg["route"]["rules"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn proxy_mode_global_strips_rules_and_uses_selected_proxy() {
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
        })
        .unwrap();
        assert_eq!(cfg["route"]["final"], "Proxies");
        assert!(
            cfg["route"].get("rules").is_none(),
            "global mode must strip route rules"
        );
        assert!(
            cfg["route"].get("rule_set").is_none(),
            "global mode must strip rule_sets"
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
        })
        .unwrap();
        assert_eq!(cfg["route"]["final"], "proxy");
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
    fn proxy_mode_direct_strips_rules_and_routes_direct() {
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
        })
        .unwrap();
        assert_eq!(cfg["route"]["final"], "direct");
        assert!(
            cfg["route"].get("rules").is_none(),
            "direct mode must strip route rules"
        );
        assert!(
            cfg["route"].get("rule_set").is_none(),
            "direct mode must strip rule_sets"
        );
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
}
