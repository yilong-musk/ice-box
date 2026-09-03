//! Clash `dns` → sing-box `dns` block.
//!
//! Windows emission differs (design note tun-windows-t0 §1.2, locked
//! 2026-09-03): UDP upstreams are dialed by the core's own UDP outbound and
//! captured by its own TUN (always fail); fake-ip answers (198.18.0.0/15)
//! are outside the Windows auto-route sub-ranges and unreachable; and the
//! OS-resolver-backed `local` server re-enters the TUN via the adapter DNS,
//! risking a query loop. The Windows shape therefore forces TCP transports,
//! drops the fakeip server and the `local` server, and rewires every
//! `local` reference to the DNS `final` tag.

use serde_json::{json, Value};

pub fn parse_dns(doc: &Value) -> (Option<Value>, Vec<String>) {
    parse_dns_on(doc, cfg!(target_os = "windows"))
}

/// Platform-selectable DNS builder so the Windows shape is testable on any
/// host.
pub fn parse_dns_on(doc: &Value, windows: bool) -> (Option<Value>, Vec<String>) {
    let mut warnings = Vec::new();
    let Some(dns) = doc.get("dns") else {
        return (None, warnings);
    };

    let enabled = dns.get("enable").and_then(|v| v.as_bool()).unwrap_or(true);
    if !enabled {
        return (None, warnings);
    }

    let mut servers = Vec::new();
    let mut tag_idx = 0usize;

    if let Some(list) = dns.get("nameserver").and_then(|v| v.as_array()) {
        for entry in list {
            if let Some(s) = entry.as_str() {
                if let Some(server) = map_dns_server(s, &mut tag_idx) {
                    servers.push(server);
                } else {
                    warnings.push(format!("dns: unsupported nameserver {s}"));
                }
            }
        }
    }

    if let Some(list) = dns.get("fallback").and_then(|v| v.as_array()) {
        for entry in list {
            if let Some(s) = entry.as_str() {
                if let Some(server) = map_dns_server(s, &mut tag_idx) {
                    servers.push(server);
                }
            }
        }
    }

    if servers.is_empty() {
        warnings.push("dns: no usable nameserver entries".into());
        return (None, warnings);
    }

    // Windows: UDP upstreams are captured by the core's own TUN; rewrite them
    // to DoT on the same host so every server is TCP-capable.
    if windows {
        for server in &mut servers {
            if server.get("type").and_then(|v| v.as_str()) == Some("udp") {
                if let Some(obj) = server.as_object_mut() {
                    obj.insert("type".into(), json!("tls"));
                    obj.insert("server_port".into(), json!(853));
                }
            }
        }
    }

    // Windows: the resolution *anchor* must be an IP-hosted TCP server. A
    // domain-hosted server (DoH) needs a `domain_resolver` for its own host,
    // and a resolver chain that ends in itself is a circular dependency —
    // sing-box 1.13 aborts the DNS service at startup
    // (`FATAL ... circular server dependency`, host spike V12). The anchor is
    // the last IP-hosted server; a profile with only domain-hosted servers
    // gets a builtin DoT anchor. The `final` and every `domain_resolver`
    // reference point at the anchor on Windows.
    let anchor_tag = if windows {
        let anchor = servers
            .iter()
            .rev()
            .find(|server| {
                server
                    .get("server")
                    .and_then(|v| v.as_str())
                    .is_some_and(|host| host.parse::<std::net::IpAddr>().is_ok())
            })
            .map(|server| {
                server
                    .get("tag")
                    .and_then(|v| v.as_str())
                    .unwrap_or("dns-remote")
                    .to_string()
            })
            .unwrap_or_else(|| {
                let tag = format!("dns-remote-{tag_idx}");
                tag_idx += 1;
                servers.push(json!({
                    "type": "tls",
                    "tag": tag,
                    "server": "223.5.5.5",
                    "server_port": 853,
                }));
                tag
            });
        Some(anchor)
    } else {
        None
    };

    let final_tag = anchor_tag.unwrap_or_else(|| {
        servers
            .last()
            .and_then(|s| s.get("tag"))
            .and_then(|v| v.as_str())
            .unwrap_or("dns-remote")
            .to_string()
    });

    let mut out = json!({
        "final": final_tag,
        "strategy": if windows { "ipv4_only" } else { "prefer_ipv4" },
    });

    let mut needs_local = servers.iter().any(server_needs_domain_resolver);
    let mut dns_rules: Vec<Value> = Vec::new();

    if !windows && dns.get("ipv6").and_then(|v| v.as_bool()) == Some(false) {
        out["strategy"] = json!("ipv4_only");
    }

    if let Some(range) = dns.get("fake-ip-range").and_then(|v| v.as_str()) {
        if windows {
            // The fake-ip range (198.18.0.0/15) is not captured by the
            // Windows auto-route sub-ranges; fake-ip answers are
            // unreachable (V11). The filter suffixes still resolve via a
            // real server below; the A/AAAA → fakeip rule is dropped.
        } else {
            servers.insert(
                0,
                json!({
                    "type": "fakeip",
                    "tag": "fakeip",
                    "inet4_range": range,
                }),
            );
        }

        if let Some(filters) = dns.get("fake-ip-filter").and_then(|v| v.as_array()) {
            for f in filters {
                if let Some(s) = f.as_str() {
                    if s.starts_with("*.") {
                        needs_local = true;
                        dns_rules.push(json!({
                            "domain_suffix": [s.trim_start_matches("*.")],
                            "server": if windows { final_tag.clone() } else { "local".to_string() },
                        }));
                    } else if s.starts_with('.') {
                        needs_local = true;
                        dns_rules.push(json!({
                            "domain_suffix": [s.trim_start_matches('.')],
                            "server": if windows { final_tag.clone() } else { "local".to_string() },
                        }));
                    }
                }
            }
        }
        if !windows {
            dns_rules.push(json!({
                "query_type": ["A", "AAAA"],
                "server": "fakeip",
            }));
        }
    }

    // Windows: `local` re-enters the TUN via the adapter DNS; point every
    // `domain_resolver` at the `final` tag instead and never emit `local`.
    if windows {
        for server in &mut servers {
            if server.get("domain_resolver").and_then(|v| v.as_str()) == Some("local") {
                if let Some(obj) = server.as_object_mut() {
                    obj.insert("domain_resolver".into(), json!(final_tag.clone()));
                }
            }
        }
    } else if needs_local
        && !servers
            .iter()
            .any(|s| s.get("tag").and_then(|v| v.as_str()) == Some("local"))
    {
        // sing-box 1.12+: nameservers with a domain address must resolve their
        // host via another server (`domain_resolver`) — the `local` (OS
        // resolver) server also backs fake-ip-filter rules.
        servers.insert(0, json!({ "type": "local", "tag": "local" }));
    }

    let obj = out.as_object_mut().expect("out is an object");
    obj.insert("servers".into(), json!(servers));
    if !dns_rules.is_empty() {
        obj.insert("rules".into(), json!(dns_rules));
    }

    // Clash `dns.listen` (LAN DNS server) has no sing-box 1.13 equivalent (the
    // `dns` inbound type was removed); it is dropped silently rather than kept.

    (Some(out), warnings)
}

/// Whether a mapped nameserver dials a domain host (needs `domain_resolver`).
fn server_needs_domain_resolver(server: &Value) -> bool {
    let Some(host) = server.get("server").and_then(|v| v.as_str()) else {
        return false;
    };
    host.parse::<std::net::IpAddr>().is_err()
}

fn map_dns_server(raw: &str, tag_idx: &mut usize) -> Option<Value> {
    let tag = format!("dns-remote-{tag_idx}");
    *tag_idx += 1;

    let mut server = if raw.starts_with("https://") {
        let host = raw.trim_start_matches("https://").split('/').next()?;
        json!({
            "type": "https",
            "tag": tag,
            "server": host,
            "server_port": 443,
            "path": "/dns-query",
        })
    } else if raw.starts_with("tls://") {
        let host_port = raw.trim_start_matches("tls://");
        let (host, port) = host_port.rsplit_once(':').unwrap_or((host_port, "853"));
        json!({
            "type": "tls",
            "tag": tag,
            "server": host,
            "server_port": port.parse::<u16>().unwrap_or(853),
        })
    } else if raw.starts_with("udp://") {
        let host_port = raw.trim_start_matches("udp://");
        let (host, port) = host_port.rsplit_once(':').unwrap_or((host_port, "53"));
        json!({
            "type": "udp",
            "tag": tag,
            "server": host,
            "server_port": port.parse::<u16>().unwrap_or(53),
        })
    } else if raw.parse::<std::net::IpAddr>().is_ok() || !raw.contains("://") {
        json!({
            "type": "udp",
            "tag": tag,
            "server": raw,
            "server_port": 53,
        })
    } else {
        return None;
    };

    if server_needs_domain_resolver(&server) {
        server
            .as_object_mut()
            .expect("mapped server is object")
            .insert("domain_resolver".into(), json!("local"));
    }
    Some(server)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn production_like_doc() -> Value {
        json!({
            "dns": {
                "enable": true,
                "ipv6": false,
                "nameserver": [
                    "119.29.29.29",
                    "223.5.5.5",
                    "tls://119.29.29.29",
                    "tls://223.5.5.5",
                    "https://dns.pub/dns-query",
                    "https://dns.alidns.com/dns-query",
                ],
                "fake-ip-range": "198.18.0.0/15",
                "fake-ip-filter": [
                    "*.lan",
                    "*.local",
                    "*.msftconnecttest.com",
                    "*.msftncsi.com",
                    "time.edu.cn",
                ],
            }
        })
    }

    #[test]
    fn generic_shape_keeps_fakeip_and_local() {
        let (dns, warnings) = parse_dns_on(&production_like_doc(), false);
        assert!(warnings.is_empty());
        let dns = dns.expect("dns block");
        let tags: Vec<&str> = dns["servers"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| s["tag"].as_str())
            .collect();
        assert!(tags.contains(&"fakeip"));
        assert!(tags.contains(&"local"));
        let fakeip = dns["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["tag"] == "fakeip")
            .expect("fakeip server");
        assert_eq!(fakeip["type"], "fakeip");
        assert_eq!(fakeip["inet4_range"], "198.18.0.0/15");
        assert_eq!(dns["strategy"], "ipv4_only", "profile ipv6:false");
        assert_eq!(dns["servers"][0]["type"], "local", "local precedes fakeip");
        let udp = dns["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["tag"] == "dns-remote-0")
            .expect("bare nameserver");
        assert_eq!(udp["type"], "udp", "bare IP stays udp");
        let rules = dns["rules"].as_array().unwrap();
        assert!(
            rules.iter().any(|r| r["server"] == "fakeip"),
            "query_type → fakeip rule must exist"
        );
        assert!(
            rules.iter().any(|r| r["server"] == "local"),
            "fake-ip-filter suffixes resolve via local"
        );
    }

    #[test]
    fn windows_shape_forces_tcp_drops_fakeip_and_local() {
        let (dns, warnings) = parse_dns_on(&production_like_doc(), true);
        assert!(warnings.is_empty());
        let dns = dns.expect("dns block");
        let tags: Vec<&str> = dns["servers"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| s["tag"].as_str())
            .collect();
        assert!(
            !tags.contains(&"fakeip"),
            "fakeip is unreachable on Windows (V11); must not be emitted"
        );
        assert!(
            !tags.contains(&"local"),
            "local re-enters the TUN on Windows; must not be emitted"
        );
        for server in dns["servers"].as_array().unwrap() {
            assert_ne!(
                server["type"], "udp",
                "UDP upstreams are captured by the core's own TUN on Windows"
            );
            if server["type"] == "tls" {
                assert_eq!(server["server_port"], 853, "DoT upstreams dial 853");
            }
        }
        let converted = dns["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["tag"] == "dns-remote-0")
            .expect("bare nameserver");
        assert_eq!(
            converted["type"], "tls",
            "bare IP nameservers become DoT on Windows"
        );
        assert_eq!(
            converted["server_port"], 853,
            "converted UDP upstreams become DoT"
        );
        assert_eq!(dns["strategy"], "ipv4_only", "forced on Windows");
        let rules = dns["rules"].as_array().unwrap();
        assert!(
            !rules.iter().any(|r| r["server"] == "fakeip"),
            "no query_type → fakeip rule on Windows"
        );
        for rule in rules {
            assert_ne!(
                rule["server"], "local",
                "fake-ip-filter suffixes resolve via a TCP server on Windows"
            );
        }
        assert_eq!(
            dns["final"], "dns-remote-3",
            "the final must be the last IP-hosted TCP server (a domain-hosted final is a circular dependency)"
        );
        let doh = dns["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["tag"] == "dns-remote-4")
            .expect("DoH server");
        assert_eq!(
            doh["domain_resolver"], "dns-remote-3",
            "domain_resolver rewired to the IP-hosted anchor, never local"
        );
    }

    #[test]
    fn windows_shape_rewires_domain_resolver_to_the_final_tag() {
        let doc = json!({
            "dns": {
                "nameserver": ["223.5.5.5", "https://dns.pub/dns-query"],
            }
        });
        let (dns, _) = parse_dns_on(&doc, true);
        let dns = dns.expect("dns block");
        let doh = dns["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["tag"] == "dns-remote-1")
            .expect("DoH server");
        assert_eq!(
            dns["final"], "dns-remote-0",
            "a domain-hosted final must fall back to the IP-hosted anchor"
        );
        assert_eq!(
            doh["domain_resolver"], "dns-remote-0",
            "domain host resolves via the IP-hosted anchor, never local"
        );
    }
}
