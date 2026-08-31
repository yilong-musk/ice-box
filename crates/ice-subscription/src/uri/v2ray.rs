//! `vmess://` and `vless://` share link parsing → sing-box outbounds.

use base64::Engine;
use serde_json::{json, Value};

use super::{parse_query, query_bool, query_get, split_host_port, split_userinfo, SkipReason};

/// Parse `vmess://`: either v2rayN base64 JSON (`vmess://BASE64(JSON)`) or the
/// URI style (`vmess://uuid@host:port?params`).
///
/// Returns the outbound plus the node name from the JSON body's `ps` field
/// (the URI style has no such field; its name comes from the `#fragment`).
pub fn parse_vmess(rest: &str) -> Result<(Value, Option<String>), SkipReason> {
    if let Ok(decoded) = decode_base64_quiet(rest) {
        if let Ok(value) = serde_json::from_str::<Value>(&decoded) {
            if let Some((out, name)) = parse_vmess_json(&value) {
                return Ok((out, name));
            }
        }
    }

    // Fall back to URI-style share link.
    let (userinfo, authority) = split_userinfo(rest);
    let userinfo = userinfo
        .ok_or_else(|| SkipReason::Incomplete("vmess link missing user info (uuid)".into()))?;
    let (host, port) = split_host_port(authority)?;
    let params = parse_query(query_part(authority));
    let uuid = &userinfo;

    let security_param = query_get(&params, "security")
        .or_else(|| query_get(&params, "encryption"))
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "auto".into());
    let tls_mode = query_get(&params, "tls")
        .unwrap_or("none")
        .to_ascii_lowercase();
    if security_param == "reality" {
        return Err(SkipReason::Unsupported(
            "vmess reality is not supported by sing-box".into(),
        ));
    }
    // The standard URI-style format marks TLS via `security=tls|xtls` (qv2ray /
    // v2ray spec); the legacy `tls=` param is honored too. sing-box vmess
    // `security` only accepts encryption methods, so tls/xtls map to `auto`.
    let tls_enabled = matches!(tls_mode.as_str(), "tls" | "xtls")
        || matches!(security_param.as_str(), "tls" | "xtls");
    let security = if matches!(security_param.as_str(), "tls" | "xtls") {
        "auto".to_string()
    } else {
        security_param
    };
    let transport_type = query_get(&params, "type")
        .or_else(|| query_get(&params, "headerType"))
        .unwrap_or("tcp")
        .to_ascii_lowercase();
    let host_hdr = query_get(&params, "host").unwrap_or("");
    let path = query_get(&params, "path").unwrap_or("");
    let sni = query_get(&params, "sni").unwrap_or("");
    let fp = query_get(&params, "fp").unwrap_or("");
    let alpn = query_get(&params, "alpn").unwrap_or("");

    let insecure = query_bool(&params, "insecure") || query_bool(&params, "allowInsecure");

    let mut out = json!({
        "type": "vmess",
        "server": host,
        "server_port": port,
        "uuid": uuid,
        "security": security,
        "alter_id": 0,
    });

    if tls_enabled {
        apply_tls(
            &mut out, sni, host_hdr, fp, alpn, insecure, None, None, None,
        )?;
    }

    apply_transport(
        &mut out,
        transport_type.as_str(),
        host_hdr,
        path,
        None,
        None,
    )?;
    Ok((out, None))
}

/// v2rayN `vmess://BASE64(JSON)` body.
fn parse_vmess_json(value: &Value) -> Option<(Value, Option<String>)> {
    let obj = value.as_object()?;
    let host = obj.get("add")?.as_str()?.trim().to_string();
    if host.is_empty() {
        return None;
    }
    let port: u16 = obj
        .get("port")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| v.as_u64().map(|u| u as u16))
        })
        .filter(|&p| p > 0)?;
    let uuid = obj.get("id")?.as_str()?.trim().to_string();
    if uuid.is_empty() {
        return None;
    }
    let ps = obj.get("ps").and_then(|v| v.as_str()).unwrap_or("");
    let security = obj
        .get("scy")
        .and_then(|v| v.as_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "auto".into());
    let alter_id: u64 = obj
        .get("aid")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| v.as_u64())
        })
        .unwrap_or(0);
    let network = obj
        .get("net")
        .and_then(|v| v.as_str())
        .unwrap_or("tcp")
        .to_ascii_lowercase();
    let tcp_header = obj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("none")
        .to_ascii_lowercase();
    let transport_host = obj.get("host").and_then(|v| v.as_str()).unwrap_or("");
    let path = obj.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let tls_mode = obj
        .get("tls")
        .and_then(|v| v.as_str())
        .unwrap_or("none")
        .to_ascii_lowercase();
    let sni = obj.get("sni").and_then(|v| v.as_str()).unwrap_or("");
    let fp = obj.get("fp").and_then(|v| v.as_str()).unwrap_or("");
    let alpn = obj.get("alpn").and_then(|v| v.as_str()).unwrap_or("");
    let allow_insecure = obj.get("allowInsecure").is_some_and(|v| match v {
        Value::Bool(b) => *b,
        Value::String(s) => {
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        }
        _ => false,
    });
    let name = ps.trim().to_string();
    let name = (!name.is_empty()).then_some(name);

    let mut out = json!({
        "type": "vmess",
        "server": host,
        "server_port": port,
        "uuid": uuid,
        "security": security,
        "alter_id": alter_id,
    });

    if tls_mode == "tls" || tls_mode == "xtls" {
        apply_tls(
            &mut out,
            sni,
            transport_host,
            fp,
            alpn,
            allow_insecure,
            None,
            None,
            None,
        )
        .ok()?;
    }

    apply_transport(
        &mut out,
        network.as_str(),
        transport_host,
        path,
        Some(tcp_header.as_str()),
        None,
    )
    .ok()?;
    Some((out, name))
}

/// Parse `vless://uuid@host:port?params`.
pub fn parse_vless(rest: &str) -> Result<Value, SkipReason> {
    let (userinfo, authority) = split_userinfo(rest);
    let userinfo = userinfo
        .ok_or_else(|| SkipReason::Incomplete("vless link missing user info (uuid)".into()))?;
    let (host, port) = split_host_port(authority)?;
    let params = parse_query(query_part(authority));
    let uuid = &userinfo;

    let encryption = query_get(&params, "encryption").unwrap_or("none");
    if encryption != "none" {
        return Err(SkipReason::Unsupported(format!(
            "vless encryption {encryption} is not supported by sing-box"
        )));
    }

    let security = query_get(&params, "security")
        .unwrap_or("none")
        .to_ascii_lowercase();
    let flow = query_get(&params, "flow").unwrap_or("");
    let transport_type = query_get(&params, "type")
        .or_else(|| query_get(&params, "headerType"))
        .unwrap_or("tcp")
        .to_ascii_lowercase();
    let host_hdr = query_get(&params, "host").unwrap_or("");
    let path = query_get(&params, "path").unwrap_or("");
    let service_name = query_get(&params, "serviceName").unwrap_or("");
    let sni = query_get(&params, "sni").unwrap_or("");
    let fp = query_get(&params, "fp").unwrap_or("");
    let alpn = query_get(&params, "alpn").unwrap_or("");
    let pbk = query_get(&params, "pbk").unwrap_or("");
    let sid = query_get(&params, "sid").unwrap_or("");
    let spx = query_get(&params, "spx").unwrap_or("");
    let insecure = query_bool(&params, "insecure") || query_bool(&params, "allowInsecure");

    let mut out = json!({
        "type": "vless",
        "server": host,
        "server_port": port,
        "uuid": uuid,
    });

    match flow {
        "" => {}
        "xtls-rprx-vision" | "xtls-rprx-vision-udp443" => {
            out.as_object_mut()
                .unwrap()
                .insert("flow".into(), json!(flow));
            if flow.ends_with("udp443") {
                out.as_object_mut()
                    .unwrap()
                    .insert("packet_encoding".into(), json!("xudp"));
            }
        }
        other => {
            return Err(SkipReason::Unsupported(format!(
                "vless flow {other} is not supported by sing-box"
            )));
        }
    }

    match security.as_str() {
        "none" => {}
        "tls" | "xtls" => {
            apply_tls(
                &mut out, sni, host_hdr, fp, alpn, insecure, None, None, None,
            )?;
        }
        "reality" => {
            if pbk.is_empty() {
                return Err(SkipReason::Incomplete(
                    "vless reality link missing pbk".into(),
                ));
            }
            apply_tls(
                &mut out,
                sni,
                host_hdr,
                fp,
                alpn,
                insecure,
                Some(pbk),
                Some(sid),
                Some(spx),
            )?;
        }
        other => {
            return Err(SkipReason::Unsupported(format!(
                "vless security {other} is not supported by sing-box"
            )));
        }
    }

    apply_transport(
        &mut out,
        transport_type.as_str(),
        host_hdr,
        path,
        None,
        Some(service_name),
    )?;
    Ok(out)
}

/// Add a `tls` block; `reality` keys enable sing-box reality instead of utls.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_tls(
    out: &mut Value,
    sni: &str,
    host: &str,
    fp: &str,
    alpn: &str,
    insecure: bool,
    public_key: Option<&str>,
    short_id: Option<&str>,
    spider_x: Option<&str>,
) -> Result<(), SkipReason> {
    let obj = out.as_object_mut().unwrap();
    let server_name = if sni.is_empty() { host } else { sni };
    let server_name = if server_name.is_empty() {
        obj.get("server")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        server_name.to_string()
    };

    let mut tls = json!({
        "enabled": true,
        "server_name": server_name,
        "insecure": insecure,
    });

    if let Some(pk) = public_key {
        let mut reality = json!({
            "enabled": true,
            "public_key": pk,
        });
        if let Some(sid) = short_id.filter(|s| !s.is_empty()) {
            reality
                .as_object_mut()
                .unwrap()
                .insert("short_id".into(), json!(sid));
        }
        if let Some(spx) = spider_x.filter(|s| !s.is_empty()) {
            reality
                .as_object_mut()
                .unwrap()
                .insert("spider_x".into(), json!(spx));
        }
        tls.as_object_mut()
            .unwrap()
            .insert("reality".into(), reality);
    }

    if !alpn.is_empty() {
        let list: Vec<Value> = alpn
            .split(',')
            .map(|s| json!(s.trim()))
            .filter(|v| v.as_str().map(|s| !s.is_empty()).unwrap_or(false))
            .collect();
        if !list.is_empty() {
            tls.as_object_mut()
                .unwrap()
                .insert("alpn".into(), json!(list));
        }
    }

    // Reality (and TLS in general) requires a uTLS fingerprint in sing-box;
    // fall back to the share-link default (`chrome`) when the link omits `fp`.
    let fingerprint = if fp.is_empty() { "chrome" } else { fp };
    if !fingerprint.is_empty() {
        tls.as_object_mut().unwrap().insert(
            "utls".into(),
            json!({
                "enabled": true,
                "fingerprint": fingerprint,
            }),
        );
    }

    obj.insert("tls".into(), tls);
    Ok(())
}

/// Add a `transport` block from the share-link transport type.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_transport(
    out: &mut Value,
    transport_type: &str,
    host: &str,
    path: &str,
    tcp_header: Option<&str>,
    grpc_service: Option<&str>,
) -> Result<(), SkipReason> {
    let obj = out.as_object_mut().unwrap();
    match transport_type {
        "" | "tcp" => {
            if tcp_header == Some("http") && !host.is_empty() {
                obj.insert(
                    "transport".into(),
                    http_transport(host, if path.is_empty() { "/" } else { path }),
                );
            }
        }
        "ws" => {
            let mut transport = json!({
                "type": "ws",
                "path": if path.is_empty() { "/" } else { path },
            });
            if !host.is_empty() {
                transport
                    .as_object_mut()
                    .unwrap()
                    .insert("headers".into(), json!({ "Host": host }));
            }
            obj.insert("transport".into(), transport);
        }
        "grpc" => {
            let service_name = grpc_service.unwrap_or(path);
            let mut transport = json!({
                "type": "grpc",
                "service_name": service_name,
            });
            if !host.is_empty() {
                transport
                    .as_object_mut()
                    .unwrap()
                    .insert("authority".into(), json!(host));
            }
            obj.insert("transport".into(), transport);
        }
        "http" | "h2" => {
            obj.insert(
                "transport".into(),
                http_transport(host, if path.is_empty() { "/" } else { path }),
            );
        }
        other => {
            return Err(SkipReason::Unsupported(format!(
                "transport type {other} is not supported by sing-box"
            )));
        }
    }
    Ok(())
}

fn http_transport(host: &str, path: &str) -> Value {
    let hosts: Vec<Value> = host
        .split(',')
        .map(|s| json!(s.trim()))
        .filter(|v| v.as_str().map(|s| !s.is_empty()).unwrap_or(false))
        .collect();
    json!({
        "type": "http",
        "host": hosts,
        "path": path,
    })
}

fn query_part(rest: &str) -> &str {
    match rest.find('?') {
        Some(pos) => &rest[pos + 1..],
        None => "",
    }
}

fn decode_base64_quiet(s: &str) -> Result<String, base64::DecodeError> {
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&compact)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(&compact))
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(&compact))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&compact))?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn vmess_v2rayn_json() {
        let link = json!({
            "v": "2",
            "ps": "JP01",
            "add": "example.com",
            "port": "443",
            "id": "3f2e0f9a-1c5b-4e7a-9d6b-8a1c2d3e4f50",
            "aid": "0",
            "scy": "auto",
            "net": "ws",
            "type": "none",
            "host": "cdn.example.com",
            "path": "/vmess",
            "tls": "tls",
            "sni": "cdn.example.com",
            "alpn": "h2,http/1.1",
            "fp": "chrome"
        })
        .to_string();
        let encoded = base64::engine::general_purpose::STANDARD.encode(link.as_bytes());
        let (out, name) = parse_vmess(&encoded).unwrap();
        assert_eq!(name.as_deref(), Some("JP01"));
        assert_eq!(out["type"], "vmess");
        assert_eq!(out["server"], "example.com");
        assert_eq!(out["server_port"], 443);
        assert_eq!(out["uuid"], "3f2e0f9a-1c5b-4e7a-9d6b-8a1c2d3e4f50");
        assert_eq!(out["tls"]["enabled"], true);
        assert_eq!(out["tls"]["server_name"], "cdn.example.com");
        assert_eq!(out["tls"]["alpn"][0], "h2");
        assert_eq!(out["tls"]["utls"]["fingerprint"], "chrome");
        assert_eq!(out["transport"]["type"], "ws");
        assert_eq!(out["transport"]["path"], "/vmess");
        assert_eq!(out["transport"]["headers"]["Host"], "cdn.example.com");
    }

    #[test]
    fn vmess_uri_style() {
        let out = parse_vmess(
            "3f2e0f9a-1c5b-4e7a-9d6b-8a1c2d3e4f50@example.com:443?security=auto&type=ws&host=a.com&path=%2Fws&tls=tls&sni=a.com&fp=chrome",
        )
        .unwrap()
        .0;
        assert_eq!(out["type"], "vmess");
        assert_eq!(out["transport"]["path"], "/ws");
        assert_eq!(out["tls"]["server_name"], "a.com");
    }

    #[test]
    fn vmess_uri_style_tls_via_security_param() {
        let out = parse_vmess(
            "3f2e0f9a-1c5b-4e7a-9d6b-8a1c2d3e4f50@example.com:443?security=tls&type=ws&host=a.com&path=%2Fws&sni=a.com&fp=chrome",
        )
        .unwrap()
        .0;
        assert_eq!(out["tls"]["enabled"], true);
        assert_eq!(out["tls"]["server_name"], "a.com");
        assert_eq!(out["security"], "auto");
    }

    #[test]
    fn vmess_uri_style_insecure_flag() {
        let (out, name) = parse_vmess(
            "3f2e0f9a-1c5b-4e7a-9d6b-8a1c2d3e4f50@example.com:443?security=tls&tls=tls&sni=a.com&allowInsecure=1",
        )
        .unwrap();
        assert_eq!(out["tls"]["insecure"], true);
        assert_eq!(name, None);
    }

    #[test]
    fn vmess_v2rayn_json_insecure() {
        let link = json!({
            "v": "2",
            "ps": "JP02",
            "add": "example.com",
            "port": "443",
            "id": "3f2e0f9a-1c5b-4e7a-9d6b-8a1c2d3e4f50",
            "aid": "0",
            "scy": "auto",
            "net": "tcp",
            "type": "none",
            "host": "",
            "path": "",
            "tls": "tls",
            "sni": "example.com",
            "allowInsecure": true
        })
        .to_string();
        let encoded = base64::engine::general_purpose::STANDARD.encode(link.as_bytes());
        let (out, name) = parse_vmess(&encoded).unwrap();
        assert_eq!(out["tls"]["insecure"], true);
        assert_eq!(name.as_deref(), Some("JP02"));
    }

    #[test]
    fn vless_ws_tls() {
        let out = parse_vless(
            "3f2e0f9a-1c5b-4e7a-9d6b-8a1c2d3e4f50@example.com:443?encryption=none&security=tls&type=ws&host=cdn.com&path=%2Fvless&sni=cdn.com&fp=chrome&alpn=h2",
        )
        .unwrap();
        assert_eq!(out["type"], "vless");
        assert_eq!(out["tls"]["enabled"], true);
        assert_eq!(out["tls"]["server_name"], "cdn.com");
        assert_eq!(out["tls"]["utls"]["fingerprint"], "chrome");
        assert_eq!(out["transport"]["type"], "ws");
        assert_eq!(out["transport"]["headers"]["Host"], "cdn.com");
    }

    #[test]
    fn vless_reality_vision() {
        let out = parse_vless(
            "uuid@example.com:443?encryption=none&security=reality&sni=apple.com&fp=chrome&pbk=abc123&sid=deadbeef&spx=%2F&flow=xtls-rprx-vision",
        )
        .unwrap();
        assert_eq!(out["flow"], "xtls-rprx-vision");
        assert_eq!(out["tls"]["reality"]["enabled"], true);
        assert_eq!(out["tls"]["reality"]["public_key"], "abc123");
        assert_eq!(out["tls"]["reality"]["short_id"], "deadbeef");
        assert_eq!(out["tls"]["reality"]["spider_x"], "/");
        assert_eq!(out["tls"]["utls"]["fingerprint"], "chrome");
    }

    #[test]
    fn vless_reality_without_fp_defaults_to_chrome_utls() {
        let out = parse_vless(
            "uuid@example.com:443?encryption=none&security=reality&sni=apple.com&pbk=abc123&sid=deadbeef",
        )
        .unwrap();
        assert_eq!(out["tls"]["utls"]["fingerprint"], "chrome");
    }

    #[test]
    fn vless_tls_insecure_flag() {
        let out =
            parse_vless("uuid@example.com:443?encryption=none&security=tls&sni=a.com&insecure=1")
                .unwrap();
        assert_eq!(out["tls"]["insecure"], true);
    }

    #[test]
    fn vless_unsupported_flow_skipped() {
        assert!(parse_vless("uuid@example.com:443?flow=xtls-rprx-splice").is_err());
    }

    #[test]
    fn vless_grpc() {
        let out = parse_vless(
            "uuid@example.com:443?encryption=none&security=tls&type=grpc&serviceName=svc&host=grpc.example.com&sni=grpc.example.com",
        )
        .unwrap();
        assert_eq!(out["transport"]["type"], "grpc");
        assert_eq!(out["transport"]["service_name"], "svc");
        assert_eq!(out["transport"]["authority"], "grpc.example.com");
    }
}
