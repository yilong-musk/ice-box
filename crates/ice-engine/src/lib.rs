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
    build_runtime_config, clash_mode_name, config_to_pretty_json, minimal_dns_block,
    redact_config_str, rule_type_of, validate_template, AppSettings, BuildInput, ConfigError,
    GroupSelections, LocalTemplate, NormalizedOutbound, NormalizedProfile, NormalizedRoute,
    ProxyMode, RuleOverrides, ENGINE_COMPAT_CORE_VERSION, RULE_TYPE_KEYS,
};
pub use ice_subscription::{
    detect_format, maybe_decode_base64, normalize_raw_body, parse_clash_profile, parse_profile,
    parse_singbox, parse_singbox_profile, parse_subscription, DirectFetcher, FetchResponse,
    HttpFetcher, MemorySubscriptionManager, SubscriptionError, SubscriptionFormat,
    SubscriptionIndex, SubscriptionManager, SubscriptionMeta, SubscriptionPaths,
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
pub fn subscription_to_config(
    raw: &str,
    template: LocalTemplate,
    geoip_rule_set_dir: Option<PathBuf>,
) -> Result<String, EngineError> {
    let (_, profile) = normalize_raw_body(raw)?;
    let input = BuildInput {
        template,
        profile,
        selected_tag: None,
        geoip_rule_set_dir,
        group_selections: GroupSelections::new(),
        rule_overrides: RuleOverrides::default(),
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
        let json = subscription_to_config(CLASH_FIXTURE, LocalTemplate::default(), None)
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
