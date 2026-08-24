//! WinInet user-level system proxy (architecture §13.3, plan slice 4b).
//!
//! Backs up and sets the user-level `Internet Settings` registry keys
//! (`ProxyEnable`, `ProxyServer`, `ProxyOverride`) under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings`, then notifies
//! running processes via `InternetSetOption`. Other keys in the hive (e.g.
//! `AutoConfigURL`) are never touched.
//!
//! Note: WinInet's user-level settings expose a single `ProxyServer` string covering
//! HTTP/HTTPS (and FTP); there is no separate user-level SOCKS field, so SOCKS is not
//! set on Windows. The mixed inbound still accepts SOCKS when an application connects
//! to it directly.

use serde_json::json;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

use crate::bypass::bypass_domains;
use crate::{ProxyBackup, ProxyEndpoints, ProxySysError, SystemProxy};

/// User-level WinInet settings hive path.
const INTERNET_SETTINGS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

const VALUE_PROXY_ENABLE: &str = "ProxyEnable";
const VALUE_PROXY_SERVER: &str = "ProxyServer";
const VALUE_PROXY_OVERRIDE: &str = "ProxyOverride";

/// Tell WinInet (and running processes) that the settings changed.
fn notify_settings_changed() -> Result<(), ProxySysError> {
    use windows_sys::Win32::Networking::WinInet::{
        InternetSetOptionW, INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED,
    };
    unsafe {
        if InternetSetOptionW(
            std::ptr::null_mut(),
            INTERNET_OPTION_SETTINGS_CHANGED,
            std::ptr::null_mut(),
            0,
        ) == 0
        {
            return Err(ProxySysError::ApplyFailed(format!(
                "InternetSetOption(SETTINGS_CHANGED): {}",
                std::io::Error::last_os_error()
            )));
        }
        if InternetSetOptionW(
            std::ptr::null_mut(),
            INTERNET_OPTION_REFRESH,
            std::ptr::null_mut(),
            0,
        ) == 0
        {
            return Err(ProxySysError::ApplyFailed(format!(
                "InternetSetOption(REFRESH): {}",
                std::io::Error::last_os_error()
            )));
        }
    }
    Ok(())
}

/// WinInet backend. The registry key path is injectable for unit tests
/// (temporary hives); the default is the real user-level `Internet Settings`.
pub struct WindowsSystemProxy {
    key_path: String,
}

impl Default for WindowsSystemProxy {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsSystemProxy {
    pub fn new() -> Self {
        Self {
            key_path: INTERNET_SETTINGS_KEY.to_string(),
        }
    }

    /// Test-only constructor pointing at a temporary hive.
    #[doc(hidden)]
    pub fn with_key_path(key_path: String) -> Self {
        Self { key_path }
    }

    fn open_key(&self, read_write: bool) -> Result<RegKey, ProxySysError> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let flags = if read_write {
            KEY_READ | KEY_WRITE
        } else {
            KEY_READ
        };
        hkcu.open_subkey_with_flags(&self.key_path, flags)
            .map_err(|e| ProxySysError::Other(anyhow::anyhow!("open {}: {e}", self.key_path)))
    }
}

/// Raw tri-state snapshot stored in `ProxyBackup.extra` for faithful restore.
/// `None` means the registry value did not exist when backed up, so restore must
/// remove it (rather than writing an empty value the user never had).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
struct WinInetState {
    proxy_enable: Option<u32>,
    proxy_server: Option<String>,
    proxy_override: Option<String>,
}

impl WinInetState {
    fn read(key: &RegKey) -> Result<Self, ProxySysError> {
        Ok(Self {
            proxy_enable: key.get_value(VALUE_PROXY_ENABLE).ok(),
            proxy_server: key.get_value::<String, _>(VALUE_PROXY_SERVER).ok(),
            proxy_override: key.get_value::<String, _>(VALUE_PROXY_OVERRIDE).ok(),
        })
    }

    /// Write back only the values that existed originally; delete the rest so a restore
    /// never leaves values the user never configured.
    fn write(&self, key: &RegKey) -> Result<(), ProxySysError> {
        let set_string = |name: &str, value: &str| -> Result<(), ProxySysError> {
            key.set_value(name, &value)
                .map_err(|e| ProxySysError::ApplyFailed(format!("set {name}: {e}")))
        };
        match &self.proxy_enable {
            Some(v) => key.set_value(VALUE_PROXY_ENABLE, v).map_err(|e| {
                ProxySysError::ApplyFailed(format!("set {VALUE_PROXY_ENABLE}: {e}"))
            })?,
            None => delete_if_present(key, VALUE_PROXY_ENABLE),
        }
        match &self.proxy_server {
            Some(v) => set_string(VALUE_PROXY_SERVER, v.as_str())?,
            None => delete_if_present(key, VALUE_PROXY_SERVER),
        }
        match &self.proxy_override {
            Some(v) => set_string(VALUE_PROXY_OVERRIDE, v.as_str())?,
            None => delete_if_present(key, VALUE_PROXY_OVERRIDE),
        }
        Ok(())
    }

    fn to_backup(&self) -> ProxyBackup {
        let server = self
            .proxy_server
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(String::from);
        ProxyBackup {
            enabled: self.proxy_enable.unwrap_or(0) != 0,
            http: server.clone(),
            https: server,
            socks: None,
            extra: json!({
                "proxy_enable": self.proxy_enable,
                "proxy_server": self.proxy_server,
                "proxy_override": self.proxy_override,
            }),
        }
    }
}

/// Best-effort removal of a registry value that never existed in the backed-up state.
fn delete_if_present(key: &RegKey, name: &str) {
    let _ = key.delete_value(name);
}

fn from_backup(backup: &ProxyBackup) -> WinInetState {
    let has_wininet_extra = backup
        .extra
        .as_object()
        .is_some_and(|o| o.contains_key("proxy_server"));
    if !has_wininet_extra {
        // Backup from another platform: derive a sensible WinInet state.
        let server = backup
            .http
            .clone()
            .or_else(|| backup.https.clone())
            .unwrap_or_default();
        return WinInetState {
            proxy_enable: Some(u32::from(backup.enabled)),
            proxy_server: (!server.is_empty()).then_some(server),
            proxy_override: None,
        };
    }
    WinInetState {
        proxy_enable: backup
            .extra
            .get("proxy_enable")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        proxy_server: backup
            .extra
            .get("proxy_server")
            .and_then(|v| v.as_str())
            .map(String::from),
        proxy_override: backup
            .extra
            .get("proxy_override")
            .and_then(|v| v.as_str())
            .map(String::from),
    }
}

impl SystemProxy for WindowsSystemProxy {
    fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
        let key = self.open_key(false)?;
        Ok(WinInetState::read(&key)?.to_backup())
    }

    fn apply(&self, endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
        let key = self.open_key(true)?;
        let proxy_server = format!("{}:{}", endpoints.http_host, endpoints.http_port);
        let proxy_override = bypass_domains().join(";");
        key.set_value(VALUE_PROXY_ENABLE, &1u32)
            .map_err(|e| ProxySysError::ApplyFailed(format!("set ProxyEnable: {e}")))?;
        key.set_value(VALUE_PROXY_SERVER, &proxy_server.as_str())
            .map_err(|e| ProxySysError::ApplyFailed(format!("set ProxyServer: {e}")))?;
        key.set_value(VALUE_PROXY_OVERRIDE, &proxy_override.as_str())
            .map_err(|e| ProxySysError::ApplyFailed(format!("set ProxyOverride: {e}")))?;
        notify_settings_changed()?;
        Ok(())
    }

    fn restore(&self, backup: &ProxyBackup) -> Result<(), ProxySysError> {
        let state = from_backup(backup);
        let key = self.open_key(true)?;
        state
            .write(&key)
            .map_err(|e| ProxySysError::RestoreFailed(e.to_string()))?;
        notify_settings_changed().map_err(|e| ProxySysError::RestoreFailed(e.to_string()))?;
        Ok(())
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_key_path(label: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        format!(r"Software\ice-box-test-{label}-{nanos}")
    }

    fn create_temp_key(path: &str) -> RegKey {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _disp) = hkcu
            .create_subkey(path)
            .expect("create temporary test hive");
        key
    }

    fn cleanup_temp_key(path: &str) {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let parent = path.rsplit_once('\\').map(|(p, _)| p).unwrap_or(path);
        let name = path.rsplit('\\').next().unwrap_or(path);
        if let Ok(key) = hkcu.open_subkey_with_flags(parent, KEY_WRITE) {
            let _ = key.delete_subkey_all(name);
        }
    }

    #[test]
    fn wininet_roundtrip_backup_apply_restore_in_temp_hive() {
        let path = temp_key_path("roundtrip");
        create_temp_key(&path);
        let proxy = WindowsSystemProxy::with_key_path(path.clone());

        // Seed a distinctive "user" proxy state first.
        {
            let key = proxy.open_key(true).unwrap();
            key.set_value(VALUE_PROXY_ENABLE, &1u32).unwrap();
            key.set_value(VALUE_PROXY_SERVER, &"10.0.0.99:3128")
                .unwrap();
            key.set_value(VALUE_PROXY_OVERRIDE, &"localhost").unwrap();
        }
        let before = proxy.backup().expect("backup");
        assert!(before.enabled);
        assert_eq!(before.http.as_deref(), Some("10.0.0.99:3128"));
        assert_eq!(before.socks, None, "WinInet user-level has no SOCKS field");
        assert_eq!(before.extra["proxy_enable"], 1);

        // Apply ice-box settings.
        let endpoints = ProxyEndpoints {
            http_host: "127.0.0.1".into(),
            http_port: 17890,
            socks_host: None,
            socks_port: None,
        };
        proxy.apply(&endpoints).expect("apply");

        let mid = proxy.backup().expect("read after apply");
        assert!(mid.enabled);
        assert_eq!(mid.http.as_deref(), Some("127.0.0.1:17890"));
        let override_list = mid.extra["proxy_override"]
            .as_str()
            .unwrap_or_default()
            .split(';')
            .collect::<Vec<_>>();
        assert!(override_list.contains(&"localhost"));
        assert!(override_list.contains(&"<local>"));

        // Restore must return the user's previous state verbatim.
        proxy.restore(&before).expect("restore");
        let after = proxy.backup().expect("read after restore");
        assert_eq!(after.extra, before.extra, "raw tri-state restored exactly");
        assert_eq!(after.enabled, before.enabled);
        assert_eq!(after.http, before.http);

        cleanup_temp_key(&path);
    }

    #[test]
    fn wininet_restore_disables_proxy_when_originally_off() {
        let path = temp_key_path("restore-off");
        create_temp_key(&path);
        let proxy = WindowsSystemProxy::with_key_path(path.clone());

        // Original state: proxy disabled.
        {
            let key = proxy.open_key(true).unwrap();
            key.set_value(VALUE_PROXY_ENABLE, &0u32).unwrap();
            key.set_value(VALUE_PROXY_SERVER, &"").unwrap();
            key.set_value(VALUE_PROXY_OVERRIDE, &"").unwrap();
        }
        let before = proxy.backup().expect("backup");
        assert!(!before.enabled);

        proxy
            .apply(&ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            })
            .expect("apply");
        assert!(proxy.backup().unwrap().enabled);

        proxy.restore(&before).expect("restore");
        let after = proxy.backup().expect("after");
        assert!(!after.enabled, "ProxyEnable must be restored to 0");
        assert_eq!(after.extra, before.extra);

        cleanup_temp_key(&path);
    }

    #[test]
    fn wininet_restore_removes_values_that_never_existed() {
        let path = temp_key_path("restore-absent");
        create_temp_key(&path);
        let proxy = WindowsSystemProxy::with_key_path(path.clone());

        // Original state: proxy disabled, no ProxyServer/ProxyOverride values at all.
        let before = proxy.backup().expect("backup");
        assert_eq!(before.extra["proxy_enable"], serde_json::Value::Null);
        assert_eq!(before.extra["proxy_server"], serde_json::Value::Null);
        assert_eq!(before.extra["proxy_override"], serde_json::Value::Null);

        proxy
            .apply(&ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            })
            .expect("apply");
        assert!(proxy.backup().unwrap().enabled);

        proxy.restore(&before).expect("restore");
        let key = proxy.open_key(false).unwrap();
        assert!(
            key.get_value::<u32, _>(VALUE_PROXY_ENABLE).is_err(),
            "ProxyEnable must be removed, it never existed before"
        );
        assert!(
            key.get_value::<String, _>(VALUE_PROXY_SERVER).is_err(),
            "ProxyServer must be removed, it never existed before"
        );
        assert!(
            key.get_value::<String, _>(VALUE_PROXY_OVERRIDE).is_err(),
            "ProxyOverride must be removed, it never existed before"
        );
        drop(key);
        cleanup_temp_key(&path);
    }

    #[test]
    fn wininet_apply_leaves_unrelated_values_untouched() {
        let path = temp_key_path("untouched");
        let key = create_temp_key(&path);
        key.set_value("AutoConfigURL", &"http://pac.example/proxy.pac")
            .unwrap();
        drop(key);
        let proxy = WindowsSystemProxy::with_key_path(path.clone());

        proxy
            .apply(&ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            })
            .expect("apply");

        let key = proxy.open_key(false).unwrap();
        assert_eq!(
            key.get_value::<String, _>("AutoConfigURL").unwrap(),
            "http://pac.example/proxy.pac",
            "unrelated Internet Settings values must be kept"
        );
        drop(key);
        cleanup_temp_key(&path);
    }
}
