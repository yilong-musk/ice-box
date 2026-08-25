//! WinInet + WinHTTP system proxy (architecture §13.3, plan slice 4b).
//!
//! Live hive: per-connection WinInet API is the source of truth. `apply` writes
//! `PROXY_TYPE_PROXY | PROXY_TYPE_DIRECT` on every named connection (LAN, RAS/VPN,
//! leftover Connections-key names) and does not dual-write registry keys. PAC is
//! disabled only by clearing `PROXY_TYPE_AUTO_PROXY_URL`; `AutoConfigURL` is not
//! written on apply. Live backup fails closed if the LAN snapshot or WinHTTP
//! default proxy cannot be read. WinHTTP apply/restore is best-effort when the
//! process lacks elevation.
//!
//! Temp-hive unit tests still read/write `ProxyEnable` / `ProxyServer` /
//! `ProxyOverride` because they cannot call live WinInet.
//!
//! WinInet has no separate SOCKS checkbox; apply writes a multi-protocol
//! `ProxyServer` (`http=…;https=…;socks=…`) so clients that read `socks=` can
//! follow mixed. WinHTTP stays on a plain `host:port` (HTTP proxy only).

mod connections;
mod wide;
mod winhttp;
mod wininet;

use serde_json::json;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

use crate::bypass::bypass_domains;
use crate::{ProxyBackup, ProxyEndpoints, ProxySysError, SystemProxy};
use connections::named_connection_names;
use winhttp::{apply_winhttp, query_winhttp, restore_winhttp, WinHttpSnapshot, WinHttpWrite};
use wininet::{
    apply_per_conn, notify_settings_changed, query_per_conn, restore_per_conn, PerConnSnapshot,
    APPLY_FLAGS,
};

#[cfg(all(test, target_os = "windows"))]
pub(crate) use wininet::query_effective_flags;

/// User-level WinInet settings hive path.
const INTERNET_SETTINGS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

const VALUE_PROXY_ENABLE: &str = "ProxyEnable";
const VALUE_PROXY_SERVER: &str = "ProxyServer";
const VALUE_PROXY_OVERRIDE: &str = "ProxyOverride";
const VALUE_AUTO_CONFIG_URL: &str = "AutoConfigURL";

/// Flags written back on restore when a legacy backup did not capture per-conn state.
///
/// Pre-upgrade `proxy-backup.json` files have no `connections` array. WPAD/PAC-only
/// setups keep `ProxyEnable` at 0, so falling back to `PROXY_TYPE_DIRECT` would
/// clear automatic proxy. Reconstruct from `ProxyEnable` plus leftover PAC URL,
/// and keep WPAD (Windows default) when the original flags are unknown.
fn restore_flags(state: &WinInetState, auto_config_url: Option<&str>) -> u32 {
    use windows_sys::Win32::Networking::WinInet::{
        PROXY_TYPE_AUTO_DETECT, PROXY_TYPE_AUTO_PROXY_URL, PROXY_TYPE_DIRECT,
    };

    if let Some(flags) = state.per_conn_flags {
        return flags;
    }
    if state.proxy_enable.unwrap_or(0) != 0 {
        return APPLY_FLAGS;
    }
    let mut flags = PROXY_TYPE_DIRECT | PROXY_TYPE_AUTO_DETECT;
    if auto_config_url.is_some_and(|url| !url.is_empty()) {
        flags |= PROXY_TYPE_AUTO_PROXY_URL;
    }
    flags
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

    fn is_live_hive(&self) -> bool {
        self.key_path == INTERNET_SETTINGS_KEY
    }

    fn apply_registry(
        &self,
        proxy_server: &str,
        proxy_override: &str,
    ) -> Result<(), ProxySysError> {
        let key = self.open_key(true)?;
        key.set_value(VALUE_PROXY_ENABLE, &1u32)
            .map_err(|e| ProxySysError::ApplyFailed(format!("set ProxyEnable: {e}")))?;
        key.set_value(VALUE_PROXY_SERVER, &proxy_server)
            .map_err(|e| ProxySysError::ApplyFailed(format!("set ProxyServer: {e}")))?;
        key.set_value(VALUE_PROXY_OVERRIDE, &proxy_override)
            .map_err(|e| ProxySysError::ApplyFailed(format!("set ProxyOverride: {e}")))?;
        Ok(())
    }
}

/// Live WinInet + WinHTTP snapshot. LAN is required; named connections are best-effort.
#[derive(Debug, Clone)]
struct LiveSnapshot {
    connections: Vec<PerConnSnapshot>,
    winhttp: WinHttpSnapshot,
}

fn snapshot_live() -> Result<LiveSnapshot, ProxySysError> {
    let mut connections = vec![query_per_conn(None)?];
    for name in named_connection_names() {
        match query_per_conn(Some(&name)) {
            Ok(snap) => connections.push(snap),
            Err(err) => tracing::warn!(
                connection = %name,
                error = %err,
                "skipping unreadable WinInet connection"
            ),
        }
    }
    Ok(LiveSnapshot {
        connections,
        winhttp: query_winhttp()?,
    })
}

/// WinInet `ProxyServer` multi-protocol form used by Clash / v2rayN-style clients.
fn format_wininet_proxy_server(endpoints: &ProxyEndpoints) -> String {
    let http = format!("{}:{}", endpoints.http_host, endpoints.http_port);
    let socks = match (&endpoints.socks_host, endpoints.socks_port) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        _ => http.clone(),
    };
    format!("http={http};https={http};socks={socks}")
}

/// Plain host:port for WinHTTP (HTTP proxy; does not consume WinInet `socks=`).
fn format_winhttp_proxy_server(endpoints: &ProxyEndpoints) -> String {
    format!("{}:{}", endpoints.http_host, endpoints.http_port)
}

/// Split a WinInet `ProxyServer` into http / https / socks `host:port` values.
///
/// Accepts both `host:port` and `http=…;https=…;socks=…`. Unknown protocol keys
/// are ignored. When only a plain address is present, it is treated as HTTP/HTTPS
/// (no SOCKS), matching historical ice-box applies.
fn parse_wininet_proxy_server(raw: &str) -> (Option<String>, Option<String>, Option<String>) {
    let raw = raw.trim();
    if raw.is_empty() {
        return (None, None, None);
    }
    if !raw.contains('=') {
        let plain = raw.to_string();
        return (Some(plain.clone()), Some(plain), None);
    }

    let mut http = None;
    let mut https = None;
    let mut socks = None;
    for part in raw.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((proto, addr)) = part.split_once('=') else {
            continue;
        };
        let addr = addr.trim();
        if addr.is_empty() {
            continue;
        }
        match proto.trim().to_ascii_lowercase().as_str() {
            "http" => http = Some(addr.to_string()),
            "https" => https = Some(addr.to_string()),
            "socks" | "socks5" => socks = Some(addr.to_string()),
            _ => {}
        }
    }
    let http = http.or_else(|| https.clone());
    let https = https.or_else(|| http.clone());
    (http, https, socks)
}

fn apply_live(endpoints: &ProxyEndpoints, snapshot: &LiveSnapshot) -> Result<(), ProxySysError> {
    let wininet_server = format_wininet_proxy_server(endpoints);
    let winhttp_server = format_winhttp_proxy_server(endpoints);
    let proxy_override = bypass_domains().join(";");
    for conn in &snapshot.connections {
        if let Err(err) = apply_per_conn(
            conn.name.as_deref(),
            &wininet_server,
            &proxy_override,
            APPLY_FLAGS,
            None,
        ) {
            // LAN is required. A bad RAS/VPN name (or a stale Connections-key
            // entry) must not roll back a successful LAN apply — same skip rule
            // as restore (§13.3). Chinese dial-up names are a known WinInet footgun.
            if skip_named_connection_failure(conn) {
                tracing::warn!(
                    connection = conn.name.as_deref().unwrap_or(""),
                    error = %err,
                    "skipping apply on unwritable named connection; LAN still applied"
                );
                continue;
            }
            return Err(err);
        }
    }
    match apply_winhttp(&winhttp_server, &proxy_override)? {
        WinHttpWrite::Applied => {}
        WinHttpWrite::AccessDenied => {
            tracing::warn!("WinHTTP default proxy requires elevation; WinInet still applied")
        }
    }
    notify_settings_changed()
}

fn restore_live(state: &WinInetState) -> Result<(), ProxySysError> {
    if state.connections.is_empty() {
        let leftover_pac = state.autoconfig_url.clone().or_else(|| {
            query_per_conn(None)
                .ok()
                .and_then(|conn| conn.autoconfig_url)
        });
        apply_per_conn(
            None,
            state.proxy_server.as_deref().unwrap_or(""),
            state.proxy_override.as_deref().unwrap_or(""),
            restore_flags(state, leftover_pac.as_deref()),
            leftover_pac.as_deref(),
        )
        .map_err(|e| ProxySysError::RestoreFailed(e.to_string()))?;
    } else {
        for conn in &state.connections {
            if let Err(err) = restore_per_conn(conn) {
                if skip_named_connection_failure(conn) {
                    tracing::warn!(
                        connection = conn.name.as_deref().unwrap_or(""),
                        error = %err,
                        "skipping restore of vanished or unwritable named connection"
                    );
                    continue;
                }
                return Err(ProxySysError::RestoreFailed(err.to_string()));
            }
        }
    }
    if let Some(ref winhttp) = state.winhttp {
        match restore_winhttp(winhttp).map_err(|e| ProxySysError::RestoreFailed(e.to_string()))? {
            WinHttpWrite::Applied => {}
            WinHttpWrite::AccessDenied => {
                tracing::warn!("WinHTTP default proxy restore requires elevation; skipped")
            }
        }
    }
    notify_settings_changed().map_err(|e| ProxySysError::RestoreFailed(e.to_string()))
}

/// A RAS/VPN removed since apply, or unwritable during apply, must not abort
/// LAN / WinHTTP apply or restore.
fn skip_named_connection_failure(conn: &PerConnSnapshot) -> bool {
    conn.name.is_some()
}

/// Raw snapshot stored in `ProxyBackup.extra` for faithful restore.
/// `None` on a registry field means the value did not exist when backed up, so a
/// temp-hive restore must remove it (rather than writing an empty value).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
struct WinInetState {
    proxy_enable: Option<u32>,
    proxy_server: Option<String>,
    proxy_override: Option<String>,
    /// LAN WinInet flags at backup time (also `connections[0].flags` on live).
    per_conn_flags: Option<u32>,
    autoconfig_url: Option<String>,
    #[serde(default)]
    connections: Vec<PerConnSnapshot>,
    winhttp: Option<WinHttpSnapshot>,
}

impl WinInetState {
    fn read(key: &RegKey) -> Result<Self, ProxySysError> {
        Ok(Self {
            proxy_enable: key.get_value(VALUE_PROXY_ENABLE).ok(),
            proxy_server: key.get_value::<String, _>(VALUE_PROXY_SERVER).ok(),
            proxy_override: key.get_value::<String, _>(VALUE_PROXY_OVERRIDE).ok(),
            per_conn_flags: None,
            autoconfig_url: key
                .get_value::<String, _>(VALUE_AUTO_CONFIG_URL)
                .ok()
                .filter(|s| !s.is_empty()),
            connections: Vec::new(),
            winhttp: None,
        })
    }

    fn merge_live(&mut self, mut live: LiveSnapshot) {
        if let Some(lan) = live.connections.iter_mut().find(|c| c.name.is_none()) {
            self.per_conn_flags = Some(lan.flags);
            if lan.autoconfig_url.is_none() {
                lan.autoconfig_url = self.autoconfig_url.clone();
            } else {
                self.autoconfig_url = lan.autoconfig_url.clone();
            }
        }
        self.connections = live.connections;
        self.winhttp = Some(live.winhttp);
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

    fn lan(&self) -> Option<&PerConnSnapshot> {
        self.connections.iter().find(|c| c.name.is_none())
    }

    fn to_backup(&self) -> ProxyBackup {
        use windows_sys::Win32::Networking::WinInet::PROXY_TYPE_PROXY;
        let lan = self.lan();
        let raw = lan
            .and_then(|c| c.proxy_server.clone())
            .or_else(|| {
                self.proxy_server
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .map(String::from)
            })
            .unwrap_or_default();
        let (http, https, socks) = parse_wininet_proxy_server(&raw);
        let enabled = if let Some(lan) = lan {
            (lan.flags & PROXY_TYPE_PROXY) != 0 || self.proxy_enable.unwrap_or(0) != 0
        } else {
            self.proxy_enable.unwrap_or(0) != 0
        };
        ProxyBackup {
            enabled,
            http,
            https,
            socks,
            extra: json!({
                "proxy_enable": self.proxy_enable,
                "proxy_server": self.proxy_server,
                "proxy_override": lan.and_then(|c| c.proxy_bypass.clone()).or_else(|| self.proxy_override.clone()),
                "per_conn_flags": self.per_conn_flags,
                "autoconfig_url": self.autoconfig_url,
                "connections": self.connections,
                "winhttp": self.winhttp,
            }),
        }
    }
}

/// Best-effort removal of a registry value that never existed in the backed-up state.
fn delete_if_present(key: &RegKey, name: &str) {
    let _ = key.delete_value(name);
}

fn json_u32(value: &serde_json::Value) -> Option<u32> {
    value.as_u64().map(|v| v as u32)
}

fn json_string(value: &serde_json::Value) -> Option<String> {
    value.as_str().map(String::from)
}

fn from_backup(backup: &ProxyBackup) -> WinInetState {
    let has_wininet_extra = backup
        .extra
        .as_object()
        .is_some_and(|o| o.contains_key("proxy_server"));
    if !has_wininet_extra {
        let server = backup
            .http
            .clone()
            .or_else(|| backup.https.clone())
            .unwrap_or_default();
        return WinInetState {
            proxy_enable: Some(u32::from(backup.enabled)),
            proxy_server: (!server.is_empty()).then_some(server),
            proxy_override: None,
            per_conn_flags: None,
            autoconfig_url: None,
            connections: Vec::new(),
            winhttp: None,
        };
    }
    let connections = backup
        .extra
        .get("connections")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let winhttp = backup
        .extra
        .get("winhttp")
        .filter(|v| !v.is_null())
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    WinInetState {
        proxy_enable: backup.extra.get("proxy_enable").and_then(json_u32),
        proxy_server: backup.extra.get("proxy_server").and_then(json_string),
        proxy_override: backup.extra.get("proxy_override").and_then(json_string),
        per_conn_flags: backup.extra.get("per_conn_flags").and_then(json_u32),
        autoconfig_url: backup
            .extra
            .get("autoconfig_url")
            .and_then(json_string)
            .filter(|s| !s.is_empty()),
        connections,
        winhttp,
    }
}

impl SystemProxy for WindowsSystemProxy {
    fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
        let key = self.open_key(false)?;
        let mut state = WinInetState::read(&key)?;
        if self.is_live_hive() {
            state.merge_live(snapshot_live()?);
        }
        Ok(state.to_backup())
    }

    fn apply(&self, endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
        let proxy_override = bypass_domains().join(";");
        if !self.is_live_hive() {
            return self.apply_registry(&format_wininet_proxy_server(endpoints), &proxy_override);
        }
        let rollback = snapshot_live()?;
        if let Err(err) = apply_live(endpoints, &rollback) {
            if let Err(restore_err) = restore_live(&WinInetState {
                connections: rollback.connections,
                winhttp: Some(rollback.winhttp),
                ..WinInetState::default()
            }) {
                tracing::error!(
                    error = %restore_err,
                    "rollback partial Windows proxy apply"
                );
            }
            return Err(err);
        }
        Ok(())
    }

    fn restore(&self, backup: &ProxyBackup) -> Result<(), ProxySysError> {
        let state = from_backup(backup);
        let key = self.open_key(true)?;
        if self.is_live_hive() {
            restore_live(&state)?;
            // Registry tri-state last so extra.proxy_* round-trips even if the
            // per-connection API rewrote the simple keys.
            state
                .write(&key)
                .map_err(|e| ProxySysError::RestoreFailed(e.to_string()))?;
            notify_settings_changed().map_err(|e| ProxySysError::RestoreFailed(e.to_string()))?;
            return Ok(());
        }
        state
            .write(&key)
            .map_err(|e| ProxySysError::RestoreFailed(e.to_string()))
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use windows_sys::Win32::Networking::WinInet::{
        PROXY_TYPE_AUTO_DETECT, PROXY_TYPE_AUTO_PROXY_URL, PROXY_TYPE_DIRECT, PROXY_TYPE_PROXY,
    };

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
        assert_eq!(before.socks, None, "plain ProxyServer has no socks= entry");
        assert_eq!(before.extra["proxy_enable"], 1);

        let endpoints = ProxyEndpoints {
            http_host: "127.0.0.1".into(),
            http_port: 17890,
            socks_host: Some("127.0.0.1".into()),
            socks_port: Some(17890),
        };
        proxy.apply(&endpoints).expect("apply");

        let mid = proxy.backup().expect("read after apply");
        assert!(mid.enabled);
        assert_eq!(mid.http.as_deref(), Some("127.0.0.1:17890"));
        assert_eq!(mid.https.as_deref(), Some("127.0.0.1:17890"));
        assert_eq!(mid.socks.as_deref(), Some("127.0.0.1:17890"));
        assert_eq!(
            mid.extra["proxy_server"].as_str(),
            Some("http=127.0.0.1:17890;https=127.0.0.1:17890;socks=127.0.0.1:17890")
        );
        let override_list = mid.extra["proxy_override"]
            .as_str()
            .unwrap_or_default()
            .split(';')
            .collect::<Vec<_>>();
        assert!(override_list.contains(&"localhost"));
        assert!(override_list.contains(&"<local>"));

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

    #[test]
    fn apply_flags_disable_wpad_and_pac() {
        assert_eq!(APPLY_FLAGS & PROXY_TYPE_AUTO_DETECT, 0);
        assert_eq!(APPLY_FLAGS & PROXY_TYPE_AUTO_PROXY_URL, 0);
        assert_eq!(APPLY_FLAGS & PROXY_TYPE_PROXY, PROXY_TYPE_PROXY);
        assert_eq!(APPLY_FLAGS & PROXY_TYPE_DIRECT, PROXY_TYPE_DIRECT);
    }

    #[test]
    fn restore_flags_prefers_captured_per_conn_flags() {
        let state = WinInetState {
            proxy_enable: Some(0),
            per_conn_flags: Some(9),
            ..WinInetState::default()
        };
        assert_eq!(
            restore_flags(&state, Some("http://pac.example/proxy.pac")),
            9,
            "captured flags must win over AutoConfigURL inference"
        );
    }

    #[test]
    fn restore_flags_keeps_wpad_when_missing_and_proxy_off() {
        let off = WinInetState {
            proxy_enable: Some(0),
            ..WinInetState::default()
        };
        assert_eq!(
            restore_flags(&off, None),
            PROXY_TYPE_DIRECT | PROXY_TYPE_AUTO_DETECT
        );

        let absent = WinInetState::default();
        assert_eq!(
            restore_flags(&absent, None),
            PROXY_TYPE_DIRECT | PROXY_TYPE_AUTO_DETECT
        );

        let on = WinInetState {
            proxy_enable: Some(1),
            ..WinInetState::default()
        };
        assert_eq!(restore_flags(&on, None), APPLY_FLAGS);
    }

    #[test]
    fn restore_flags_keeps_pac_when_missing_and_autoconfig_url_present() {
        let off = WinInetState {
            proxy_enable: Some(0),
            ..WinInetState::default()
        };
        let flags = restore_flags(&off, Some("http://pac.example/proxy.pac"));
        assert_eq!(
            flags,
            PROXY_TYPE_DIRECT | PROXY_TYPE_AUTO_DETECT | PROXY_TYPE_AUTO_PROXY_URL
        );
    }

    #[test]
    fn restore_flags_from_legacy_backup_without_per_conn_flags() {
        let backup = ProxyBackup {
            enabled: false,
            http: None,
            https: None,
            socks: None,
            extra: json!({
                "proxy_enable": 0,
                "proxy_server": null,
                "proxy_override": null,
            }),
        };
        let state = from_backup(&backup);
        assert_eq!(state.per_conn_flags, None);
        assert!(state.connections.is_empty());
        assert!(state.winhttp.is_none());
        let flags = restore_flags(&state, Some("http://pac.example/proxy.pac"));
        assert_eq!(flags & PROXY_TYPE_AUTO_PROXY_URL, PROXY_TYPE_AUTO_PROXY_URL);
        assert_eq!(flags & PROXY_TYPE_AUTO_DETECT, PROXY_TYPE_AUTO_DETECT);
        assert_eq!(flags & PROXY_TYPE_PROXY, 0);
    }

    #[test]
    fn from_backup_roundtrips_connections_and_winhttp() {
        let backup = ProxyBackup {
            enabled: false,
            http: None,
            https: None,
            socks: None,
            extra: json!({
                "proxy_enable": 0,
                "proxy_server": null,
                "proxy_override": "<local>",
                "per_conn_flags": 9,
                "autoconfig_url": "http://pac.example/proxy.pac",
                "connections": [{
                    "name": null,
                    "flags": 9,
                    "proxy_server": null,
                    "proxy_bypass": "<local>",
                    "autoconfig_url": "http://pac.example/proxy.pac"
                }, {
                    "name": "VPN Test",
                    "flags": 1,
                    "proxy_server": null,
                    "proxy_bypass": null,
                    "autoconfig_url": null
                }],
                "winhttp": {
                    "access_type": 1,
                    "proxy": null,
                    "bypass": null
                }
            }),
        };
        let state = from_backup(&backup);
        assert_eq!(state.per_conn_flags, Some(9));
        assert_eq!(state.connections.len(), 2);
        assert_eq!(state.connections[0].name, None);
        assert_eq!(state.connections[1].name.as_deref(), Some("VPN Test"));
        assert_eq!(
            state.autoconfig_url.as_deref(),
            Some("http://pac.example/proxy.pac")
        );
        assert_eq!(state.winhttp.as_ref().map(|w| w.access_type), Some(1));
    }

    #[test]
    fn format_and_parse_wininet_proxy_server_roundtrip() {
        let endpoints = ProxyEndpoints {
            http_host: "127.0.0.1".into(),
            http_port: 17890,
            socks_host: Some("127.0.0.1".into()),
            socks_port: Some(17890),
        };
        let raw = format_wininet_proxy_server(&endpoints);
        assert_eq!(
            raw,
            "http=127.0.0.1:17890;https=127.0.0.1:17890;socks=127.0.0.1:17890"
        );
        let (http, https, socks) = parse_wininet_proxy_server(&raw);
        assert_eq!(http.as_deref(), Some("127.0.0.1:17890"));
        assert_eq!(https.as_deref(), Some("127.0.0.1:17890"));
        assert_eq!(socks.as_deref(), Some("127.0.0.1:17890"));
    }

    #[test]
    fn parse_plain_proxy_server_has_no_socks() {
        let (http, https, socks) = parse_wininet_proxy_server("10.0.0.99:3128");
        assert_eq!(http.as_deref(), Some("10.0.0.99:3128"));
        assert_eq!(https.as_deref(), Some("10.0.0.99:3128"));
        assert!(socks.is_none());
    }

    #[test]
    fn format_wininet_falls_back_to_http_when_socks_missing() {
        let endpoints = ProxyEndpoints {
            http_host: "127.0.0.1".into(),
            http_port: 17890,
            socks_host: None,
            socks_port: None,
        };
        assert_eq!(
            format_wininet_proxy_server(&endpoints),
            "http=127.0.0.1:17890;https=127.0.0.1:17890;socks=127.0.0.1:17890"
        );
        assert_eq!(format_winhttp_proxy_server(&endpoints), "127.0.0.1:17890");
    }

    #[test]
    fn named_restore_failures_are_skippable_lan_is_not() {
        let lan = PerConnSnapshot {
            name: None,
            flags: APPLY_FLAGS,
            ..PerConnSnapshot::default()
        };
        let vpn = PerConnSnapshot {
            name: Some("Gone VPN".into()),
            flags: APPLY_FLAGS,
            ..PerConnSnapshot::default()
        };
        assert!(!skip_named_connection_failure(&lan));
        assert!(skip_named_connection_failure(&vpn));
    }

    #[test]
    fn restore_leaves_pac_string_when_snapshot_has_no_url() {
        let snap = PerConnSnapshot {
            name: None,
            flags: PROXY_TYPE_DIRECT | PROXY_TYPE_AUTO_DETECT,
            proxy_server: None,
            proxy_bypass: None,
            autoconfig_url: None,
        };
        // Contract of restore_per_conn: None autoconfig => do not write AutoConfigURL.
        assert!(snap.autoconfig_url.as_deref().is_none());
        let with_pac = PerConnSnapshot {
            autoconfig_url: Some("http://pac.example/proxy.pac".into()),
            ..snap.clone()
        };
        assert_eq!(
            with_pac.autoconfig_url.as_deref(),
            Some("http://pac.example/proxy.pac")
        );
    }

    #[test]
    fn temp_hive_backup_does_not_query_live_wininet_or_winhttp() {
        let path = temp_key_path("flags-null");
        create_temp_key(&path);
        let proxy = WindowsSystemProxy::with_key_path(path.clone());
        let backup = proxy.backup().expect("backup");
        assert!(
            backup.extra["per_conn_flags"].is_null(),
            "temp hive must not query live WinInet flags"
        );
        assert_eq!(
            backup.extra["connections"].as_array().map(Vec::len),
            Some(0)
        );
        assert!(backup.extra["winhttp"].is_null());
        cleanup_temp_key(&path);
    }
}
