//! Bypass domain lists for system proxy (architecture §13.1).

/// Domains that must never go through the mixed inbound (all platforms).
pub const BYPASS_COMMON: &[&str] = &["localhost", "127.0.0.1", "::1"];

/// Extra Windows ProxyOverride token (WinInet).
pub const BYPASS_WINDOWS_EXTRA: &[&str] = &["<local>"];

/// Bypass list for the current OS (used by apply).
pub fn bypass_domains() -> Vec<&'static str> {
    #[cfg(target_os = "windows")]
    {
        let mut list: Vec<&'static str> = BYPASS_COMMON.to_vec();
        list.extend_from_slice(BYPASS_WINDOWS_EXTRA);
        list
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
        assert!(list.contains(&"::1"));

        #[cfg(target_os = "windows")]
        {
            assert!(list.contains(&"<local>"));
            assert!(BYPASS_WINDOWS_EXTRA.contains(&"<local>"));
        }
    }
}
