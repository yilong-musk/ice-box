//! Loopback / listen-address helpers shared across crates.

use std::net::IpAddr;

fn normalize_host(host: &str) -> &str {
    host.trim().trim_matches(|c| c == '[' || c == ']')
}

/// Clash / sing-box fake-ip pool (RFC 2544 benchmarking range).
pub fn is_fake_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 198 && (o[1] == 18 || o[1] == 19)
        }
        IpAddr::V6(_) => false,
    }
}

/// Whether an IP must be rejected for outbound fetches (SSRF guard).
pub fn is_restricted_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_unspecified()
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.octets() == [169, 254, 169, 254]
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_restricted_ip(IpAddr::V4(v4));
            }
            v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                || (v6.octets()[0] & 0xfe == 0xfc) // unique local
                || (v6.segments()[0] & 0xffc0 == 0xfe80) // link-local
        }
    }
}

fn ip_is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => {
            v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback()) || v6.is_loopback()
        }
    }
}

/// Whether `host` is an allowed loopback bind/listen target for Clash API.
pub fn is_loopback_host(host: &str) -> bool {
    let host = host.trim();
    if matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]") {
        return true;
    }
    if let Ok(ip) = normalize_host(host).parse::<IpAddr>() {
        return ip_is_loopback(ip);
    }
    false
}

/// Whether `host` must be rejected for outbound fetches (SSRF guard).
pub fn is_restricted_fetch_host(host: &str) -> bool {
    let host = normalize_host(host);
    if host.is_empty() {
        return true;
    }

    if is_loopback_host(host) {
        return true;
    }

    let lower = host.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "metadata.google.internal" | "metadata.goog" | "169.254.169.254" | "0.0.0.0" | "::"
    ) {
        return true;
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_restricted_ip(ip);
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_hosts_accepted() {
        for h in ["127.0.0.1", "localhost", "::1", "[::1]"] {
            assert!(is_loopback_host(h), "{h}");
        }
    }

    #[test]
    fn fake_ip_detected() {
        let ip: IpAddr = "198.18.7.3".parse().unwrap();
        assert!(is_fake_ip(ip));
        assert!(!is_restricted_ip(ip));
    }

    #[test]
    fn restricted_fetch_blocks_internal_targets() {
        for h in [
            "127.0.0.1",
            "localhost",
            "10.0.0.1",
            "192.168.1.1",
            "172.16.0.1",
            "169.254.169.254",
            "metadata.google.internal",
            "[::1]",
            "::ffff:127.0.0.1",
            "[::ffff:127.0.0.1]",
        ] {
            assert!(is_restricted_fetch_host(h), "{h}");
        }
        assert!(!is_restricted_fetch_host("example.com"));
        assert!(!is_restricted_fetch_host("8.8.8.8"));
    }

    #[test]
    fn ipv4_mapped_loopback_is_restricted_ip() {
        let ip: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        assert!(is_restricted_ip(ip));
    }
}
