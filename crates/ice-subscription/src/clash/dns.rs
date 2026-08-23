//! Clash `dns` → sing-box `dns` block.

use serde_json::{json, Value};

pub fn parse_dns(doc: &Value) -> (Option<Value>, Vec<String>) {
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

    let final_tag = servers
        .last()
        .and_then(|s| s.get("tag"))
        .and_then(|v| v.as_str())
        .unwrap_or("dns-remote")
        .to_string();

    let mut out = json!({
        "servers": servers,
        "final": final_tag,
        "strategy": "prefer_ipv4",
    });

    let mut needs_local = servers.iter().any(server_needs_domain_resolver);

    if let Some(obj) = out.as_object_mut() {
        if dns.get("ipv6").and_then(|v| v.as_bool()) == Some(false) {
            obj.insert("strategy".into(), json!("ipv4_only"));
        }

        if let Some(range) = dns.get("fake-ip-range").and_then(|v| v.as_str()) {
            let fake_tag = "fakeip";
            servers.insert(
                0,
                json!({
                    "type": "fakeip",
                    "tag": fake_tag,
                    "inet4_range": range,
                }),
            );
            obj.insert("servers".into(), json!(servers));

            let mut dns_rules = Vec::new();
            if let Some(filters) = dns.get("fake-ip-filter").and_then(|v| v.as_array()) {
                for f in filters {
                    if let Some(s) = f.as_str() {
                        if s.starts_with("*.") {
                            needs_local = true;
                            dns_rules.push(json!({
                                "domain_suffix": [s.trim_start_matches("*.")],
                                "server": "local",
                            }));
                        } else if s.starts_with('.') {
                            needs_local = true;
                            dns_rules.push(json!({
                                "domain_suffix": [s.trim_start_matches('.')],
                                "server": "local",
                            }));
                        }
                    }
                }
            }
            dns_rules.push(json!({
                "query_type": ["A", "AAAA"],
                "server": fake_tag,
            }));
            obj.insert("rules".into(), json!(dns_rules));
        }

        // sing-box 1.12+: nameservers with a domain address must resolve their host
        // via another server (`domain_resolver`) — the `local` (OS resolver) server
        // also backs fake-ip-filter rules. Ensure it exists when anything needs it.
        if needs_local
            && !servers
                .iter()
                .any(|s| s.get("tag").and_then(|v| v.as_str()) == Some("local"))
        {
            servers.insert(0, json!({ "type": "local", "tag": "local" }));
            obj.insert("servers".into(), json!(servers));
        }

        // Clash `dns.listen` (LAN DNS server) has no sing-box 1.13 equivalent (the
        // `dns` inbound type was removed); it is dropped rather than kept.
        if dns.get("listen").is_some() {
            warnings.push("dns: listen unsupported by sing-box 1.13+; dropped".into());
        }
    }

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
