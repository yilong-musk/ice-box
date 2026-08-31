//! `hysteria://` and `hysteria2://` share link parsing → sing-box outbounds.

use serde_json::{json, Value};

use super::{parse_query, query_bool, query_get, split_host_port, split_userinfo, SkipReason};

/// Parse `hysteria://host:port?params` (v1 share link, no userinfo).
pub fn parse_hysteria(rest: &str) -> Result<Value, SkipReason> {
    let (host, port) = split_host_port(rest)?;
    let params = parse_query(query_part(rest));

    let protocol = query_get(&params, "protocol").unwrap_or("udp");
    if protocol != "udp" {
        return Err(SkipReason::Unsupported(format!(
            "hysteria protocol {protocol} is not supported by sing-box"
        )));
    }

    let auth = query_get(&params, "auth").unwrap_or("");
    let insecure = query_bool(&params, "insecure") || query_bool(&params, "allowInsecure");
    let sni = query_get(&params, "sni")
        .or_else(|| query_get(&params, "peer"))
        .unwrap_or("");
    let obfs = query_get(&params, "obfs").unwrap_or("");
    if !obfs.is_empty() && obfs != "xplus" && obfs != "salamander" {
        return Err(SkipReason::Unsupported(format!(
            "hysteria obfs {obfs} is not supported by sing-box"
        )));
    }
    let alpn = query_get(&params, "alpn").unwrap_or("");

    let up_mbps = mbps_value(&params, "upmbps")
        .or_else(|| kbps_value(&params, "up"))
        .unwrap_or(0);
    let down_mbps = mbps_value(&params, "downmbps")
        .or_else(|| kbps_value(&params, "down"))
        .unwrap_or(0);

    let mut out = json!({
        "type": "hysteria",
        "server": host,
        "server_port": port,
        "up_mbps": up_mbps,
        "down_mbps": down_mbps,
    });

    if !auth.is_empty() {
        out.as_object_mut()
            .unwrap()
            .insert("auth_str".into(), json!(auth));
    }
    if !obfs.is_empty() {
        out.as_object_mut()
            .unwrap()
            .insert("obfs".into(), json!(obfs));
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

/// Parse `hysteria2://password@host:port?params` (also `hy2://`).
pub fn parse_hysteria2(rest: &str) -> Result<Value, SkipReason> {
    let (userinfo, authority) = split_userinfo(rest);
    let password = userinfo.unwrap_or_default();
    let (host, port) = split_host_port(authority)?;
    let params = parse_query(query_part(authority));

    // sing-box has no cert-pin support. Providers that ship `pinSHA256` do so
    // because their server certificate is not standards-compliant (it would not
    // verify against system roots), so the pin must degrade to `insecure`.
    let pinned = query_get(&params, "pinSHA256")
        .or_else(|| query_get(&params, "pin_sha256"))
        .is_some();
    let insecure =
        query_bool(&params, "insecure") || query_bool(&params, "allowInsecure") || pinned;
    let sni = query_get(&params, "sni")
        .or_else(|| query_get(&params, "peer"))
        .unwrap_or("");
    let alpn = query_get(&params, "alpn").unwrap_or("");

    let mut out = json!({
        "type": "hysteria2",
        "server": host,
        "server_port": port,
    });
    if !password.is_empty() {
        out.as_object_mut()
            .unwrap()
            .insert("password".into(), json!(password));
    }

    let obfs = query_get(&params, "obfs").unwrap_or("");
    let obfs_password = query_get(&params, "obfs-password")
        .or_else(|| query_get(&params, "obfs_password"))
        .unwrap_or("");
    if !obfs.is_empty() {
        if obfs != "salamander" {
            return Err(SkipReason::Unsupported(format!(
                "hysteria2 obfs {obfs} is not supported by sing-box"
            )));
        }
        let mut obfs_obj = json!({ "type": "salamander" });
        if !obfs_password.is_empty() {
            obfs_obj
                .as_object_mut()
                .unwrap()
                .insert("password".into(), json!(obfs_password));
        }
        out.as_object_mut().unwrap().insert("obfs".into(), obfs_obj);
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

fn mbps_value(params: &[(String, String)], key: &str) -> Option<u64> {
    query_get(params, key)?
        .parse::<f64>()
        .ok()
        .map(|v| v as u64)
}

fn kbps_value(params: &[(String, String)], key: &str) -> Option<u64> {
    query_get(params, key)?
        .parse::<f64>()
        .ok()
        .map(|v| (v / 1000.0) as u64)
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
    fn hysteria_v1_full() {
        let out = parse_hysteria(
            "example.com:443?protocol=udp&auth=sekret&insecure=1&sni=example.com&obfs=xplus&upmbps=100&downmbps=200&alpn=h3",
        )
        .unwrap();
        assert_eq!(out["type"], "hysteria");
        assert_eq!(out["server"], "example.com");
        assert_eq!(out["server_port"], 443);
        assert_eq!(out["auth_str"], "sekret");
        assert_eq!(out["up_mbps"], 100);
        assert_eq!(out["down_mbps"], 200);
        assert_eq!(out["obfs"], "xplus");
        assert_eq!(out["tls"]["server_name"], "example.com");
        assert_eq!(out["tls"]["insecure"], true);
        assert_eq!(out["tls"]["alpn"][0], "h3");
    }

    #[test]
    fn hysteria_v1_faketcp_skipped() {
        assert!(parse_hysteria("example.com:443?protocol=faketcp").is_err());
    }

    #[test]
    fn hysteria2_full() {
        let out = parse_hysteria2(
            "pass%40word@example.com:8443?insecure=1&sni=example.com&obfs=salamander&obfs-password=salty",
        )
        .unwrap();
        assert_eq!(out["type"], "hysteria2");
        assert_eq!(out["password"], "pass@word");
        assert_eq!(out["obfs"]["type"], "salamander");
        assert_eq!(out["obfs"]["password"], "salty");
        assert_eq!(out["tls"]["server_name"], "example.com");
        assert_eq!(out["tls"]["insecure"], true);
    }

    #[test]
    fn hysteria2_pinsha256_degrades_to_insecure() {
        let out = parse_hysteria2(
            "pass@example.com:443?insecure=false&sni=example.com&pinSHA256=c77f0fb1aef429ebc3f7e15078ea13c3",
        )
        .unwrap();
        assert_eq!(out["tls"]["insecure"], true, "pin must degrade to insecure");
        assert_eq!(out["tls"]["server_name"], "example.com");
    }

    #[test]
    fn hysteria2_without_userinfo() {
        let out = parse_hysteria2("example.com:443").unwrap();
        assert!(out.get("password").is_none());
        assert_eq!(out["tls"]["enabled"], true);
    }

    #[test]
    fn hysteria2_bad_obfs_skipped() {
        assert!(parse_hysteria2("pass@example.com:443?obfs=weird").is_err());
    }
}
