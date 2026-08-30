//! Basic share links: `socks://`, `socks5://`, `http://`, `https://`, `wireguard://`.

use serde_json::{json, Value};

use super::{parse_query, query_get, split_host_port, split_userinfo, SkipReason};

/// Parse `socks://` / `socks5://[user:pass@]host:port`.
pub fn parse_socks(rest: &str) -> Result<Value, SkipReason> {
    let (userinfo, authority) = split_userinfo(rest);
    let (host, port) = split_host_port(authority)?;

    let mut out = json!({
        "type": "socks",
        "server": host,
        "server_port": port,
        "version": "5",
    });

    if let Some(userinfo) = userinfo {
        let (user, pass) = match userinfo.split_once(':') {
            Some((u, p)) => (u.to_string(), Some(p.to_string())),
            None => (userinfo, None),
        };
        if !user.is_empty() {
            out.as_object_mut()
                .unwrap()
                .insert("username".into(), json!(user));
        }
        if let Some(p) = pass.filter(|p| !p.is_empty()) {
            out.as_object_mut()
                .unwrap()
                .insert("password".into(), json!(p));
        }
    }

    Ok(out)
}

/// Parse `http://` / `https://[user:pass@]host:port`; `https` enables TLS.
pub fn parse_http(rest: &str, tls: bool) -> Result<Value, SkipReason> {
    let (userinfo, authority) = split_userinfo(rest);
    let (host, port) = split_host_port(authority)?;

    let mut out = json!({
        "type": "http",
        "server": host,
        "server_port": port,
    });

    if let Some(userinfo) = userinfo {
        let (user, pass) = match userinfo.split_once(':') {
            Some((u, p)) => (u.to_string(), Some(p.to_string())),
            None => (userinfo, None),
        };
        if !user.is_empty() {
            out.as_object_mut()
                .unwrap()
                .insert("username".into(), json!(user));
        }
        if let Some(p) = pass.filter(|p| !p.is_empty()) {
            out.as_object_mut()
                .unwrap()
                .insert("password".into(), json!(p));
        }
    }

    if tls {
        out.as_object_mut()
            .unwrap()
            .insert("tls".into(), json!({ "enabled": true }));
    }

    Ok(out)
}

/// Parse `wireguard://` links: either `wireguard://publickey@host:port?params`
/// or ClashMeta style `wireguard://host:port?publickey=...&params`.
pub fn parse_wireguard(rest: &str) -> Result<Value, SkipReason> {
    let (userinfo, authority) = split_userinfo(rest);
    let (host, port) = split_host_port(authority)?;
    let params = parse_query(query_part(authority));

    let peer_public_key = userinfo
        .filter(|u| !u.is_empty())
        .or_else(|| {
            query_get(&params, "publickey")
                .or_else(|| query_get(&params, "public_key"))
                .or_else(|| query_get(&params, "peer_public_key"))
                .map(str::to_string)
        })
        .ok_or_else(|| SkipReason::Incomplete("wireguard link missing peer public key".into()))?;

    let private_key = query_get(&params, "privatekey")
        .or_else(|| query_get(&params, "private_key"))
        .ok_or_else(|| SkipReason::Incomplete("wireguard link missing private key".into()))?;
    let preshared_key = query_get(&params, "presharedkey").unwrap_or("");
    let address = query_get(&params, "address").unwrap_or("");
    let mtu = query_get(&params, "mtu").unwrap_or("");

    let mut out = json!({
        "type": "wireguard",
        "server": host,
        "server_port": port,
        "private_key": private_key,
        "peer_public_key": peer_public_key,
    });

    if !address.is_empty() {
        out.as_object_mut().unwrap().insert(
            "local_address".into(),
            json!(address.split(',').map(|s| s.trim()).collect::<Vec<_>>()),
        );
    }
    if !preshared_key.is_empty() {
        out.as_object_mut()
            .unwrap()
            .insert("preshared_key".into(), json!(preshared_key));
    }
    if let Some(reserved) = query_get(&params, "reserved") {
        let list: Vec<Value> = reserved
            .split(',')
            .filter_map(|s| s.trim().parse::<u8>().ok())
            .map(Value::from)
            .collect();
        if !list.is_empty() {
            out.as_object_mut()
                .unwrap()
                .insert("reserved".into(), json!(list));
        }
    }
    if let Ok(mtu) = mtu.parse::<u64>() {
        out.as_object_mut()
            .unwrap()
            .insert("mtu".into(), json!(mtu));
    }

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
    fn socks_with_auth() {
        let out = parse_socks("user:pass@1.2.3.4:1080").unwrap();
        assert_eq!(out["type"], "socks");
        assert_eq!(out["server"], "1.2.3.4");
        assert_eq!(out["server_port"], 1080);
        assert_eq!(out["version"], "5");
        assert_eq!(out["username"], "user");
        assert_eq!(out["password"], "pass");
    }

    #[test]
    fn socks_no_auth() {
        let out = parse_socks("1.2.3.4:1080").unwrap();
        assert!(out.get("username").is_none());
    }

    #[test]
    fn http_plain_and_tls() {
        let out = parse_http("user:pass@1.2.3.4:8080", false).unwrap();
        assert_eq!(out["type"], "http");
        assert!(out["tls"].as_object().is_none());
        let out = parse_http("1.2.3.4:443", true).unwrap();
        assert_eq!(out["tls"]["enabled"], true);
    }

    #[test]
    fn wireguard_full() {
        let out = parse_wireguard(
            "pubkey@1.2.3.4:443?privatekey=priv&presharedkey=psk&reserved=0,1,2&address=10.0.0.1%2F32%2C10.0.0.2%2F32&mtu=1420",
        )
        .unwrap();
        assert_eq!(out["type"], "wireguard");
        assert_eq!(out["peer_public_key"], "pubkey");
        assert_eq!(out["private_key"], "priv");
        assert_eq!(out["preshared_key"], "psk");
        assert_eq!(out["local_address"][0], "10.0.0.1/32");
        assert_eq!(out["local_address"][1], "10.0.0.2/32");
        assert_eq!(out["reserved"], json!([0, 1, 2]));
        assert_eq!(out["mtu"], 1420);
    }

    #[test]
    fn wireguard_clashmeta_style() {
        let out =
            parse_wireguard("1.2.3.4:51820?publickey=pub&privatekey=priv&address=10.0.0.1%2F32")
                .unwrap();
        assert_eq!(out["peer_public_key"], "pub");
        assert_eq!(out["private_key"], "priv");
        assert_eq!(out["local_address"][0], "10.0.0.1/32");
    }

    #[test]
    fn wireguard_missing_private_key() {
        assert!(parse_wireguard("pubkey@1.2.3.4:443").is_err());
    }
}
