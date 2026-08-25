//! System proxy backup / apply / restore (macOS & Windows).
//! TUN is intentionally out of scope for v1.

mod backup_file;
mod bypass;
mod record;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::{
    parse_bypass_output, parse_proxy_get_output, MacosSystemProxy, NetworkSetupRunner,
    RealNetworkSetup, ServiceBackup, ServiceProxyState,
};

#[cfg(target_os = "windows")]
pub use windows::WindowsSystemProxy;

pub use backup_file::{
    is_proxy_applied_on_disk, is_proxy_live_applied, recover_if_applied, ProxyBackupFile,
};
pub use bypass::{bypass_domains, BYPASS_COMMON, BYPASS_WINDOWS_EXTRA};
pub use record::{apply_and_record, restore_and_clear_flag};

use ice_config::{AppError, ErrorCode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyEndpoints {
    pub http_host: String,
    pub http_port: u16,
    #[serde(default)]
    pub socks_host: Option<String>,
    #[serde(default)]
    pub socks_port: Option<u16>,
}

/// Snapshot of the user's previous system proxy settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProxyBackup {
    pub enabled: bool,
    pub http: Option<String>,
    pub https: Option<String>,
    pub socks: Option<String>,
    /// Platform-specific opaque fields for faithful restore.
    #[serde(default = "empty_object")]
    pub extra: serde_json::Value,
}

fn empty_object() -> serde_json::Value {
    serde_json::json!({})
}

impl Default for ProxyBackup {
    fn default() -> Self {
        Self {
            enabled: false,
            http: None,
            https: None,
            socks: None,
            extra: empty_object(),
        }
    }
}

pub trait SystemProxy: Send {
    fn backup(&self) -> Result<ProxyBackup, ProxySysError>;
    fn apply(&self, endpoints: &ProxyEndpoints) -> Result<(), ProxySysError>;
    fn restore(&self, backup: &ProxyBackup) -> Result<(), ProxySysError>;
}

/// Placeholder used on platforms without a system-proxy backend.
///
/// `apply` stays unimplemented so Start can warn instead of pretending the OS
/// proxy changed. `restore` succeeds so a failed apply can clear `pending_apply`
/// instead of poisoning crash recovery and Stop.
#[derive(Debug, Default)]
pub struct NoopSystemProxy;

impl SystemProxy for NoopSystemProxy {
    fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
        Ok(ProxyBackup::default())
    }

    fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
        Err(ProxySysError::NotImplemented("apply"))
    }

    fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
        Ok(())
    }
}

/// Create the platform-appropriate backend.
pub fn create_system_proxy() -> Box<dyn SystemProxy> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacosSystemProxy::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsSystemProxy::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Box::new(NoopSystemProxy)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProxySysError {
    #[error("not implemented: {0}")]
    NotImplemented(&'static str),
    #[error("proxy apply failed: {0}")]
    ApplyFailed(String),
    #[error("proxy restore failed: {0}")]
    RestoreFailed(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<ProxySysError> for AppError {
    fn from(err: ProxySysError) -> Self {
        match &err {
            ProxySysError::RestoreFailed(_) => {
                AppError::new(ErrorCode::ProxyRestoreFailed, err.to_string())
            }
            ProxySysError::NotImplemented("restore") => {
                AppError::new(ErrorCode::ProxyRestoreFailed, err.to_string())
            }
            ProxySysError::ApplyFailed(_)
            | ProxySysError::NotImplemented(_)
            | ProxySysError::Io(_)
            | ProxySysError::Json(_)
            | ProxySysError::Other(_) => {
                AppError::new(ErrorCode::ProxyApplyFailed, err.to_string())
            }
        }
    }
}

#[cfg(test)]
mod factory_tests {
    use super::*;

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn unsupported_platform_uses_noop_that_does_not_poison_restore() {
        let proxy = create_system_proxy();
        proxy
            .restore(&ProxyBackup::default())
            .expect("restore is a no-op so pending_apply can clear");
        let err = proxy
            .apply(&ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            })
            .expect_err("apply stays unimplemented");
        assert!(matches!(err, ProxySysError::NotImplemented(_)));
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn supported_platform_backend_can_backup_without_mutating() {
        create_system_proxy()
            .backup()
            .expect("read-only backup must work without changing the OS proxy");
    }
}

#[cfg(all(test, target_os = "windows"))]
mod live_tests {
    use super::*;
    use crate::windows::WindowsSystemProxy;

    /// G4.3-windows — real machine: backup → apply → read back → restore.
    /// Run: `cargo test -p ice-proxy-sys g4_3 -- --ignored --nocapture` (Windows)
    #[test]
    #[ignore = "proxy_sys: mutates real WinInet Internet Settings"]
    fn g4_3_backup_apply_restore_roundtrip() {
        let proxy = WindowsSystemProxy::new();
        let before = proxy.backup().expect("backup");

        let endpoints = ProxyEndpoints {
            http_host: "127.0.0.1".into(),
            http_port: 17890,
            socks_host: None,
            socks_port: None,
        };
        proxy.apply(&endpoints).expect("apply");

        let mid = proxy.backup().expect("read after apply");
        assert!(
            mid.http
                .as_deref()
                .is_some_and(|h| h.contains("127.0.0.1") && h.contains("17890")),
            "http after apply: {:?}",
            mid.http
        );
        assert!(
            mid.extra["proxy_override"]
                .as_str()
                .unwrap_or_default()
                .contains("<local>"),
            "Windows bypass must include <local>"
        );
        let flags = crate::windows::query_effective_flags(None).expect("effective LAN flags");
        assert_eq!(
            flags & 8,
            0,
            "apply must clear PROXY_TYPE_AUTO_DETECT on FLAGS (effective): {flags}"
        );
        assert_eq!(
            flags & 4,
            0,
            "apply must clear PROXY_TYPE_AUTO_PROXY_URL on FLAGS (effective): {flags}"
        );
        assert_ne!(
            flags & 2,
            0,
            "apply must set PROXY_TYPE_PROXY on FLAGS: {flags}"
        );
        // Backup stores FLAGS_UI for restore fidelity; after apply both should match.
        assert_eq!(
            mid.extra["per_conn_flags"].as_u64().unwrap_or(0),
            u64::from(flags),
            "FLAGS_UI readback after apply must match effective FLAGS"
        );
        assert!(
            before.extra["connections"]
                .as_array()
                .is_some_and(|c| !c.is_empty()),
            "live backup must snapshot at least the LAN connection"
        );
        assert!(
            before.extra["winhttp"].is_object(),
            "live backup must snapshot WinHTTP default proxy"
        );

        proxy.restore(&before).expect("restore");
        let after = proxy.backup().expect("read after restore");
        assert_eq!(after.extra, before.extra, "raw tri-state restored exactly");
        assert_eq!(after.enabled, before.enabled);
        assert_eq!(after.http, before.http);
        println!(
            "G4.3-windows ok: restored enable={} server={:?}",
            after.enabled, after.http
        );
    }

    /// G4.4-windows — user already had a proxy; restore must bring it back (not merely disable).
    #[test]
    #[ignore = "proxy_sys: mutates real WinInet Internet Settings"]
    fn g4_4_restore_preserves_prior_user_proxy() {
        let proxy = WindowsSystemProxy::new();
        let original = proxy.backup().expect("original");

        // Install a distinctive "user" proxy first.
        let user_ep = ProxyEndpoints {
            http_host: "10.0.0.99".into(),
            http_port: 3128,
            socks_host: None,
            socks_port: None,
        };
        proxy.apply(&user_ep).expect("set user proxy");
        let user_backup = proxy.backup().expect("user backup");
        assert!(
            user_backup
                .http
                .as_deref()
                .is_some_and(|h| h.contains("10.0.0.99") && h.contains("3128")),
            "user http: {:?}",
            user_backup.http
        );

        // ice-box apply
        let ice_ep = ProxyEndpoints {
            http_host: "127.0.0.1".into(),
            http_port: 17890,
            socks_host: None,
            socks_port: None,
        };
        proxy.apply(&ice_ep).expect("ice apply");

        // Restore to user settings (as Stop would)
        proxy.restore(&user_backup).expect("restore user");
        let after = proxy.backup().expect("after");
        assert!(
            after
                .http
                .as_deref()
                .is_some_and(|h| h.contains("10.0.0.99") && h.contains("3128")),
            "must restore user host:port, got {:?}",
            after.http
        );

        // Put machine back to truly original state.
        proxy.restore(&original).expect("restore original");
        println!("G4.4-windows ok: user proxy restored then original restored");
    }
}

#[cfg(all(test, target_os = "macos"))]
mod live_tests {
    use super::*;
    use crate::macos::MacosSystemProxy;

    /// G4.3 — real machine: backup → apply → read back → restore.
    /// Run: `cargo test -p ice-proxy-sys g4_3 -- --ignored --nocapture`
    #[test]
    #[ignore = "proxy_sys: mutates real macOS network settings"]
    fn g4_3_backup_apply_restore_roundtrip() {
        let proxy = MacosSystemProxy::new();
        let before = proxy.backup().expect("backup");

        let endpoints = ProxyEndpoints {
            http_host: "127.0.0.1".into(),
            http_port: 17890,
            socks_host: Some("127.0.0.1".into()),
            socks_port: Some(17890),
        };
        proxy.apply(&endpoints).expect("apply");

        let mid = proxy.backup().expect("read after apply");
        assert!(
            mid.http
                .as_deref()
                .is_some_and(|h| h.contains("127.0.0.1") && h.contains("17890")),
            "http after apply: {:?}",
            mid.http
        );
        assert!(
            mid.https
                .as_deref()
                .is_some_and(|h| h.contains("127.0.0.1") && h.contains("17890")),
            "https after apply: {:?}",
            mid.https
        );
        assert!(
            mid.socks
                .as_deref()
                .is_some_and(|h| h.contains("127.0.0.1") && h.contains("17890")),
            "socks after apply: {:?}",
            mid.socks
        );

        proxy.restore(&before).expect("restore");
        let after = proxy.backup().expect("read after restore");
        assert_eq!(after.enabled, before.enabled);
        assert_eq!(after.http, before.http);
        assert_eq!(after.https, before.https);
        assert_eq!(after.socks, before.socks);
        println!(
            "G4.3 ok: restored http={:?} https={:?}",
            after.http, after.https
        );
    }

    /// G4.4 — user already had a proxy; restore must bring it back (not merely disable).
    #[test]
    #[ignore = "proxy_sys: mutates real macOS network settings"]
    fn g4_4_restore_preserves_prior_user_proxy() {
        let proxy = MacosSystemProxy::new();
        let original = proxy.backup().expect("original");

        // Install a distinctive "user" proxy first.
        let user_ep = ProxyEndpoints {
            http_host: "10.0.0.99".into(),
            http_port: 3128,
            socks_host: None,
            socks_port: None,
        };
        proxy.apply(&user_ep).expect("set user proxy");
        let user_backup = proxy.backup().expect("user backup");
        assert!(
            user_backup
                .http
                .as_deref()
                .is_some_and(|h| h.contains("10.0.0.99") && h.contains("3128")),
            "user http: {:?}",
            user_backup.http
        );

        // ice-box apply
        let ice_ep = ProxyEndpoints {
            http_host: "127.0.0.1".into(),
            http_port: 17890,
            socks_host: Some("127.0.0.1".into()),
            socks_port: Some(17890),
        };
        proxy.apply(&ice_ep).expect("ice apply");

        // Restore to user settings (as Stop would)
        proxy.restore(&user_backup).expect("restore user");
        let after = proxy.backup().expect("after");
        assert!(
            after
                .http
                .as_deref()
                .is_some_and(|h| h.contains("10.0.0.99") && h.contains("3128")),
            "must restore user host:port, got {:?}",
            after.http
        );

        // Put machine back to truly original state.
        proxy.restore(&original).expect("restore original");
        println!("G4.4 ok: user proxy restored then original restored");
    }
}
