//! `trojan://` share link parsing → sing-box trojan outbound.

use serde_json::{json, Value};

use super::{parse_query, query_bool, query_get, split_host_port, split_userinfo, SkipReason};
use crate::uri::v2ray::{apply_tls, apply_transport};

/// Parse `trojan://password@host:port?params`.
pub fn parse_trojan(rest: &str) -> Result<Value, SkipReason> {
    let (userinfo, authority) = split_userinfo(rest);
    let userinfo = userinfo
        .ok_or_else(|| SkipReason::Incomplete("trojan link missing user info (password)".into()))?;
    let (host, port) = split_host_port(authority)?;
    let params = parse_query(query_part(authority));
    let password = &userinfo;
    if password.is_empty() {
        return Err(SkipReason::Incomplete(
            "trojan link missing password".into(),
        ));
    }

    let security = query_get(&params, "security")
        .unwrap_or("tls")
        .to_ascii_lowercase();
    if security != "tls" && security != "xtls" && security != "reality" {
        return Err(SkipReason::Unsupported(format!(
            "trojan security {security} is not supported by sing-box"
        )));
    }

    let sni = query_get(&params, "sni").unwrap_or("");
    let host_hdr = query_get(&params, "host").unwrap_or("");
    let fp = query_get(&params, "fp").unwrap_or("");
    let alpn = query_get(&params, "alpn").unwrap_or("");
    let insecure = query_bool(&params, "allowInsecure") || query_bool(&params, "insecure");
    let transport_type = query_get(&params, "type")
        .unwrap_or("tcp")
        .to_ascii_lowercase();
    let path = query_get(&params, "path").unwrap_or("");
    let service_name = query_get(&params, "serviceName").unwrap_or("");

    if let Some(flow) = query_get(&params, "flow") {
        if !flow.is_empty() {
            return Err(SkipReason::Unsupported(format!(
                "trojan flow {flow} is not supported by sing-box"
            )));
        }
    }

    let mut out = json!({
        "type": "trojan",
        "server": host,
        "server_port": port,
        "password": password,
    });

    let pbk = query_get(&params, "pbk").unwrap_or("");
    let sid = query_get(&params, "sid").unwrap_or("");
    let spx = query_get(&params, "spx").unwrap_or("");
    apply_tls(
        &mut out,
        sni,
        host_hdr,
        fp,
        alpn,
        insecure,
        if security == "reality" && !pbk.is_empty() {
            Some(pbk)
        } else {
            None
        },
        if security == "reality" {
            Some(sid)
        } else {
            None
        },
        if security == "reality" {
            Some(spx)
        } else {
            None
        },
    )?;
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

fn query_part(rest: &str) -> &str {
    match rest.find('?') {
        Some(pos) => &rest[pos + 1..],
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trojan_ws_tls() {
        let out = parse_trojan(
            "secret-pass@example.com:443?type=ws&host=cdn.com&path=%2Ftrojan&sni=cdn.com&allowInsecure=1&fp=chrome&alpn=h2%2Chttp%2F1.1",
        )
        .unwrap();
        assert_eq!(out["type"], "trojan");
        assert_eq!(out["password"], "secret-pass");
        assert_eq!(out["server"], "example.com");
        assert_eq!(out["tls"]["enabled"], true);
        assert_eq!(out["tls"]["server_name"], "cdn.com");
        assert_eq!(out["tls"]["insecure"], true);
        assert_eq!(out["tls"]["utls"]["fingerprint"], "chrome");
        assert_eq!(out["tls"]["alpn"][0], "h2");
        assert_eq!(out["transport"]["type"], "ws");
        assert_eq!(out["transport"]["path"], "/trojan");
        assert_eq!(out["transport"]["headers"]["Host"], "cdn.com");
    }

    #[test]
    fn trojan_plain_tcp() {
        let out = parse_trojan("pass@example.com:443").unwrap();
        assert!(out["transport"].as_object().is_none());
        assert_eq!(out["tls"]["server_name"], "example.com");
    }

    #[test]
    fn trojan_missing_password() {
        assert!(parse_trojan("@example.com:443").is_err());
    }
}
