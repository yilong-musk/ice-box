//! `tuic://` share link parsing → sing-box tuic outbound.

use serde_json::{json, Value};

use super::{parse_query, query_bool, query_get, split_host_port, split_userinfo, SkipReason};

/// Parse `tuic://uuid:password@host:port?params`.
pub fn parse_tuic(rest: &str) -> Result<Value, SkipReason> {
    let (userinfo, authority) = split_userinfo(rest);
    let userinfo = userinfo
        .ok_or_else(|| SkipReason::Incomplete("tuic link missing user info (uuid)".into()))?;
    let (host, port) = split_host_port(authority)?;
    let params = parse_query(query_part(authority));

    let (uuid, password) = match userinfo.split_once(':') {
        Some((u, p)) => (u.to_string(), Some(p.to_string())),
        None => (userinfo, None),
    };
    if uuid.is_empty() {
        return Err(SkipReason::Incomplete("tuic link missing uuid".into()));
    }
    let password = password
        .filter(|p| !p.is_empty())
        .or_else(|| query_get(&params, "password").map(str::to_string))
        .or_else(|| query_get(&params, "token").map(str::to_string));

    let insecure = query_bool(&params, "allow_insecure")
        || query_bool(&params, "allowInsecure")
        || query_bool(&params, "insecure");
    let sni = query_get(&params, "sni")
        .or_else(|| query_get(&params, "peer"))
        .unwrap_or("");
    let alpn = query_get(&params, "alpn").unwrap_or("");
    let congestion_control = query_get(&params, "congestion_control").unwrap_or("");
    let udp_relay_mode = query_get(&params, "udp_relay_mode").unwrap_or("");

    let mut out = json!({
        "type": "tuic",
        "server": host,
        "server_port": port,
        "uuid": uuid,
    });
    if let Some(p) = password {
        out.as_object_mut()
            .unwrap()
            .insert("password".into(), json!(p));
    }
    if !congestion_control.is_empty() {
        out.as_object_mut()
            .unwrap()
            .insert("congestion_control".into(), json!(congestion_control));
    }
    if !udp_relay_mode.is_empty() {
        out.as_object_mut()
            .unwrap()
            .insert("udp_relay_mode".into(), json!(udp_relay_mode));
    }

    let mut tls = json!({
        "enabled": true,
        "insecure": insecure,
    });
    if !sni.is_empty() {
        tls.as_object_mut()
            .unwrap()
            .insert("server_name".into(), json!(sni));
    }
    if !alpn.is_empty() {
        tls.as_object_mut().unwrap().insert(
            "alpn".into(),
            json!(alpn.split(',').map(|s| s.trim()).collect::<Vec<_>>()),
        );
    }
    out.as_object_mut().unwrap().insert("tls".into(), tls);

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
    fn tuic_full() {
        let out = parse_tuic(
            "3f2e0f9a-1c5b-4e7a-9d6b-8a1c2d3e4f50:token123@example.com:443?sni=example.com&alpn=h3&congestion_control=bbr&allow_insecure=1&udp_relay_mode=native",
        )
        .unwrap();
        assert_eq!(out["type"], "tuic");
        assert_eq!(out["uuid"], "3f2e0f9a-1c5b-4e7a-9d6b-8a1c2d3e4f50");
        assert_eq!(out["password"], "token123");
        assert_eq!(out["congestion_control"], "bbr");
        assert_eq!(out["udp_relay_mode"], "native");
        assert_eq!(out["tls"]["server_name"], "example.com");
        assert_eq!(out["tls"]["insecure"], true);
        assert_eq!(out["tls"]["alpn"][0], "h3");
    }

    #[test]
    fn tuic_uuid_only() {
        let out = parse_tuic("uuid@example.com:443?sni=x.com").unwrap();
        assert!(out.get("password").is_none());
    }

    #[test]
    fn tuic_missing_uuid() {
        assert!(parse_tuic("@example.com:443").is_err());
    }
}
