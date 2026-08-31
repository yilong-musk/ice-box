//! Shared auto-route model and route-probe helpers (plan §5 T2).
//!
//! The sub-range sets sing-box installs for `auto_route` on macOS (verified
//! live in the T0 spike). Both the host-free fake backend and the macOS
//! backend derive the owned-route set from here, so tests assert the same
//! shape the native path produces. The probe helpers are shared by the
//! macOS and Windows backends (host-free parsing, never OS mutations).

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

/// Convert a CIDR ownership key into an address accepted by a route lookup.
///
/// sing-box may split a broad auto-route around excluded ranges, so the
/// queried address only needs to be a member of the expected prefix. Adding
/// one avoids a network-only lookup; `::1` is skipped because it is reserved
/// for loopback and would resolve to `lo0` instead of the TUN split route.
/// (macOS `route -n get` accepts an address, not CIDR notation; Windows
/// `route print` is probed the same way.)
pub fn route_probe_address(destination: &str) -> String {
    let Some((address, prefix)) = destination.split_once('/') else {
        return destination.to_string();
    };
    let Ok(prefix) = prefix.parse::<u8>() else {
        return address.to_string();
    };
    if let Ok(parsed) = address.parse::<std::net::Ipv4Addr>() {
        if prefix == 32 {
            return parsed.to_string();
        }
        let value = u32::from(parsed).saturating_add(1);
        return std::net::Ipv4Addr::from(value).to_string();
    }
    if let Ok(parsed) = address.parse::<std::net::Ipv6Addr>() {
        if prefix == 128 {
            return parsed.to_string();
        }
        let mut value = u128::from(parsed).saturating_add(1);
        if value == 1 {
            value = 2;
        }
        return std::net::Ipv6Addr::from(value).to_string();
    }
    address.to_string()
}

/// Multiple probe addresses spread across a destination prefix, used when a
/// single probe can be shadowed by a more-specific local route.
///
/// The auto-route sub-ranges are broad (e.g. `128.0.0.0/1`), and sing-box
/// keeps the LAN subnet on the host route, so a probe that lands inside the
/// LAN resolves to the LAN interface instead of the utun — a single probe
/// would then report the whole route as missing forever. Spreading the
/// probes (base+1, quarters, top) means the route is accepted as owned as
/// long as *any* probe still resolves to the adapter; a fully missing route
/// fails every probe. The first probe matches the legacy single-probe
/// behavior.
pub fn route_probe_addresses(destination: &str) -> Vec<String> {
    let Some((address, prefix)) = destination.split_once('/') else {
        return vec![destination.to_string()];
    };
    let Ok(prefix) = prefix.parse::<u8>() else {
        return vec![address.to_string()];
    };
    if let Ok(parsed) = address.parse::<std::net::Ipv4Addr>() {
        if prefix >= 32 {
            return vec![parsed.to_string()];
        }
        let base = u64::from(u32::from(parsed));
        let size = 1u64 << (32 - prefix);
        return probe_offsets(size as u128)
            .into_iter()
            .map(|offset| std::net::Ipv4Addr::from((base as u128 + offset) as u32).to_string())
            .collect();
    }
    if let Ok(parsed) = address.parse::<std::net::Ipv6Addr>() {
        if prefix >= 128 {
            return vec![parsed.to_string()];
        }
        let base = u128::from(parsed);
        let size = 1u128 << (128 - prefix);
        return probe_offsets(size)
            .into_iter()
            .map(|offset| std::net::Ipv6Addr::from(base + offset).to_string())
            .collect();
    }
    vec![address.to_string()]
}

/// Candidate offsets inside a prefix of `size` addresses: base+1, quarters,
/// and the top (minus the last address, which is the IPv4 broadcast). The
/// network address itself (offset 0) is skipped; duplicates collapse.
fn probe_offsets(size: u128) -> Vec<u128> {
    let mut offsets = Vec::new();
    for candidate in [
        1u128,
        size / 4,
        size / 2,
        size - size / 4,
        size.saturating_sub(2),
    ] {
        if candidate > 0 && candidate < size && !offsets.contains(&candidate) {
            offsets.push(candidate);
        }
    }
    offsets
}

/// `0xfffffffc` → 30 (IPv4 netmask string from `ifconfig` / Windows
/// `route print -4` netmask columns).
pub fn netmask_to_prefix(netmask: &str) -> Option<u32> {
    let raw = u32::from_str_radix(netmask.trim_start_matches("0x"), 16).ok()?;
    Some(raw.count_ones())
}

/// `255.255.255.252` → 30 (dotted-decimal netmask from `route print -4`).
/// `0.0.0.0` → 0 (the default route). Rejects non-contiguous masks.
pub fn dotted_netmask_to_prefix(netmask: &str) -> Option<u32> {
    let octets: Vec<u32> = netmask
        .split('.')
        .map(|octet| octet.parse::<u32>().ok())
        .collect::<Option<_>>()?;
    if octets.len() != 4 {
        return None;
    }
    let raw = (octets[0] << 24) | (octets[1] << 16) | (octets[2] << 8) | octets[3];
    let inverse = !raw;
    if inverse & inverse.wrapping_add(1) != 0 {
        return None;
    }
    Some(raw.count_ones())
}

/// Longest-prefix route lookup over parsed route tables: the most specific
/// entry containing `probe` wins (matches the kernel's routing decision).
/// `routes` is a list of `(prefix, prefix_bits)`; returns the index of the
/// most specific match, or `None` when nothing contains the probe address.
pub fn longest_prefix_route(routes: &[(String, u32)], probe: &str) -> Option<usize> {
    let probe_v4 = probe.parse::<std::net::Ipv4Addr>().ok();
    let probe_v6 = probe.parse::<std::net::Ipv6Addr>().ok();
    let mut best: Option<(usize, u32)> = None;
    for (index, (network, bits)) in routes.iter().enumerate() {
        let contains = if let Some(probe) = probe_v4 {
            network
                .parse::<std::net::Ipv4Addr>()
                .ok()
                .map(|net| ipv4_contains(net, *bits, probe))
                .unwrap_or(false)
        } else if let Some(probe) = probe_v6 {
            network
                .parse::<std::net::Ipv6Addr>()
                .ok()
                .map(|net| ipv6_contains(net, *bits, probe))
                .unwrap_or(false)
        } else {
            false
        };
        if contains && best.is_none_or(|(_, best_bits)| *bits > best_bits) {
            best = Some((index, *bits));
        }
    }
    best.map(|(index, _)| index)
}

fn ipv4_contains(network: std::net::Ipv4Addr, bits: u32, probe: std::net::Ipv4Addr) -> bool {
    let mask = if bits == 0 {
        0u32
    } else {
        u32::MAX << (32 - bits)
    };
    (u32::from(network) & mask) == (u32::from(probe) & mask)
}

fn ipv6_contains(network: std::net::Ipv6Addr, bits: u32, probe: std::net::Ipv6Addr) -> bool {
    if bits == 0 {
        return true;
    }
    let mask = if bits == 128 {
        u128::MAX
    } else {
        u128::MAX << (128 - bits)
    };
    (u128::from(network) & mask) == (u128::from(probe) & mask)
}

/// Strip an optional `/prefix` suffix (CIDR key → bare IP). IPv6 addresses
/// reported by Windows `netsh` carry no prefix, so presence checks compare
/// on the bare address.
pub fn address_key(cidr_or_address: &str) -> &str {
    cidr_or_address.split('/').next().unwrap_or(cidr_or_address)
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

    #[test]
    fn route_probe_address_converts_cidr_to_non_reserved_member() {
        assert_eq!(route_probe_address("1.0.0.0/8"), "1.0.0.1");
        assert_eq!(route_probe_address("10.0.0.2/30"), "10.0.0.3");
        assert_eq!(route_probe_address("::/1"), "::2");
        assert_eq!(route_probe_address("8000::/1"), "8000::1");
        assert_eq!(
            route_probe_address("fdfe:dcba:9876::1/128"),
            "fdfe:dcba:9876::1"
        );
        assert_eq!(route_probe_address("not-a-cidr"), "not-a-cidr");
    }

    #[test]
    fn route_probe_addresses_spread_across_the_prefix() {
        let v4 = route_probe_addresses("128.0.0.0/1");
        assert_eq!(v4.first().map(String::as_str), Some("128.0.0.1"));
        assert!(v4.len() >= 4, "spread probes: {v4:?}");
        assert_eq!(
            v4.last().map(String::as_str),
            Some("255.255.255.254"),
            "top probe skips the broadcast address"
        );
        assert!(v4.iter().all(|p| {
            let value = u32::from(p.parse::<std::net::Ipv4Addr>().unwrap());
            value >= u32::from("128.0.0.0".parse::<std::net::Ipv4Addr>().unwrap())
        }));

        let v6 = route_probe_addresses("100::/8");
        assert_eq!(v6.first().map(String::as_str), Some("100::1"));
        assert!(v6.len() >= 4, "spread probes: {v6:?}");
        assert!(v6.iter().all(|p| p.parse::<std::net::Ipv6Addr>().is_ok()));

        // Host-sized prefixes keep a single probe; bare addresses pass through.
        assert_eq!(route_probe_addresses("10.0.0.1/32"), ["10.0.0.1"]);
        assert_eq!(
            route_probe_addresses("fdfe:dcba:9876::1/128"),
            ["fdfe:dcba:9876::1"]
        );
        assert_eq!(route_probe_addresses("not-a-cidr"), ["not-a-cidr"]);
    }

    #[test]
    fn route_probe_addresses_first_matches_single_probe() {
        for destination in [
            "1.0.0.0/8",
            "64.0.0.0/2",
            "128.0.0.0/1",
            "100::/8",
            "8000::/1",
        ] {
            assert_eq!(
                route_probe_addresses(destination).first(),
                Some(&route_probe_address(destination)),
                "{destination}"
            );
        }
    }

    #[test]
    fn netmask_to_prefix_counts_bits() {
        assert_eq!(netmask_to_prefix("0xfffffffc"), Some(30));
        assert_eq!(netmask_to_prefix("0xffffffff"), Some(32));
        assert_eq!(netmask_to_prefix("0xffffff00"), Some(24));
        assert_eq!(netmask_to_prefix("nope"), None);
    }

    #[test]
    fn dotted_netmask_to_prefix_counts_bits() {
        assert_eq!(dotted_netmask_to_prefix("0.0.0.0"), Some(0));
        assert_eq!(dotted_netmask_to_prefix("255.255.255.252"), Some(30));
        assert_eq!(dotted_netmask_to_prefix("255.255.255.255"), Some(32));
        assert_eq!(dotted_netmask_to_prefix("255.255.0.0"), Some(16));
        assert_eq!(
            dotted_netmask_to_prefix("255.0.255.0"),
            None,
            "non-contiguous mask"
        );
        assert_eq!(dotted_netmask_to_prefix("nope"), None);
    }

    #[test]
    fn longest_prefix_route_picks_the_most_specific_match() {
        let routes = vec![
            ("0.0.0.0".to_string(), 0u32),
            ("10.0.0.0".to_string(), 8u32),
            ("10.0.0.0".to_string(), 30u32),
        ];
        assert_eq!(longest_prefix_route(&routes, "10.0.0.3"), Some(2));
        assert_eq!(longest_prefix_route(&routes, "10.5.5.5"), Some(1));
        assert_eq!(longest_prefix_route(&routes, "192.168.1.1"), Some(0));
        assert_eq!(longest_prefix_route(&routes, "999.1.1.1"), None);
    }

    #[test]
    fn longest_prefix_route_matches_ipv6_prefixes() {
        let routes = vec![
            ("::".to_string(), 0u32),
            ("fdfe:dcba:9876::".to_string(), 126u32),
        ];
        assert_eq!(longest_prefix_route(&routes, "fdfe:dcba:9876::1"), Some(1));
        assert_eq!(longest_prefix_route(&routes, "2001:db8::1"), Some(0));
    }

    #[test]
    fn address_key_strips_prefix() {
        assert_eq!(address_key("10.0.0.1/30"), "10.0.0.1");
        assert_eq!(address_key("fdfe:dcba:9876::1"), "fdfe:dcba:9876::1");
        assert_eq!(address_key("fdfe:dcba:9876::1/126"), "fdfe:dcba:9876::1");
    }
}
