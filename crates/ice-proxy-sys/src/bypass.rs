//! Bypass domain lists for system proxy (architecture §13.1).

/// Domains that must never go through the mixed inbound (all platforms).
///
/// On Windows, prefer [`BYPASS_WINDOWS`] — a bare `::1` in WinInet
/// `ProxyOverride` / `INTERNET_PER_CONN_PROXY_BYPASS` returns
/// `ERROR_INVALID_PARAMETER` (87). The OS itself stores `[::1]`.
pub const BYPASS_COMMON: &[&str] = &["localhost", "127.0.0.1", "::1"];

/// Windows WinInet ProxyOverride tokens (IPv6 loopback must be bracketed).
pub const BYPASS_WINDOWS: &[&str] = &["localhost", "127.0.0.1", "[::1]", "<local>"];

/// Extra Windows ProxyOverride token (WinInet). Kept for callers that append
/// to a common list; prefer [`BYPASS_WINDOWS`] for a complete Windows list.
pub const BYPASS_WINDOWS_EXTRA: &[&str] = &["<local>"];

/// Bypass list for the current OS (used by apply).
pub fn bypass_domains() -> Vec<&'static str> {
    #[cfg(target_os = "windows")]
    {
        BYPASS_WINDOWS.to_vec()
    }
    #[cfg(not(target_os = "windows"))]
    {
        BYPASS_COMMON.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g4_1_bypass_list_contains_required_hosts() {
        assert!(BYPASS_COMMON.contains(&"localhost"));
        assert!(BYPASS_COMMON.contains(&"127.0.0.1"));
        assert!(BYPASS_COMMON.contains(&"::1"));

        let list = bypass_domains();
        assert!(list.contains(&"localhost"));
        assert!(list.contains(&"127.0.0.1"));

        #[cfg(target_os = "windows")]
        {
            assert!(
                list.contains(&"[::1]"),
                "WinInet rejects bare ::1 in ProxyOverride (error 87)"
            );
            assert!(!list.contains(&"::1"));
            assert!(list.contains(&"<local>"));
            assert_eq!(list, BYPASS_WINDOWS);
        }
        #[cfg(not(target_os = "windows"))]
        {
            assert!(list.contains(&"::1"));
        }
    }
}
