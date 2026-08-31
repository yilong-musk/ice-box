//! `ss://` share link parsing (SIP002 and legacy formats).

use base64::Engine;
use serde_json::json;

use super::{parse_query, query_get, split_host_port, split_userinfo, SkipReason};

/// Parse `ss://` link body: SIP002 (`userinfo@host:port?plugin=...#name`)
/// or legacy (`base64(method:password@host:port)`).
pub fn parse_ss(rest: &str) -> Result<serde_json::Value, SkipReason> {
    let (userinfo, authority) = split_userinfo(rest);
    let (method, password, host, port) = if let Some(userinfo) = userinfo {
        let (method, password) = decode_method_password(&userinfo)?;
        let (host, port) = split_host_port(authority)?;
        (method, password, host, port)
    } else {
        // Legacy: entire body (minus query/fragment) is base64 of method:password@host:port.
        let decoded = decode_legacy(rest)?;
        let (userinfo, authority) = split_userinfo(&decoded);
        let userinfo = userinfo
            .ok_or_else(|| SkipReason::Incomplete("ss legacy link missing '@' separator".into()))?;
        let (method, password) = decode_method_password(&userinfo)?;
        let (host, port) = split_host_port(authority)?;
        (method, password, host, port)
    };

    let mut out = json!({
        "type": "shadowsocks",
        "server": host,
        "server_port": port,
        "method": method,
        "password": password,
    });

    let params = parse_query(query_part(rest));
    if let Some(plugin) = query_get(&params, "plugin") {
        let (plugin_name, opts) = plugin
            .split_once(';')
            .map_or((plugin, None), |(n, o)| (n, Some(o)));
        out.as_object_mut()
            .unwrap()
            .insert("plugin".into(), json!(plugin_name));
        if let Some(opts) = opts {
            out.as_object_mut()
                .unwrap()
                .insert("plugin_opts".into(), json!(opts));
        }
    }

    Ok(out)
}

fn query_part(rest: &str) -> &str {
    match rest.find('?') {
        Some(pos) => &rest[pos + 1..],
        None => "",
    }
}

fn decode_method_password(userinfo: &str) -> Result<(String, String), SkipReason> {
    let decoded = percent_decode(userinfo);
    // SIP002 encodes `method:password` as (URL-safe, unpadded) base64.
    let text = decode_b64_quiet(&decoded).unwrap_or(decoded);
    let (method, password) = text.split_once(':').ok_or_else(|| {
        SkipReason::Incomplete(format!("ss userinfo missing ':' separator: {userinfo}"))
    })?;
    if method.is_empty() || password.is_empty() {
        return Err(SkipReason::Incomplete(
            "ss userinfo missing method or password".into(),
        ));
    }
    Ok((method.to_string(), password.to_string()))
}

fn decode_b64_quiet(s: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(s)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(s))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(s))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s))
        .ok()?;
    let text = String::from_utf8_lossy(&bytes).to_string();
    if text.contains(':') {
        Some(text)
    } else {
        None
    }
}

fn decode_legacy(body: &str) -> Result<String, SkipReason> {
    // The fragment (`#name`) and any query are not part of the base64 payload
    // (the base64 alphabet has no `?`/`#`), so strip them before decoding.
    let body = match body.find('#') {
        Some(pos) => &body[..pos],
        None => body,
    };
    let body = match body.find('?') {
        Some(pos) => &body[..pos],
        None => body,
    };
    let compact: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&compact)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(&compact))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(&compact))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&compact))
        .map_err(|_| SkipReason::Incomplete("ss legacy link is not valid base64".into()))?;
    String::from_utf8(bytes)
        .map_err(|_| SkipReason::Incomplete("ss legacy link is not valid UTF-8".into()))
}

fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sip002_plain_userinfo() {
        let out = parse_ss("aes-128-gcm:secret@1.2.3.4:8388").unwrap();
        assert_eq!(out["type"], "shadowsocks");
        assert_eq!(out["method"], "aes-128-gcm");
        assert_eq!(out["password"], "secret");
        assert_eq!(out["server"], "1.2.3.4");
        assert_eq!(out["server_port"], 8388);
    }

    #[test]
    fn sip002_base64_userinfo_with_plugin() {
        use base64::Engine;
        let userinfo =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("aes-256-gcm:pass@word");
        let link = format!(
            "{userinfo}@example.com:443?plugin=obfs-local%3Bobfs%3Dhttp%3Bobfs-host%3Dx.com"
        );
        let out = parse_ss(&link).unwrap();
        assert_eq!(out["method"], "aes-256-gcm");
        assert_eq!(out["password"], "pass@word");
        assert_eq!(out["plugin"], "obfs-local");
        assert_eq!(out["plugin_opts"], "obfs=http;obfs-host=x.com");
    }

    #[test]
    fn legacy_whole_base64() {
        use base64::Engine;
        let inner = "aes-128-gcm:secret@1.2.3.4:8388";
        let encoded = base64::engine::general_purpose::STANDARD.encode(inner);
        let out = parse_ss(&encoded).unwrap();
        assert_eq!(out["server"], "1.2.3.4");
        assert_eq!(out["server_port"], 8388);
        assert_eq!(out["method"], "aes-128-gcm");
        assert_eq!(out["password"], "secret");
    }

    #[test]
    fn legacy_with_fragment_parses() {
        use base64::Engine;
        let inner = "aes-128-gcm:secret@1.2.3.4:8388";
        let encoded = base64::engine::general_purpose::STANDARD.encode(inner);
        let out = parse_ss(&format!("{encoded}#SS-01")).unwrap();
        assert_eq!(out["server"], "1.2.3.4");
        assert_eq!(out["server_port"], 8388);
    }

    #[test]
    fn invalid_ss_missing_separator() {
        assert!(parse_ss("plain").is_err());
    }
}
