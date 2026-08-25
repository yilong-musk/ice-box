//! On-disk `proxy-backup.json` and crash-recovery restore.

use std::path::Path;

use chrono::{DateTime, Utc};
use ice_config::write_json_atomic;
use serde::{Deserialize, Serialize};

use crate::{ProxyBackup, ProxyEndpoints, ProxySysError, SystemProxy};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProxyBackupFile {
    pub applied: bool,
    /// True while `apply()` is in flight or was interrupted before `applied` was persisted.
    /// Crash recovery restores from `backup` when this is set.
    #[serde(default)]
    pub pending_apply: bool,
    pub applied_at: Option<DateTime<Utc>>,
    pub endpoints: ProxyEndpoints,
    pub backup: ProxyBackup,
}

impl ProxyBackupFile {
    pub fn load(path: &Path) -> Result<Self, ProxySysError> {
        let raw = std::fs::read_to_string(path).map_err(ProxySysError::Io)?;
        let file: Self = serde_json::from_str(&raw)?;
        Ok(file)
    }

    pub fn save(&self, path: &Path) -> Result<(), ProxySysError> {
        write_json_atomic(path, self).map_err(|e| ProxySysError::Other(e.into()))?;
        Ok(())
    }
}

/// Whether on-disk backup indicates system proxy is fully applied (not pending).
pub fn is_proxy_applied_on_disk(backup_path: &Path) -> bool {
    if !backup_path.exists() {
        return false;
    }
    ProxyBackupFile::load(backup_path)
        .map(|r| r.applied && !r.pending_apply)
        .unwrap_or(false)
}

/// Whether ice-box has applied system proxy both on disk and in the OS snapshot.
pub fn is_proxy_live_applied(
    proxy: &dyn SystemProxy,
    backup_path: &Path,
    endpoints: &ProxyEndpoints,
) -> bool {
    if !is_proxy_applied_on_disk(backup_path) {
        return false;
    }
    let Ok(record) = ProxyBackupFile::load(backup_path) else {
        return false;
    };
    match proxy.backup() {
        Ok(current) => proxy_backup_matches(&record, &current, endpoints),
        Err(_) => false,
    }
}

fn proxy_backup_matches(
    record: &ProxyBackupFile,
    current: &ProxyBackup,
    endpoints: &ProxyEndpoints,
) -> bool {
    if !current.enabled {
        return false;
    }
    // macOS-style per-service state: every service that existed at apply time must still
    // route through our endpoints; services enabled later (or their foreign proxies) are
    // not ice-box's concern and must not shadow a correct apply.
    let expected: std::collections::HashSet<String> = record
        .backup
        .extra
        .get("services")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(service_name).collect())
        .unwrap_or_default();
    let current_services: Vec<&serde_json::Value> = current
        .extra
        .get("services")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().collect())
        .unwrap_or_default();
    if !expected.is_empty() && !current_services.is_empty() {
        for svc in current_services {
            let Some(name) = service_name(svc) else {
                continue;
            };
            if !expected.contains(&name) {
                continue;
            }
            if !service_matches_endpoints(svc, endpoints) {
                return false;
            }
        }
        return true;
    }
    proxy_backup_matches_endpoints(current, endpoints)
}

fn service_name(service: &serde_json::Value) -> Option<String> {
    service
        .get("name")
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// One applied service routes through our endpoints when all proxy kinds we set are
/// enabled at our host:port.
fn service_matches_endpoints(service: &serde_json::Value, expected: &ProxyEndpoints) -> bool {
    let matches = |state: &serde_json::Value, host: &str, port: u16| {
        if !state
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return false;
        }
        let server = state.get("server").and_then(|v| v.as_str()).unwrap_or("");
        let state_port = state.get("port").and_then(|v| v.as_u64()).unwrap_or(0);
        normalize_proxy_host(server) == normalize_proxy_host(host) && state_port == u64::from(port)
    };
    if !service
        .get("web")
        .is_some_and(|s| matches(s, &expected.http_host, expected.http_port))
        || !service
            .get("secure_web")
            .is_some_and(|s| matches(s, &expected.http_host, expected.http_port))
    {
        return false;
    }
    match (&expected.socks_host, expected.socks_port) {
        (Some(host), Some(port)) => service.get("socks").is_some_and(|s| matches(s, host, port)),
        _ => true,
    }
}

fn normalize_proxy_host(host: &str) -> String {
    let host = host.trim().trim_matches(|c| c == '[' || c == ']');
    if host.eq_ignore_ascii_case("localhost") {
        return "127.0.0.1".into();
    }
    host.to_ascii_lowercase()
}

fn parse_host_port(raw: &str) -> Option<(String, u16)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with('[') {
        let end = raw.find(']')?;
        let host = raw[1..end].to_string();
        let rest = raw.get(end + 1..)?.strip_prefix(':')?;
        let port = rest.parse().ok()?;
        return Some((host, port));
    }
    let (host, port_str) = raw.rsplit_once(':')?;
    let port = port_str.parse().ok()?;
    Some((host.to_string(), port))
}

fn proxy_endpoint_matches(actual: Option<&str>, expected_host: &str, expected_port: u16) -> bool {
    let Some(actual) = actual else {
        return false;
    };
    let Some((host, port)) = parse_host_port(actual) else {
        return false;
    };
    normalize_proxy_host(&host) == normalize_proxy_host(expected_host) && port == expected_port
}

fn proxy_backup_matches_endpoints(backup: &ProxyBackup, expected: &ProxyEndpoints) -> bool {
    if !backup.enabled {
        return false;
    }
    if !proxy_endpoint_matches(
        backup.http.as_deref(),
        &expected.http_host,
        expected.http_port,
    ) {
        return false;
    }
    if let Some(https) = backup.https.as_deref() {
        if !proxy_endpoint_matches(Some(https), &expected.http_host, expected.http_port) {
            return false;
        }
    }
    match (&expected.socks_host, expected.socks_port) {
        (Some(host), Some(port)) => proxy_endpoint_matches(backup.socks.as_deref(), host, port),
        _ => true,
    }
}

/// If `proxy-backup.json` exists with `applied == true`, call `restore` once,
/// then set `applied = false` and keep the file. Never calls `apply`.
///
/// Returns `true` when a restore was performed.
pub fn recover_if_applied(
    backup_path: &Path,
    proxy: &dyn SystemProxy,
) -> Result<bool, ProxySysError> {
    if !backup_path.exists() {
        return Ok(false);
    }

    let mut record = ProxyBackupFile::load(backup_path)?;
    if !record.applied && !record.pending_apply {
        return Ok(false);
    }

    proxy.restore(&record.backup)?;
    record.applied = false;
    record.pending_apply = false;
    record.save(backup_path)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProxyBackup, ProxyEndpoints, ProxySysError, SystemProxy};
    use std::cell::Cell;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct MockProxy {
        restore_calls: Cell<usize>,
        apply_calls: Cell<usize>,
    }

    impl SystemProxy for MockProxy {
        fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
            Ok(ProxyBackup::default())
        }

        fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
            self.apply_calls.set(self.apply_calls.get() + 1);
            Ok(())
        }

        fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
            self.restore_calls.set(self.restore_calls.get() + 1);
            Ok(())
        }
    }

    fn temp_backup_path(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-proxy-backup-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("mkdir");
        dir.join("proxy-backup.json")
    }

    fn sample_record(applied: bool) -> ProxyBackupFile {
        ProxyBackupFile {
            applied,
            pending_apply: false,
            applied_at: Some(Utc::now()),
            endpoints: ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: Some("127.0.0.1".into()),
                socks_port: Some(17890),
            },
            backup: ProxyBackup {
                enabled: false,
                http: None,
                https: None,
                socks: None,
                extra: serde_json::json!({ "service": "Wi-Fi" }),
            },
        }
    }

    #[test]
    fn proxy_backup_file_serde_roundtrip() {
        let record = sample_record(true);
        let json = serde_json::to_value(&record).expect("ser");
        assert_eq!(json["applied"], true);
        assert!(json["applied_at"].is_string());
        assert_eq!(json["endpoints"]["http_port"], 17890);
        assert_eq!(json["backup"]["extra"]["service"], "Wi-Fi");

        let back: ProxyBackupFile = serde_json::from_value(json).expect("de");
        assert!(back.applied);
        assert_eq!(back.endpoints.http_port, 17890);
        assert_eq!(back.backup.extra["service"], "Wi-Fi");
    }

    #[test]
    fn is_proxy_applied_on_disk_reflects_flags() {
        let path = temp_backup_path("applied-flag");
        sample_record(true).save(&path).expect("seed");
        assert!(is_proxy_applied_on_disk(&path));

        let pending = ProxyBackupFile {
            applied: false,
            pending_apply: true,
            applied_at: None,
            endpoints: ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            },
            backup: ProxyBackup::default(),
        };
        pending.save(&path).expect("pending");
        assert!(!is_proxy_applied_on_disk(&path));

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn is_proxy_live_applied_requires_os_match() {
        #[derive(Default)]
        struct LiveProxy {
            backup: ProxyBackup,
        }

        impl SystemProxy for LiveProxy {
            fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
                Ok(self.backup.clone())
            }

            fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
                Ok(())
            }

            fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
                Ok(())
            }
        }

        let path = temp_backup_path("live-applied");
        sample_record(true).save(&path).expect("seed");
        let endpoints = ProxyEndpoints {
            http_host: "127.0.0.1".into(),
            http_port: 17890,
            socks_host: Some("127.0.0.1".into()),
            socks_port: Some(17890),
        };
        let proxy = LiveProxy {
            backup: ProxyBackup {
                enabled: true,
                http: Some("127.0.0.1:17890".into()),
                https: Some("127.0.0.1:17890".into()),
                socks: Some("127.0.0.1:17890".into()),
                extra: serde_json::json!({}),
            },
        };
        assert!(is_proxy_live_applied(&proxy, &path, &endpoints));

        let mismatched = LiveProxy {
            backup: ProxyBackup {
                enabled: true,
                http: Some("10.0.0.1:3128".into()),
                https: None,
                socks: None,
                extra: serde_json::json!({}),
            },
        };
        assert!(!is_proxy_live_applied(&mismatched, &path, &endpoints));

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn is_proxy_live_applied_ignores_services_enabled_after_apply() {
        #[derive(Default)]
        struct LiveProxy {
            backup: ProxyBackup,
        }

        impl SystemProxy for LiveProxy {
            fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
                Ok(self.backup.clone())
            }
            fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
                Ok(())
            }
            fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
                Ok(())
            }
        }

        let path = temp_backup_path("live-services");
        let mut record = sample_record(true);
        record.backup.extra = serde_json::json!({
            "services": [{
                "name": "Wi-Fi",
                "web": {"enabled": false, "server": "", "port": 0},
                "secure_web": {"enabled": false, "server": "", "port": 0},
                "socks": {"enabled": false, "server": "", "port": 0},
                "bypass": []
            }]
        });
        record.save(&path).expect("seed");

        let endpoints = ProxyEndpoints {
            http_host: "127.0.0.1".into(),
            http_port: 17890,
            socks_host: Some("127.0.0.1".into()),
            socks_port: Some(17890),
        };
        let state = |server: &str| {
            serde_json::json!({
                "enabled": true,
                "server": server,
                "port": 17890
            })
        };
        let proxy = LiveProxy {
            backup: ProxyBackup {
                enabled: true,
                http: Some("10.0.0.1:3128".into()),
                https: None,
                socks: None,
                extra: serde_json::json!({
                    "services": [
                        {
                            "name": "Wi-Fi",
                            "web": state("127.0.0.1"),
                            "secure_web": state("127.0.0.1"),
                            "socks": state("127.0.0.1"),
                            "bypass": []
                        },
                        {
                            "name": "Ethernet-New",
                            "web": state("10.0.0.1"),
                            "secure_web": state("10.0.0.1"),
                            "socks": state("10.0.0.1"),
                            "bypass": []
                        }
                    ]
                }),
            },
        };
        assert!(
            is_proxy_live_applied(&proxy, &path, &endpoints),
            "foreign proxy on a service enabled after apply must not shadow the apply"
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn is_proxy_live_applied_reports_pending_when_applied_service_tampered() {
        #[derive(Default)]
        struct LiveProxy {
            backup: ProxyBackup,
        }

        impl SystemProxy for LiveProxy {
            fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
                Ok(self.backup.clone())
            }
            fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
                Ok(())
            }
            fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
                Ok(())
            }
        }

        let path = temp_backup_path("live-tampered");
        let mut record = sample_record(true);
        record.backup.extra = serde_json::json!({
            "services": [{
                "name": "Wi-Fi",
                "web": {"enabled": false, "server": "", "port": 0},
                "secure_web": {"enabled": false, "server": "", "port": 0},
                "socks": {"enabled": false, "server": "", "port": 0},
                "bypass": []
            }]
        });
        record.save(&path).expect("seed");

        let endpoints = ProxyEndpoints {
            http_host: "127.0.0.1".into(),
            http_port: 17890,
            socks_host: Some("127.0.0.1".into()),
            socks_port: Some(17890),
        };
        let foreign = |server: &str| {
            serde_json::json!({
                "enabled": true,
                "server": server,
                "port": 17890
            })
        };
        let proxy = LiveProxy {
            backup: ProxyBackup {
                enabled: true,
                http: Some("10.0.0.1:3128".into()),
                https: None,
                socks: None,
                extra: serde_json::json!({
                    "services": [{
                        "name": "Wi-Fi",
                        "web": foreign("10.0.0.1"),
                        "secure_web": foreign("10.0.0.1"),
                        "socks": foreign("10.0.0.1"),
                        "bypass": []
                    }]
                }),
            },
        };
        assert!(
            !is_proxy_live_applied(&proxy, &path, &endpoints),
            "a service we applied to that no longer routes through us means pending"
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn proxy_backup_rejects_host_prefix_and_port_suffix_false_positives() {
        let endpoints = ProxyEndpoints {
            http_host: "127.0.0.1".into(),
            http_port: 17890,
            socks_host: Some("127.0.0.1".into()),
            socks_port: Some(17890),
        };
        let host_prefix = ProxyBackup {
            enabled: true,
            http: Some("127.0.0.10:7890".into()),
            https: None,
            socks: None,
            extra: serde_json::json!({}),
        };
        assert!(!proxy_backup_matches_endpoints(&host_prefix, &endpoints));

        let port_suffix = ProxyBackup {
            enabled: true,
            http: Some("127.0.0.1:178901".into()),
            https: None,
            socks: None,
            extra: serde_json::json!({}),
        };
        assert!(!proxy_backup_matches_endpoints(&port_suffix, &endpoints));
    }

    /// Multi-protocol WinInet backups expose socks= as `backup.socks`.
    #[test]
    fn windows_style_multi_protocol_backup_matches_with_socks() {
        let endpoints = ProxyEndpoints {
            http_host: "127.0.0.1".into(),
            http_port: 17890,
            socks_host: Some("127.0.0.1".into()),
            socks_port: Some(17890),
        };
        let backup = ProxyBackup {
            enabled: true,
            http: Some("127.0.0.1:17890".into()),
            https: Some("127.0.0.1:17890".into()),
            socks: Some("127.0.0.1:17890".into()),
            extra: serde_json::json!({}),
        };
        assert!(proxy_backup_matches_endpoints(&backup, &endpoints));
    }

    /// Legacy plain `host:port` applies had no socks=; live-applied must not
    /// require SOCKS when endpoints also omit it.
    #[test]
    fn windows_style_http_only_backup_matches_without_socks() {
        let endpoints = ProxyEndpoints {
            http_host: "127.0.0.1".into(),
            http_port: 17890,
            socks_host: None,
            socks_port: None,
        };
        let backup = ProxyBackup {
            enabled: true,
            http: Some("127.0.0.1:17890".into()),
            https: Some("127.0.0.1:17890".into()),
            socks: None,
            extra: serde_json::json!({}),
        };
        assert!(proxy_backup_matches_endpoints(&backup, &endpoints));

        let still_expects_socks = ProxyEndpoints {
            socks_host: Some("127.0.0.1".into()),
            socks_port: Some(17890),
            ..endpoints.clone()
        };
        assert!(
            !proxy_backup_matches_endpoints(&backup, &still_expects_socks),
            "endpoints that expect SOCKS must not match an HTTP-only snapshot"
        );
    }

    #[test]
    fn applied_true_restores_once_and_clears_flag_keeps_file() {
        let path = temp_backup_path("applied");
        sample_record(true).save(&path).expect("seed");

        let mock = MockProxy::default();
        let did = recover_if_applied(&path, &mock).expect("recover");
        assert!(did);
        assert_eq!(mock.restore_calls.get(), 1);
        assert_eq!(mock.apply_calls.get(), 0);

        assert!(path.exists(), "file must remain for audit");
        let after = ProxyBackupFile::load(&path).expect("reload");
        assert!(!after.applied);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn pending_apply_recovery_restores_once() {
        let path = temp_backup_path("pending-recover");
        let record = ProxyBackupFile {
            applied: false,
            pending_apply: true,
            applied_at: None,
            endpoints: ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            },
            backup: ProxyBackup::default(),
        };
        record.save(&path).expect("seed");

        let mock = MockProxy::default();
        let did = recover_if_applied(&path, &mock).expect("recover");
        assert!(did);
        assert_eq!(mock.restore_calls.get(), 1);

        let after = ProxyBackupFile::load(&path).expect("reload");
        assert!(!after.applied);
        assert!(!after.pending_apply);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn applied_false_does_not_restore_or_apply() {
        let path = temp_backup_path("not-applied");
        sample_record(false).save(&path).expect("seed");

        let mock = MockProxy::default();
        let did = recover_if_applied(&path, &mock).expect("recover");
        assert!(!did);
        assert_eq!(mock.restore_calls.get(), 0);
        assert_eq!(mock.apply_calls.get(), 0);

        let after = ProxyBackupFile::load(&path).expect("reload");
        assert!(!after.applied);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
