//! Shared auto-route model (plan §5 T2).
//!
//! The sub-range sets sing-box installs for `auto_route` on macOS (verified
//! live in the T0 spike). Both the host-free fake backend and the macOS
//! backend derive the owned-route set from here, so tests assert the same
//! shape the native path produces.

use crate::backend::TunConfig;

/// Sub-ranges sing-box installs for `auto_route` on macOS (the darwin
/// sub-range trick — `autoRouteUseSubRanges`, T0 spike §5) when the config
/// carries IPv4 addresses.
pub const AUTO_ROUTE_RANGES: &[&str] = &[
    "1.0.0.0/8",
    "2.0.0.0/7",
    "4.0.0.0/6",
    "8.0.0.0/5",
    "16.0.0.0/4",
    "32.0.0.0/3",
    "64.0.0.0/2",
    "128.0.0.0/1",
];

/// IPv6 sub-ranges installed alongside the IPv4 ranges whenever the config
/// carries IPv6 addresses (dual-stack lock, architecture §24.5 point 4).
/// sing-box starts at `100::/8` on Darwin; a route for `::/1` would be treated
/// as a default route by the macOS route API and is not part of its sub-range
/// set.
pub const AUTO_ROUTE_RANGES_V6: &[&str] = &[
    "100::/8", "200::/7", "400::/6", "800::/5", "1000::/4", "2000::/3", "4000::/2", "8000::/1",
];

pub fn has_v4(addresses: &[String]) -> bool {
    addresses.iter().any(|cidr| !cidr.contains(':'))
}

pub fn has_v6(addresses: &[String]) -> bool {
    addresses.iter().any(|cidr| cidr.contains(':'))
}

/// All destinations the native path installs for this config: the IPv4 and
/// IPv6 sub-range sets when `auto_route` is on (the macOS sub-range trick,
/// T0 spike §5). Connected routes for TUN addresses are deliberately not
/// included: the locked `route_exclude_address` policy sends those private or
/// ULA destinations through the pre-existing host route, as confirmed by the
/// live acceptance gate.
pub fn auto_route_destinations(config: &TunConfig) -> Vec<String> {
    let mut destinations = Vec::new();
    if config.auto_route {
        if has_v4(&config.addresses) {
            destinations.extend(AUTO_ROUTE_RANGES.iter().map(|s| (*s).to_string()));
        }
        if has_v6(&config.addresses) {
            destinations.extend(AUTO_ROUTE_RANGES_V6.iter().map(|s| (*s).to_string()));
        }
    }
    destinations
}

/// Parse an IPv6 address (optionally with a `/prefix` suffix) into 8
/// 16-bit groups; `None` when malformed.
pub fn ipv6_groups(addr: &str) -> Option<[u16; 8]> {
    let (addr, _) = addr.split_once('/').unwrap_or((addr, ""));
    let parsed: std::net::Ipv6Addr = addr.parse().ok()?;
    let octets = parsed.octets();
    let mut groups = [0u16; 8];
    for (i, group) in groups.iter_mut().enumerate() {
        *group = u16::from_be_bytes([octets[i * 2], octets[i * 2 + 1]]);
    }
    Some(groups)
}

pub fn format_ipv6(groups: [u16; 8]) -> String {
    groups.map(|g| format!("{g:x}")).join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn darwin_ipv6_auto_routes_start_at_100() {
        assert_eq!(AUTO_ROUTE_RANGES_V6.first(), Some(&"100::/8"));
        assert_eq!(AUTO_ROUTE_RANGES_V6.last(), Some(&"8000::/1"));
        assert!(!AUTO_ROUTE_RANGES_V6.contains(&"::/1"));
    }
}
