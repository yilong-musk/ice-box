//! Clash `proxies` → sing-box leaf outbounds.

use ice_config::NormalizedOutbound;
use serde_json::{json, Value};

/// Supported Clash proxy types for v1 (architecture checklist).
pub const CLASH_SUPPORTED_TYPES: &[&str] = &["ss", "vmess", "trojan", "socks", "socks5", "http"];

/// Upper bound on `proxies` array length to limit memory / config size.
pub const MAX_CLASH_PROXIES: usize = 500;

#[derive(Debug, Clone)]
pub struct ProxyParseResult {
    pub nodes: Vec<NormalizedOutbound>,
    pub skipped: usize,
}

#[derive(Debug)]
pub(crate) enum SkipReason {
    Unsupported,
    Incomplete,
    TooMany,
}

pub fn parse_proxies(doc: &Value) -> Result<ProxyParseResult, SkipReason> {
    let proxies = doc
        .get("proxies")
        .and_then(|v| v.as_array())
        .ok_or(SkipReason::Incomplete)?;

    if proxies.len() > MAX_CLASH_PROXIES {
        return Err(SkipReason::TooMany);
    }

    let mut nodes = Vec::new();
    let mut skipped = 0usize;

    for (idx, proxy) in proxies.iter().enumerate() {
        match map_proxy(proxy, idx) {
            Ok(node) => nodes.push(node),
            Err(SkipReason::Unsupported | SkipReason::Incomplete | SkipReason::TooMany) => {
                skipped += 1
            }
        }
    }

    if nodes.is_empty() {
        return Err(SkipReason::Incomplete);
    }

    Ok(ProxyParseResult { nodes, skipped })
}

pub fn map_proxy(proxy: &Value, idx: usize) -> Result<NormalizedOutbound, SkipReason> {
    let obj = proxy.as_object().ok_or(SkipReason::Incomplete)?;
    let ty = obj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let name = obj
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("clash-{idx}"));

    let outbound = match ty.as_str() {
        "ss" => map_ss(obj, &name)?,
        "vmess" => map_vmess(obj, &name)?,
        "trojan" => map_trojan(obj, &name)?,
        "socks" | "socks5" => map_socks(obj, &name)?,
        "http" => map_http(obj, &name)?,
        _ => return Err(SkipReason::Unsupported),
    };

    Ok(NormalizedOutbound {
        tag: name,
        outbound,
    })
}

fn require_server_port(obj: &serde_json::Map<String, Value>) -> Result<(String, u16), SkipReason> {
    let server = obj
        .get("server")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or(SkipReason::Incomplete)?
        .to_string();
    let port = obj
        .get("port")
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_i64().map(|i| i as u64))
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
        .filter(|&p| p > 0 && p <= u16::MAX as u64)
        .ok_or(SkipReason::Incomplete)? as u16;
    Ok((server, port))
}

fn map_ss(obj: &serde_json::Map<String, Value>, tag: &str) -> Result<Value, SkipReason> {
    let (server, port) = require_server_port(obj)?;
    let method = obj
        .get("cipher")
        .and_then(|v| v.as_str())
        .ok_or(SkipReason::Incomplete)?;
    let password = obj
        .get("password")
        .and_then(|v| v.as_str())
        .ok_or(SkipReason::Incomplete)?;

    let mut out = json!({
        "type": "shadowsocks",
        "tag": tag,
        "server": server,
        "server_port": port,
        "method": method,
        "password": password,
    });

    if let Some(plugin) = obj.get("plugin").and_then(|v| v.as_str()) {
        if let Some(obj_mut) = out.as_object_mut() {
            obj_mut.insert("plugin".into(), json!(plugin));
            if let Some(opts) = obj.get("plugin-opts") {
                obj_mut.insert("plugin_opts".into(), opts.clone());
            }
        }
    }

    Ok(out)
}

fn map_vmess(obj: &serde_json::Map<String, Value>, tag: &str) -> Result<Value, SkipReason> {
    let (server, port) = require_server_port(obj)?;
    let uuid = obj
        .get("uuid")
        .and_then(|v| v.as_str())
        .ok_or(SkipReason::Incomplete)?;

    let security = obj.get("cipher").and_then(|v| v.as_str()).unwrap_or("auto");

    let mut out = json!({
        "type": "vmess",
        "tag": tag,
        "server": server.clone(),
        "server_port": port,
        "uuid": uuid,
        "security": security,
        "alter_id": obj.get("alterId").and_then(|v| v.as_u64()).unwrap_or(0),
    });

    let tls_enabled = obj
        .get("tls")
        .map(|v| match v {
            Value::Bool(b) => *b,
            Value::String(s) => s.eq_ignore_ascii_case("true"),
            _ => false,
        })
        .unwrap_or(false);

    if tls_enabled {
        let server_name = obj
            .get("servername")
            .or_else(|| obj.get("sni"))
            .and_then(|v| v.as_str())
            .unwrap_or(server.as_str());
        let insecure = obj
            .get("skip-cert-verify")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        out.as_object_mut().unwrap().insert(
            "tls".into(),
            json!({
                "enabled": true,
                "server_name": server_name,
                "insecure": insecure,
            }),
        );
    }

    let network = obj.get("network").and_then(|v| v.as_str()).unwrap_or("tcp");
    match network {
        "ws" => {
            let path = obj
                .get("ws-opts")
                .and_then(|o| o.get("path"))
                .and_then(|v| v.as_str())
                .or_else(|| obj.get("ws-path").and_then(|v| v.as_str()))
                .unwrap_or("/");
            let mut transport = json!({
                "type": "ws",
                "path": path,
            });
            if let Some(host) = obj
                .get("ws-opts")
                .and_then(|o| o.get("headers"))
                .and_then(|h| h.get("Host").or_else(|| h.get("host")))
                .and_then(|v| v.as_str())
            {
                transport
                    .as_object_mut()
                    .unwrap()
                    .insert("headers".into(), json!({ "Host": host }));
            }
            out.as_object_mut()
                .unwrap()
                .insert("transport".into(), transport);
        }
        "grpc" => {
            let service_name = obj
                .get("grpc-opts")
                .and_then(|o| o.get("grpc-service-name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            out.as_object_mut().unwrap().insert(
                "transport".into(),
                json!({
                    "type": "grpc",
                    "service_name": service_name,
                }),
            );
        }
        _ => {}
    }

    Ok(out)
}

fn map_trojan(obj: &serde_json::Map<String, Value>, tag: &str) -> Result<Value, SkipReason> {
    let (server, port) = require_server_port(obj)?;
    let password = obj
        .get("password")
        .and_then(|v| v.as_str())
        .ok_or(SkipReason::Incomplete)?;

    let server_name = obj
        .get("sni")
        .or_else(|| obj.get("servername"))
        .and_then(|v| v.as_str())
        .unwrap_or(server.as_str())
        .to_string();
    let insecure = obj
        .get("skip-cert-verify")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok(json!({
        "type": "trojan",
        "tag": tag,
        "server": server,
        "server_port": port,
        "password": password,
        "tls": {
            "enabled": true,
            "server_name": server_name,
            "insecure": insecure,
        }
    }))
}

fn map_socks(obj: &serde_json::Map<String, Value>, tag: &str) -> Result<Value, SkipReason> {
    let (server, port) = require_server_port(obj)?;
    let mut out = json!({
        "type": "socks",
        "tag": tag,
        "server": server,
        "server_port": port,
        "version": "5",
    });
    if let Some(user) = obj.get("username").and_then(|v| v.as_str()) {
        out.as_object_mut()
            .unwrap()
            .insert("username".into(), json!(user));
    }
    if let Some(pass) = obj.get("password").and_then(|v| v.as_str()) {
        out.as_object_mut()
            .unwrap()
            .insert("password".into(), json!(pass));
    }
    Ok(out)
}

fn map_http(obj: &serde_json::Map<String, Value>, tag: &str) -> Result<Value, SkipReason> {
    let (server, port) = require_server_port(obj)?;
    let mut out = json!({
        "type": "http",
        "tag": tag,
        "server": server,
        "server_port": port,
    });
    if let Some(user) = obj.get("username").and_then(|v| v.as_str()) {
        out.as_object_mut()
            .unwrap()
            .insert("username".into(), json!(user));
    }
    if let Some(pass) = obj.get("password").and_then(|v| v.as_str()) {
        out.as_object_mut()
            .unwrap()
            .insert("password".into(), json!(pass));
    }
    let tls_enabled = obj
        .get("tls")
        .map(|v| match v {
            Value::Bool(b) => *b,
            Value::String(s) => s.eq_ignore_ascii_case("true"),
            _ => false,
        })
        .unwrap_or(false);
    if tls_enabled {
        out.as_object_mut()
            .unwrap()
            .insert("tls".into(), json!({ "enabled": true }));
    }
    Ok(out)
}
