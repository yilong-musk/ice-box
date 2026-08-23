//! Subscription URL validation (scheme + SSRF guard for fetch targets).

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

use crate::error::SubscriptionError;
use ice_config::{is_fake_ip, is_restricted_fetch_host, is_restricted_ip};

/// Allowed schemes for subscription import/update.
const ALLOWED_SCHEMES: &[&str] = &["http", "https"];

/// Validate a user-supplied subscription URL before HTTP fetch.
pub fn validate_subscription_url(raw: &str) -> Result<(), SubscriptionError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(SubscriptionError::FetchFailed(
            "subscription URL is empty".into(),
        ));
    }

    let parsed = url::Url::parse(trimmed)
        .map_err(|e| SubscriptionError::FetchFailed(format!("invalid subscription URL: {e}")))?;

    if !ALLOWED_SCHEMES.contains(&parsed.scheme()) {
        return Err(SubscriptionError::FetchFailed(format!(
            "subscription URL scheme must be http or https, got {}",
            parsed.scheme()
        )));
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(SubscriptionError::FetchFailed(
            "subscription URL must not embed credentials".into(),
        ));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| SubscriptionError::FetchFailed("subscription URL missing host".into()))?;

    if is_restricted_fetch_host(host) {
        return Err(SubscriptionError::FetchFailed(format!(
            "subscription URL host is not allowed: {host}"
        )));
    }

    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| SubscriptionError::FetchFailed("subscription URL missing port".into()))?;
    validate_resolved_addresses(host, port)?;
    Ok(())
}

/// Redact subscription URL secrets before exposing to the WebView.
pub fn redact_subscription_url_for_ui(raw: &str) -> String {
    let Ok(parsed) = url::Url::parse(raw.trim()) else {
        return "***".into();
    };
    let host = parsed.host_str().unwrap_or("unknown");
    let path = parsed.path();
    let path = if path.is_empty() { "/" } else { path };
    format!("{}://{}{}", parsed.scheme(), host, path)
}

/// Resolve a subscription URL to vetted socket addresses for pinned connect.
pub fn resolve_allowed_fetch_addrs(
    url: &str,
) -> Result<(String, Vec<SocketAddr>), SubscriptionError> {
    let parsed = url::Url::parse(url.trim())
        .map_err(|e| SubscriptionError::FetchFailed(format!("invalid subscription URL: {e}")))?;

    let host = parsed
        .host_str()
        .ok_or_else(|| SubscriptionError::FetchFailed("subscription URL missing host".into()))?;

    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| SubscriptionError::FetchFailed("subscription URL missing port".into()))?;

    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_restricted_fetch_host(&ip.to_string()) {
            return Err(SubscriptionError::FetchFailed(format!(
                "subscription URL host is not allowed: {host}"
            )));
        }
        return Ok((host.to_string(), vec![SocketAddr::new(ip, port)]));
    }

    let addrs = lookup_host_socket_addrs(host, port)?;
    let allowed = filter_fetch_addrs(addrs)?;
    Ok((host.to_string(), allowed))
}

/// Whether every resolved address is a Clash / sing-box fake-ip mapping.
pub fn addrs_are_fake_ip(addrs: &[SocketAddr]) -> bool {
    !addrs.is_empty() && addrs.iter().all(|addr| is_fake_ip(addr.ip()))
}

/// Rewrite `url` to connect via `ip` while preserving path/query (HTTP pinned connects).
pub fn pin_url_to_ip(url: &str, ip: IpAddr) -> Result<String, SubscriptionError> {
    let mut parsed = url::Url::parse(url)
        .map_err(|e| SubscriptionError::FetchFailed(format!("invalid subscription URL: {e}")))?;
    let host = match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    };
    parsed
        .set_host(Some(&host))
        .map_err(|_| SubscriptionError::FetchFailed("pin url to ip: invalid host".into()))?;
    Ok(parsed.to_string())
}

fn lookup_host_socket_addrs(host: &str, port: u16) -> Result<Vec<SocketAddr>, SubscriptionError> {
    let host = host.trim().trim_matches(|c| c == '[' || c == ']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_restricted_fetch_host(&ip.to_string()) {
            return Err(SubscriptionError::FetchFailed(format!(
                "subscription URL host is not allowed: {host}"
            )));
        }
        return Ok(vec![SocketAddr::new(ip, port)]);
    }

    let addrs: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|e| {
            SubscriptionError::FetchFailed(format!("resolve subscription host {host}: {e}"))
        })?
        .collect();

    if addrs.is_empty() {
        return Err(SubscriptionError::FetchFailed(format!(
            "subscription URL host did not resolve: {host}"
        )));
    }
    Ok(addrs)
}

fn filter_fetch_addrs(addrs: Vec<SocketAddr>) -> Result<Vec<SocketAddr>, SubscriptionError> {
    let allowed: Vec<SocketAddr> = addrs
        .into_iter()
        .filter(|addr| is_fake_ip(addr.ip()) || !is_restricted_ip(addr.ip()))
        .collect();
    if allowed.is_empty() {
        return Err(SubscriptionError::FetchFailed(
            "subscription URL host resolves only to disallowed addresses".into(),
        ));
    }
    Ok(allowed)
}

/// Reject hostnames that resolve to loopback / private / link-local addresses (DNS rebinding).
fn validate_resolved_addresses(host: &str, port: u16) -> Result<(), SubscriptionError> {
    let addrs = lookup_host_socket_addrs(host, port)?;
    filter_fetch_addrs(addrs).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_url_to_ip_rewrites_host() {
        let pinned =
            pin_url_to_ip("https://example.com/sub.json", "1.2.3.4".parse().unwrap()).unwrap();
        assert_eq!(pinned, "https://1.2.3.4/sub.json");
    }

    #[test]
    fn resolve_allowed_fetch_addrs_blocks_private() {
        let err = resolve_allowed_fetch_addrs("https://127.0.0.1/sub").unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }

    #[test]
    fn accepts_public_https_url() {
        validate_subscription_url("https://cdn.example.com/sub.json").unwrap();
    }

    #[test]
    fn rejects_non_http_scheme() {
        let err = validate_subscription_url("file:///etc/passwd").unwrap_err();
        assert!(err.to_string().contains("scheme"));
    }

    #[test]
    fn rejects_loopback_and_private_hosts() {
        for u in [
            "http://127.0.0.1/sub",
            "https://localhost/x",
            "http://192.168.0.1/x",
            "http://10.0.0.1/x",
            "http://169.254.169.254/latest/meta-data",
            "http://metadata.google.internal/",
            "http://[::ffff:127.0.0.1]/sub",
        ] {
            let err = validate_subscription_url(u).unwrap_err();
            assert!(
                err.to_string().contains("not allowed"),
                "expected block for {u}, got {err}"
            );
        }
    }

    #[test]
    fn rejects_embedded_credentials() {
        let err = validate_subscription_url("https://user:pass@example.com/sub").unwrap_err();
        assert!(err.to_string().contains("credentials"));
    }

    #[test]
    fn validate_resolved_addresses_uses_url_port_not_443() {
        // Port-specific validation: http on 8080 should resolve (host, 8080), not 443.
        let err = validate_resolved_addresses("127.0.0.1", 8080).unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }

    #[test]
    fn addrs_are_fake_ip_detects_fake_pool() {
        let addrs = vec![SocketAddr::new("198.18.7.3".parse().unwrap(), 443)];
        assert!(addrs_are_fake_ip(&addrs));
        let mixed = vec![
            SocketAddr::new("198.18.7.3".parse().unwrap(), 443),
            SocketAddr::new("104.26.4.218".parse().unwrap(), 443),
        ];
        assert!(!addrs_are_fake_ip(&mixed));
    }

    #[test]
    fn redact_subscription_url_strips_query_and_userinfo() {
        let redacted = redact_subscription_url_for_ui(
            "https://token:secret@sub.example.com/path/to/sub?key=abc&token=xyz",
        );
        assert_eq!(redacted, "https://sub.example.com/path/to/sub");
    }
}
