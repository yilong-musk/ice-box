// SPDX-License-Identifier: GPL-3.0-or-later

//! Phase 0 spike: the host-process half of the Android client.
//!
//! Exposes the ice-box engine to Kotlin over JNI: subscription fetch through
//! the desktop `DirectFetcher` (rustls + platform verifier), config generation
//! through `ice-engine`, a mobile TUN rewrite of the generated config, and
//! Clash API checks through `ice-core`. The tunnel process never loads this
//! library.

use std::path::{Path, PathBuf};

use ice_core::HealthEndpoints;
use ice_engine::{
    build_direct_only_config, subscription_to_config, CaptureIntent, DirectFetcher, HostPlatform,
    HttpFetcher, LocalTemplate,
};
use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JObject, JString};
use jni::{Env, EnvUnowned};
use serde_json::{json, Value};

const CLASH_HOST: &str = "127.0.0.1";
const CLASH_PORT: u16 = 19090;
pub const TUN_IPV4: &str = "172.19.0.1/30";
pub const TUN_IPV6: &str = "fdfe:dcba:9876::1/126";

type SpikeResult<T> = Result<T, String>;

fn template(secret: &str) -> LocalTemplate {
    LocalTemplate {
        clash_api_listen: CLASH_HOST.into(),
        clash_api_port: CLASH_PORT,
        clash_api_secret: secret.into(),
        ..LocalTemplate::default()
    }
}

/// Rewrite a Diagnostic (Mixed-only) config into the mobile TUN shape the
/// design doc proposes: no Mixed inbound, a TUN inbound without
/// `interface_name`, DNS hijack, interface auto-detection (socket protect),
/// and cache / log files under the shared root.
pub fn mobilize(config: &mut Value, shared_dir: &Path) {
    let inbounds = config["inbounds"].as_array_mut().expect("inbounds");
    inbounds.retain(|inbound| inbound["type"] != "mixed");
    inbounds.push(json!({
        "type": "tun",
        "tag": "tun-in",
        "address": [TUN_IPV4, TUN_IPV6],
        "mtu": 9000,
        "auto_route": true,
        "strict_route": true,
        "stack": "mixed",
    }));

    let route = config["route"].as_object_mut().expect("route");
    route.insert("auto_detect_interface".into(), json!(true));
    let rules = route
        .entry("rules")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("rules");
    rules.retain(|rule| rule["action"] != "sniff" && rule["action"] != "hijack-dns");
    rules.insert(0, json!({ "protocol": "dns", "action": "hijack-dns" }));
    rules.insert(0, json!({ "action": "sniff" }));

    let experimental = config
        .as_object_mut()
        .expect("config object")
        .entry("experimental")
        .or_insert_with(|| json!({}));
    experimental["cache_file"] = json!({
        "enabled": true,
        "path": shared_dir.join("cache.db"),
    });

    config["log"] = json!({
        "level": "info",
        "timestamp": true,
        "output": shared_dir.join("sing-box.log"),
    });
}

fn fetch(url: &str) -> SpikeResult<String> {
    let response = DirectFetcher
        .get(url, None, None)
        .map_err(|e| format!("fetch: {e}"))?;
    Ok(response.body)
}

/// `source` is `url:<subscription URL>`, `raw:<subscription body>`, or `direct`.
pub fn build_config(source: &str, shared_dir: &Path, secret: &str) -> SpikeResult<String> {
    let geoip_dir: PathBuf = shared_dir.join("geoip");
    let mut config: Value = if source == "direct" {
        build_direct_only_config(
            &template(secret),
            CaptureIntent::Diagnostic,
            HostPlatform::Android,
        )
        .map_err(|e| format!("direct config: {e}"))?
    } else {
        let raw = if let Some(url) = source.strip_prefix("url:") {
            fetch(url)?
        } else if let Some(raw) = source.strip_prefix("raw:") {
            raw.to_string()
        } else {
            return Err(format!("unknown source: {source}"));
        };
        let text = subscription_to_config(
            &raw,
            template(secret),
            Some(geoip_dir),
            CaptureIntent::Diagnostic,
            HostPlatform::Android,
        )
        .map_err(|e| format!("engine: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("parse generated config: {e}"))?
    };
    mobilize(&mut config, shared_dir);
    let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    let path = shared_dir.join("config.json");
    std::fs::write(&path, &text).map_err(|e| format!("write {}: {e}", path.display()))?;
    let outbounds = config["outbounds"].as_array().map_or(0, Vec::len);
    Ok(format!(
        "config ok: {} bytes, {outbounds} outbounds, {}",
        text.len(),
        path.display()
    ))
}

pub fn https_check(url: &str) -> SpikeResult<String> {
    let body = fetch(url)?;
    Ok(format!("https ok: {} bytes", body.len()))
}

pub fn clash_check(secret: &str) -> SpikeResult<String> {
    let endpoints = HealthEndpoints::new(CLASH_HOST, CLASH_PORT).with_secret(secret);
    let mode = ice_core::get_mode(&endpoints).map_err(|e| format!("mode: {e}"))?;
    let groups = ice_core::proxy_groups(&endpoints).map_err(|e| format!("groups: {e}"))?;
    let traffic = ice_core::traffic_sample(&endpoints).map_err(|e| format!("traffic: {e}"))?;
    let summary: Vec<String> = groups
        .iter()
        .map(|g| format!("{}={}", g.tag, g.now))
        .collect();
    Ok(format!(
        "clash ok: mode={mode} groups=[{}] up={} down={}",
        summary.join(", "),
        traffic.up,
        traffic.down
    ))
}

pub fn clash_delay(secret: &str, tag: &str) -> SpikeResult<String> {
    let endpoints = HealthEndpoints::new(CLASH_HOST, CLASH_PORT).with_secret(secret);
    let delay = ice_core::proxy_delay(&endpoints, tag, 5000, ice_core::DELAY_TEST_URL)
        .map_err(|e| format!("delay: {e}"))?;
    Ok(format!("delay ok: {tag} {delay} ms"))
}

fn report(result: SpikeResult<String>) -> String {
    result.unwrap_or_else(|e| format!("ERROR {e}"))
}

fn jstring<'local>(env: &mut Env<'local>, value: &str) -> jni::errors::Result<JString<'local>> {
    env.new_string(value)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yilongmusk_icebox_spike_Native_init<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    context: JObject<'caller>,
) -> JString<'caller> {
    unowned_env
        .with_env(|env| -> jni::errors::Result<_> {
            let _ = tracing_subscriber::fmt().with_ansi(false).try_init();
            #[cfg(target_os = "android")]
            let message = match rustls_platform_verifier::android::init_with_env(env, context) {
                Ok(()) => "init ok".to_string(),
                Err(e) => format!("ERROR init: {e}"),
            };
            #[cfg(not(target_os = "android"))]
            let message = {
                let _ = context;
                "init ok (host)".to_string()
            };
            jstring(env, &message)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yilongmusk_icebox_spike_Native_buildConfig<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    source: JString<'caller>,
    shared_dir: JString<'caller>,
    secret: JString<'caller>,
) -> JString<'caller> {
    unowned_env
        .with_env(|env| -> jni::errors::Result<_> {
            let source = source.try_to_string(env)?;
            let shared_dir = PathBuf::from(shared_dir.try_to_string(env)?);
            let secret = secret.try_to_string(env)?;
            let message = report(build_config(&source, &shared_dir, &secret));
            jstring(env, &message)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yilongmusk_icebox_spike_Native_httpsCheck<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    url: JString<'caller>,
) -> JString<'caller> {
    unowned_env
        .with_env(|env| -> jni::errors::Result<_> {
            let url = url.try_to_string(env)?;
            let message = report(https_check(&url));
            jstring(env, &message)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yilongmusk_icebox_spike_Native_clashCheck<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    secret: JString<'caller>,
) -> JString<'caller> {
    unowned_env
        .with_env(|env| -> jni::errors::Result<_> {
            let secret = secret.try_to_string(env)?;
            let message = report(clash_check(&secret));
            jstring(env, &message)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_yilongmusk_icebox_spike_Native_clashDelay<'caller>(
    mut unowned_env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    secret: JString<'caller>,
    tag: JString<'caller>,
) -> JString<'caller> {
    unowned_env
        .with_env(|env| -> jni::errors::Result<_> {
            let secret = secret.try_to_string(env)?;
            let tag = tag.try_to_string(env)?;
            let message = report(clash_delay(&secret, &tag));
            jstring(env, &message)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../../../configs/examples/subscription-uri-list.txt");

    #[test]
    fn mobile_config_from_fixture() {
        let dir = std::env::temp_dir().join("icebox-spike-test");
        std::fs::create_dir_all(&dir).unwrap();
        let message = build_config(&format!("raw:{FIXTURE}"), &dir, "spikesecret0123456789").unwrap();
        assert!(message.starts_with("config ok"), "{message}");
        let config: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("config.json")).unwrap())
                .unwrap();
        let inbounds = config["inbounds"].as_array().unwrap();
        assert_eq!(inbounds.len(), 1);
        assert_eq!(inbounds[0]["type"], "tun");
        assert!(inbounds[0].get("interface_name").is_none());
        assert_eq!(config["route"]["rules"][1]["action"], "hijack-dns");
        println!("{}", serde_json::to_string_pretty(&config["dns"]).unwrap());
        println!(
            "{}",
            serde_json::to_string_pretty(&config["route"]["rules"]).unwrap()
        );
    }

    #[test]
    fn mobile_direct_config() {
        let dir = std::env::temp_dir().join("icebox-spike-direct");
        std::fs::create_dir_all(&dir).unwrap();
        build_config("direct", &dir, "spikesecret0123456789").unwrap();
        let text = std::fs::read_to_string(dir.join("config.json")).unwrap();
        println!("{text}");
    }
}
