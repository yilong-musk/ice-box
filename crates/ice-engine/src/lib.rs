//! Cross-platform sing-box config generation engine (facade, architecture §22).
//!
//! Single entry point for the config pipeline: subscription import → normalized
//! profile → final sing-box config. Hosts (currently the Tauri desktop shell)
//! consume this crate instead of reaching into `ice-config` / `ice-subscription`
//! directly, so the engine stays free of desktop-only concerns (process
//! lifecycle and system proxy live in `ice-core` / `ice-proxy-sys`).
//!
//! Supported platforms today: macOS / Windows. The crate has no platform
//! dependencies, keeping future mobile hosts (embedded libsing-box) viable.

pub use ice_config::{
    build_direct_only_config, build_runtime_config, clash_mode_name, config_to_pretty_json,
    minimal_dns_block, redact_config_str, rule_type_of, tun_gate, tun_reserved_rules,
    validate_config_for_intent, validate_template, AppSettings, BuildInput, CaptureIntent,
    ConfigError, GroupSelections, LocalTemplate, NormalizedOutbound, NormalizedProfile,
    NormalizedRoute, ProxyMode, RuleOverrides, TunGate, TunSettings, ENGINE_COMPAT_CORE_VERSION,
    RULE_TYPE_KEYS,
};
pub use ice_subscription::{
    apply_builtin_default_rules, detect_format, maybe_decode_base64, normalize_raw_body,
    parse_clash_profile, parse_profile, parse_singbox, parse_singbox_profile, parse_subscription,
    DirectFetcher, FetchResponse, HttpFetcher, MemorySubscriptionManager, SubscriptionError,
    SubscriptionFormat, SubscriptionIndex, SubscriptionManager, SubscriptionMeta,
    SubscriptionPaths,
};

use std::path::PathBuf;

/// Unified engine error: config build or subscription parse failures.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("config: {0}")]
    Config(#[from] ConfigError),
    #[error("subscription: {0}")]
    Subscription(#[from] SubscriptionError),
}

/// Detect + parse a raw subscription body (base64 wrapper, Clash YAML, or
/// sing-box JSON) into a normalized profile.
pub fn import_subscription(
    raw: &str,
) -> Result<(SubscriptionFormat, NormalizedProfile), EngineError> {
    Ok(normalize_raw_body(raw)?)
}

/// Build the final sing-box JSON config from a validated input.
pub fn build_config(input: &BuildInput) -> Result<serde_json::Value, EngineError> {
    Ok(build_runtime_config(input)?)
}

/// One-shot pipeline: raw subscription body → final config as pretty JSON.
///
/// `geoip_rule_set_dir` points at bundled `geoip-{code}.srs` rule-set files;
/// GEOIP rules without a matching file are dropped at build time.
/// `capture_intent` is supplied explicitly by the caller and never inferred
/// from `tun.enabled` alone (plan §4.1).
pub fn subscription_to_config(
    raw: &str,
    template: LocalTemplate,
    geoip_rule_set_dir: Option<PathBuf>,
    capture_intent: CaptureIntent,
) -> Result<String, EngineError> {
    let (_, mut profile) = normalize_raw_body(raw)?;
    // Rule-less bodies get the built-in split-routing defaults (same default
    // the desktop app applies via `auto_default_rules`).
    apply_builtin_default_rules(&mut profile);
    let input = BuildInput {
        template,
        profile,
        selected_tag: None,
        geoip_rule_set_dir,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
        capture_intent,
    };
    let config = build_runtime_config(&input)?;
    Ok(config_to_pretty_json(&config)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLASH_FIXTURE: &str = r#"
proxies:
  - name: server1
    type: ss
    server: 127.0.0.1
    port: 8388
    cipher: aes-128-gcm
    password: secret
  - name: server2
    type: vmess
    server: 127.0.0.1
    port: 8443
    uuid: 3f2e0f9a-1c5b-4e7a-9d6b-8a1c2d3e4f50
    alterId: 0
    cipher: auto
"#;

    #[test]
    fn import_clash_subscription_into_profile() {
        let (format, profile) = import_subscription(CLASH_FIXTURE).expect("import");
        assert_eq!(format, SubscriptionFormat::Clash);
        assert_eq!(profile.nodes.len(), 2);
    }

    #[test]
    fn subscription_to_config_produces_usable_json() {
        let json = subscription_to_config(
            CLASH_FIXTURE,
            LocalTemplate::default(),
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("pipeline");
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        let outbounds = value["outbounds"].as_array().expect("outbounds");
        let tags: Vec<&str> = outbounds.iter().filter_map(|o| o["tag"].as_str()).collect();
        assert!(tags.contains(&"server1"));
        assert!(tags.contains(&"server2"));
        assert_eq!(value["inbounds"][0]["type"], "mixed");
        assert_eq!(value["inbounds"][0]["listen_port"], 17890);
    }
    #[test]
    fn uri_list_subscription_through_full_pipeline() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let raw = std::fs::read_to_string(
            manifest
                .join("../..")
                .join("configs/examples/subscription-uri-list.txt"),
        )
        .expect("fixture");
        let geoip_dir = manifest.join("../../third_party/sing-geoip/rule-set");
        let json = subscription_to_config(
            &raw,
            LocalTemplate::default(),
            Some(geoip_dir),
            CaptureIntent::Diagnostic,
        )
        .expect("pipeline");
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        let outbounds = value["outbounds"].as_array().expect("outbounds");
        let types: Vec<&str> = outbounds
            .iter()
            .filter_map(|o| o["type"].as_str())
            .collect();
        for t in [
            "vless",
            "hysteria2",
            "hysteria",
            "trojan",
            "vmess",
            "tuic",
            "shadowsocks",
            "socks",
            "http",
            "wireguard",
        ] {
            assert!(types.contains(&t), "missing outbound type {t}");
        }
        let tags: Vec<&str> = outbounds.iter().filter_map(|o| o["tag"].as_str()).collect();
        assert!(tags.contains(&"日本东京01|1023.81 GB"));
        let reality = outbounds
            .iter()
            .find(|o| o["tls"]["reality"]["enabled"] == true)
            .expect("reality outbound");
        assert_eq!(
            reality["tls"]["reality"]["public_key"],
            "EYa4ic3GAxqznV61U-Oww-WKsu5wuQQptyS3fw7czM"
        );
        assert_eq!(
            value["route"]["final"], "proxy",
            "flat profiles route via the injected selector"
        );

        // Built-in split-routing rules survive into the runtime config.
        let rules = value["route"]["rules"].as_array().expect("rules");
        assert!(rules
            .iter()
            .any(|r| r["ip_is_private"] == true && r["outbound"] == "direct"));
        assert!(rules
            .iter()
            .any(|r| r["rule_set"][0] == "geoip-cn" && r["outbound"] == "direct"));
        assert!(rules
            .iter()
            .any(|r| r.get("domain_suffix").is_some() && r["outbound"] == "direct"));
        let rule_sets = value["route"]["rule_set"].as_array().expect("rule_set");
        assert!(rule_sets.iter().any(|s| s["tag"] == "geoip-cn"));
        assert_eq!(
            rule_sets.iter().find(|s| s["tag"] == "geoip-cn").unwrap()["type"],
            "local"
        );

        // Built-in DNS split routes cn domains to the domestic server.
        let dns = &value["dns"];
        let dns_servers = dns["servers"].as_array().expect("dns servers");
        assert!(dns_servers
            .iter()
            .any(|s| s["tag"] == "cn-dns" && s["server"] == "223.5.5.5"));
        assert!(dns_servers.iter().any(|s| {
            s["tag"] == "remote-dns" && s["type"] == "https" && s["detour"] == "proxy"
        }));
        assert_eq!(dns["final"], "remote-dns");
        assert!(dns["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["server"] == "cn-dns"));
        assert_eq!(value["route"]["default_domain_resolver"], "local");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn subscription_to_config_honors_tun_intent() {
        let json = subscription_to_config(
            CLASH_FIXTURE,
            LocalTemplate {
                tun: TunSettings {
                    enabled: true,
                    interface_name: Some("utun420".into()),
                    ..TunSettings::default()
                },
                ..LocalTemplate::default()
            },
            None,
            CaptureIntent::Tun,
        )
        .expect("tun pipeline");
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        validate_config_for_intent(&value, CaptureIntent::Tun).expect("intent structural check");
        assert_eq!(value["inbounds"][1]["type"], "tun");
        assert_eq!(value["inbounds"][1]["tag"], "tun-in");
        assert_eq!(value["route"]["rules"][0]["process_name"][0], "ice-box");
        assert_eq!(value["route"]["rules"][1]["action"], "hijack-dns");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn subscription_to_config_rejects_tun_intent_off_green_platforms() {
        let err = subscription_to_config(
            CLASH_FIXTURE,
            LocalTemplate {
                tun: TunSettings {
                    enabled: true,
                    interface_name: Some("utun420".into()),
                    ..TunSettings::default()
                },
                ..LocalTemplate::default()
            },
            None,
            CaptureIntent::Tun,
        )
        .expect_err("tun gate not green");
        assert!(matches!(
            err,
            EngineError::Config(ConfigError::TunUnavailable(_))
        ));
    }

    #[test]
    fn engine_exposes_tun_gate_for_preflight() {
        let gate: TunGate = tun_gate();
        #[cfg(target_os = "macos")]
        assert!(gate.ready);
        #[cfg(not(target_os = "macos"))]
        assert!(!gate.ready);
    }

    #[test]
    fn built_config_emits_clash_mode_rules_and_default_mode() {
        let value = build_config(&BuildInput {
            template: LocalTemplate {
                proxy_mode: ProxyMode::Global,
                ..LocalTemplate::default()
            },
            profile: NormalizedProfile::from_nodes_only(vec![NormalizedOutbound {
                tag: "n1".into(),
                outbound: serde_json::json!({
                    "type": "socks",
                    "tag": "n1",
                    "server": "127.0.0.1",
                    "server_port": 1080
                }),
            }]),
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Diagnostic,
        })
        .expect("build");
        let first = &value["route"]["rules"][0];
        assert_eq!(first["clash_mode"], "global");
        assert_eq!(first["outbound"], "proxy");
        assert_eq!(value["experimental"]["clash_api"]["default_mode"], "Global");
        assert!(
            value["experimental"]["clash_api"]
                .get("mode_list")
                .is_none(),
            "mode_list must not be emitted (rejected by pinned sing-box 1.13.19)"
        );
    }

    #[test]
    fn unknown_format_is_rejected() {
        let err = import_subscription("not a subscription").expect_err("unknown");
        assert!(matches!(
            err,
            EngineError::Subscription(SubscriptionError::UnknownFormat)
        ));
    }

    #[test]
    fn config_errors_are_surfaced_as_engine_errors() {
        let err = build_config(&BuildInput {
            template: LocalTemplate::default(),
            profile: NormalizedProfile::from_nodes_only(vec![]),
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Diagnostic,
        })
        .expect_err("empty profile");
        assert!(matches!(
            err,
            EngineError::Config(ConfigError::EmptyOutbounds)
        ));
    }

    #[test]
    fn compat_core_version_pin_is_locked() {
        assert_eq!(ENGINE_COMPAT_CORE_VERSION, "1.13.19");
    }
}
