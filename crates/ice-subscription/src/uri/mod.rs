//! Proxy URI list ("share link") subscription parsing → sing-box leaf outbounds.
//!
//! Supports the standard per-protocol share-link schemes emitted by most
//! providers (v2rayN / v2rayNG / ClashMeta / Hiddify / sing-box converters):
//! `ss://`, `vmess://`, `vless://`, `trojan://`, `hysteria://`, `hysteria2://`,
//! `tuic://`, `socks://`, `socks5://`, `http://`, `https://`, `wireguard://`.
//! `ssr://` is recognized but skipped (sing-box has no SSR support).

mod basic;
mod hysteria;
mod rules;
mod ss;
mod trojan;
mod tuic;
mod v2ray;

use std::collections::HashMap;

use ice_config::{NormalizedOutbound, NormalizedProfile, NormalizedRoute, ProfileParseStats};
use percent_encoding::percent_decode_str;

use crate::error::SubscriptionError;

/// Upper bound on URI lines (mirrors `MAX_CLASH_PROXIES`).
pub const MAX_URI_LINES: usize = 500;

/// Schemes recognized as proxy share links.
const URI_SCHEMES: &[&str] = &[
    "ss://",
    "ssr://",
    "vmess://",
    "vless://",
    "trojan://",
    "hysteria://",
    "hysteria2://",
    "hy2://",
    "tuic://",
    "socks://",
    "socks5://",
    "http://",
    "https://",
    "wireguard://",
];

fn scheme_of(line: &str) -> Option<&'static str> {
    URI_SCHEMES.iter().find(|s| line.starts_with(**s)).copied()
}

pub use rules::apply_builtin_default_rules;

/// Whether every non-empty, non-comment line of `raw` is a proxy share link.
/// Lines like `STATUS=...` / `TOTAL=...` (common SSR subscriptions) are ignored.
pub fn looks_like_uri_list(raw: &str) -> bool {
    let mut count = 0usize;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || is_info_line(line) {
            continue;
        }
        count += 1;
        if scheme_of(line).is_none() {
            return false;
        }
    }
    count > 0
}

fn is_info_line(line: &str) -> bool {
    let Some((key, _)) = line.split_once('=') else {
        return false;
    };
    key.chars().all(|c| c.is_ascii_uppercase() || c == '_')
}

/// Parse a proxy URI list into a normalized profile (no groups, direct route).
pub fn parse_uri_list_profile(raw: &str) -> Result<NormalizedProfile, SubscriptionError> {
    let mut nodes: Vec<NormalizedOutbound> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut skipped = 0usize;
    let mut line_count = 0usize;

    for (idx, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || is_info_line(line) {
            continue;
        }
        line_count += 1;
        if line_count > MAX_URI_LINES {
            return Err(SubscriptionError::ParseFailed(format!(
                "uri list exceeds line limit {MAX_URI_LINES}"
            )));
        }
        match parse_uri_line(line, idx) {
            Ok(node) => nodes.push(node),
            Err(SkipReason::Unsupported(reason)) => {
                skipped += 1;
                warnings.push(format!("line {}: {reason}", idx + 1));
            }
            Err(SkipReason::Incomplete(reason)) => {
                skipped += 1;
                warnings.push(format!("line {}: {reason}", idx + 1));
            }
        }
    }

    if nodes.is_empty() {
        return Err(SubscriptionError::EmptyNodes);
    }

    dedupe_tags(&mut nodes);

    let default_outbound = nodes[0].tag.clone();
    // Rule-less share-link subscriptions route through the injected `proxy`
    // selector (so the UI's node selection takes effect); built-in split
    // routing rules + DNS are attached at profile load time
    // (`apply_builtin_default_rules`), honoring the app's settings toggle.
    let route = NormalizedRoute {
        rules: Vec::new(),
        final_outbound: "proxy".into(),
        ..Default::default()
    };

    Ok(NormalizedProfile {
        nodes,
        groups: Vec::new(),
        route,
        dns: None,
        default_outbound: Some(default_outbound),
        parse_stats: ProfileParseStats {
            skipped_proxies: skipped,
            warnings,
            ..Default::default()
        },
    })
}

/// Reason a single URI line could not become a node.
#[derive(Debug)]
pub(crate) enum SkipReason {
    /// Recognized scheme but unsupported transport / feature (e.g. `ssr://`).
    Unsupported(String),
    /// Malformed or missing required fields.
    Incomplete(String),
}

impl From<String> for SkipReason {
    fn from(s: String) -> Self {
        SkipReason::Incomplete(s)
    }
}

fn parse_uri_line(line: &str, idx: usize) -> Result<NormalizedOutbound, SkipReason> {
    let scheme = scheme_of(line)
        .ok_or_else(|| SkipReason::Incomplete("unrecognized scheme in URI list line".into()))?;
    let rest = &line[scheme.len()..];
    let (rest, fragment) = split_fragment(rest);
    let mut name = decode_name(fragment);

    let mut outbound = match scheme {
        "ss://" => ss::parse_ss(rest)?,
        "ssr://" => {
            return Err(SkipReason::Unsupported(
                "ssr:// is not supported by sing-box; skipped".into(),
            ))
        }
        "vmess://" => {
            let (out, suggested) = v2ray::parse_vmess(rest)?;
            if name.is_none() {
                name = suggested;
            }
            out
        }
        "vless://" => v2ray::parse_vless(rest)?,
        "trojan://" => trojan::parse_trojan(rest)?,
        "hysteria://" => hysteria::parse_hysteria(rest)?,
        "hysteria2://" | "hy2://" => hysteria::parse_hysteria2(rest)?,
        "tuic://" => tuic::parse_tuic(rest)?,
        "socks://" | "socks5://" => basic::parse_socks(rest)?,
        "http://" | "https://" => basic::parse_http(rest, scheme == "https://")?,
        "wireguard://" => basic::parse_wireguard(rest)?,
        _ => {
            return Err(SkipReason::Unsupported(format!(
                "unsupported scheme {scheme}"
            )))
        }
    };

    let name = name.unwrap_or_else(|| format!("node-{}", idx + 1));

    outbound
        .as_object_mut()
        .unwrap()
        .insert("tag".into(), serde_json::json!(name));
    Ok(NormalizedOutbound {
        tag: name,
        outbound,
    })
}

/// Split `uri#fragment`; returns the fragment (still percent-encoded).
fn split_fragment(rest: &str) -> (&str, Option<&str>) {
    match rest.find('#') {
        Some(pos) => (&rest[..pos], Some(&rest[pos + 1..])),
        None => (rest, None),
    }
}

fn decode_name(fragment: Option<&str>) -> Option<String> {
    let fragment = fragment?;
    if fragment.trim().is_empty() {
        return None;
    }
    let decoded = percent_decode_str(fragment).decode_utf8_lossy().to_string();
    let trimmed = decoded.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Replace duplicate tags with `-2`, `-3`, ... suffixes (sing-box requires unique tags).
pub(crate) fn dedupe_tags(nodes: &mut [NormalizedOutbound]) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for node in nodes.iter_mut() {
        let count = seen.entry(node.tag.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            node.tag = format!("{}-{}", node.tag, *count);
            node.outbound
                .as_object_mut()
                .unwrap()
                .insert("tag".into(), serde_json::json!(node.tag));
        }
    }
}

/// Parse a `key=value&key2=value2` query string, percent-decoding both sides.
pub(crate) fn parse_query(query: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        let key = percent_decode_str(k).decode_utf8_lossy().to_string();
        let value = percent_decode_str(v).decode_utf8_lossy().to_string();
        out.push((key, value));
    }
    out
}

pub(crate) fn query_get<'a>(params: &'a [(String, String)], key: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
}

pub(crate) fn query_bool(params: &[(String, String)], key: &str) -> bool {
    query_get(params, key)
        .map(|v| {
            v == "1"
                || v.eq_ignore_ascii_case("true")
                || v.eq_ignore_ascii_case("yes")
                || v.eq_ignore_ascii_case("on")
        })
        .unwrap_or(false)
}

/// `host:port` from the authority part; `host` may be bracketed IPv6.
pub(crate) fn split_host_port(authority: &str) -> Result<(String, u16), SkipReason> {
    let trimmed = authority.trim();
    let base = match trimmed.split_once('?') {
        Some((a, _)) => a,
        None => trimmed,
    };
    let base = base.trim_matches('/');
    let (host, port) = if let Some(rest) = base.strip_prefix('[') {
        let end = rest
            .find(']')
            .ok_or_else(|| SkipReason::Incomplete("missing ']' in IPv6 address".into()))?;
        let host = &rest[..end];
        let after = &rest[end + 1..];
        if let Some(p) = after.strip_prefix(':') {
            (host, p)
        } else {
            (host, "443")
        }
    } else {
        match base.rsplit_once(':') {
            Some((h, p)) => (h, p),
            None => (base, "443"),
        }
    };

    let host = host.trim();
    if host.is_empty() {
        return Err(SkipReason::Incomplete("missing server address".into()));
    }
    let port: u16 = port
        .trim()
        .parse()
        .map_err(|_| SkipReason::Incomplete(format!("invalid port {port}")))?;
    if port == 0 {
        return Err(SkipReason::Incomplete(format!("invalid port {port}")));
    }
    Ok((host.to_string(), port))
}

/// Extract the `userinfo@` part and the authority; userinfo is percent-decoded.
pub(crate) fn split_userinfo(rest: &str) -> (Option<String>, &str) {
    match rest.find('@') {
        Some(pos) => {
            let userinfo = &rest[..pos];
            let decoded = percent_decode_str(userinfo).decode_utf8_lossy().to_string();
            (Some(decoded), &rest[pos + 1..])
        }
        None => (None, rest),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_uri_list() {
        assert!(looks_like_uri_list("vless://a@b:443#n"));
        assert!(looks_like_uri_list("vless://a@b:443#n\ntrojan://p@c:443"));
        assert!(looks_like_uri_list(
            "STATUS=ONLINE\nvless://a@b:443#n\nTOTAL=100"
        ));
        assert!(!looks_like_uri_list("proxies:\n  - name: x"));
        assert!(!looks_like_uri_list("{\"outbounds\":[]}"));
        assert!(!looks_like_uri_list(""));
        assert!(!looks_like_uri_list("plain text line"));
    }

    #[test]
    fn query_parsing_decodes() {
        let params = parse_query("insecure=1&sni=exa%6Dple.com&flag");
        assert_eq!(query_get(&params, "sni"), Some("example.com"));
        assert_eq!(query_get(&params, "FLAG"), None);
        assert!(query_bool(&params, "insecure"));
        assert!(!query_bool(&params, "sni"));
    }

    #[test]
    fn host_port_splitting() {
        assert_eq!(
            split_host_port("example.com:443").unwrap(),
            ("example.com".into(), 443)
        );
        assert_eq!(
            split_host_port("example.com").unwrap(),
            ("example.com".into(), 443)
        );
        assert_eq!(
            split_host_port("[2001:db8::1]:8080").unwrap(),
            ("2001:db8::1".into(), 8080)
        );
        assert!(split_host_port("example.com:0").is_err());
        assert!(split_host_port("").is_err());
    }

    #[test]
    fn fragment_name_decoding() {
        assert_eq!(decode_name(Some("HK%20Node")), Some("HK Node".into()));
        assert_eq!(
            decode_name(Some("%E5%BF%AB%E9%80%9F01")),
            Some("快速01".into())
        );
        assert_eq!(decode_name(None), None);
        assert_eq!(decode_name(Some("  ")), None);
    }

    #[test]
    fn legacy_ss_with_fragment_via_uri_list() {
        use base64::Engine;
        let inner = "aes-128-gcm:secret@1.2.3.4:8388";
        let encoded = base64::engine::general_purpose::STANDARD.encode(inner);
        let profile = parse_uri_list_profile(&format!("ss://{encoded}#SS-01")).unwrap();
        assert_eq!(profile.nodes.len(), 1);
        assert_eq!(profile.nodes[0].tag, "SS-01");
        assert_eq!(profile.nodes[0].outbound["server"], "1.2.3.4");
    }

    #[test]
    fn dedupe_duplicate_tags() {
        let mut nodes = vec![
            NormalizedOutbound {
                tag: "a".into(),
                outbound: serde_json::json!({"type":"socks","tag":"a"}),
            },
            NormalizedOutbound {
                tag: "a".into(),
                outbound: serde_json::json!({"type":"socks","tag":"a"}),
            },
            NormalizedOutbound {
                tag: "b".into(),
                outbound: serde_json::json!({"type":"socks","tag":"b"}),
            },
        ];
        dedupe_tags(&mut nodes);
        assert_eq!(nodes[0].tag, "a");
        assert_eq!(nodes[1].tag, "a-2");
        assert_eq!(nodes[2].tag, "b");
    }
}
