//! `settings.json` load / save (architecture §6.1).

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::atomic::write_json_atomic;
use crate::error::{AppError, ErrorCode};
use crate::listen::is_loopback_host;
use crate::LocalTemplate;

/// Routing mode: rule-based routing, all traffic through the selected proxy, or all direct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    /// Route by the active subscription's `route.rules` (default).
    #[default]
    Rule,
    /// Ignore rules; send all traffic through the selected proxy / strategy group.
    Global,
    /// Ignore rules; send all traffic out `direct`.
    Direct,
}

/// Capitalized Clash runtime mode, matching sing-box's case-sensitive `mode-list`
/// membership checks (the pinned 1.13.19 does not accept an emitted `mode_list`).
///
/// sing-box `experimental/clashapi` `NewServer` starts with an empty `mode-list` and
/// prepends `default_mode` when it is missing, so the runtime list is `[<default_mode>]`
/// — a single entry, not `["Rule", "Global", "Direct"]`. `SetMode` checks membership
/// case-sensitively, so a lowercase `"global"` would be silently ignored (and, were the
/// entry present, pollute `GET /configs` `mode-list` with a mixed-case duplicate). The
/// `clash_mode` route rule matches case-insensitively, so routing behaves the same either
/// way; the capitalized form keeps the reported `mode` / `mode-list` clean.
pub fn clash_mode_name(mode: ProxyMode) -> &'static str {
    match mode {
        ProxyMode::Rule => "Rule",
        ProxyMode::Global => "Global",
        ProxyMode::Direct => "Direct",
    }
}

/// Application settings (not the sing-box runtime config).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSettings {
    pub mixed_listen: String,
    pub mixed_port: u16,
    pub clash_api_listen: String,
    pub clash_api_port: u16,
    pub selected_tag: Option<String>,
    pub auto_set_system_proxy: bool,
    /// When true, the mixed inbound binds `0.0.0.0` so LAN devices can use the proxy.
    /// Defaults to false for existing `settings.json` files (`#[serde(default)]`).
    #[serde(default)]
    pub allow_lan: bool,
    /// Routing mode; defaults to `rule` for existing `settings.json` files.
    #[serde(default)]
    pub proxy_mode: ProxyMode,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            mixed_listen: "127.0.0.1".into(),
            mixed_port: 17890,
            clash_api_listen: "127.0.0.1".into(),
            clash_api_port: 19090,
            selected_tag: None,
            auto_set_system_proxy: cfg!(target_os = "macos"),
            allow_lan: false,
            proxy_mode: ProxyMode::Rule,
        }
    }
}

impl AppSettings {
    /// Reject wildcard / non-loopback listens. Does not mutate or write disk.
    pub fn validate(&self) -> Result<(), AppError> {
        validate_listen_addr("clash_api_listen", &self.clash_api_listen)?;
        if !is_loopback_host(&self.clash_api_listen) {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                format!(
                    "clash_api_listen must be a loopback address, got {}",
                    self.clash_api_listen
                ),
            ));
        }
        // With allow_lan the mixed inbound binds 0.0.0.0 at build time, so the stored
        // mixed_listen is only meaningful when allow_lan is off.
        if !self.allow_lan {
            validate_listen_addr("mixed_listen", &self.mixed_listen)?;
            if !is_loopback_host(&self.mixed_listen) {
                return Err(AppError::new(
                    ErrorCode::ConfigInvalid,
                    format!(
                        "mixed_listen must be a loopback address, got {}",
                        self.mixed_listen
                    ),
                ));
            }
        }
        if self.mixed_port < 1024 || self.clash_api_port < 1024 {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                "ports must be in 1024..=65535",
            ));
        }
        if self.mixed_port == self.clash_api_port {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                "mixed_port must differ from clash_api_port",
            ));
        }
        Ok(())
    }

    pub fn to_local_template(&self) -> LocalTemplate {
        LocalTemplate {
            mixed_listen: self.mixed_listen.clone(),
            mixed_port: self.mixed_port,
            clash_api_listen: self.clash_api_listen.clone(),
            clash_api_port: self.clash_api_port,
            allow_lan: self.allow_lan,
            proxy_mode: self.proxy_mode,
        }
    }
}

fn is_unspecified(addr: &str) -> bool {
    matches!(addr, "0.0.0.0" | "::" | "[::]")
}

fn validate_listen_addr(field: &str, addr: &str) -> Result<(), AppError> {
    if is_unspecified(addr) {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            format!("{field} must not be {addr}; use 127.0.0.1"),
        ));
    }
    Ok(())
}

/// Missing file → architecture §6.1 defaults (does not create the file).
pub fn load_settings(path: &Path) -> Result<AppSettings, AppError> {
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let raw = fs::read_to_string(path).map_err(|e| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("read settings {}: {e}", path.display()),
        )
    })?;
    let settings: AppSettings = serde_json::from_str(&raw)
        .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("parse settings: {e}")))?;
    settings.validate()?;
    Ok(settings)
}

/// Validate then atomically write. Invalid listens are rejected (no disk write).
pub fn save_settings(path: &Path, settings: &AppSettings) -> Result<(), AppError> {
    settings.validate()?;
    write_json_atomic(path, settings).map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_settings_path(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-settings-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("mkdir");
        dir.join("settings.json")
    }

    #[test]
    fn missing_settings_file_returns_architecture_defaults() {
        let path = temp_settings_path("missing");
        assert!(!path.exists());
        let s = load_settings(&path).expect("load");
        let d = AppSettings::default();
        assert_eq!(s.mixed_listen, "127.0.0.1");
        assert_eq!(s.mixed_port, 17890);
        assert_eq!(s.clash_api_listen, "127.0.0.1");
        assert_eq!(s.clash_api_port, 19090);
        assert_eq!(s.selected_tag, None);
        assert_eq!(s.auto_set_system_proxy, cfg!(target_os = "macos"));
        assert!(!s.allow_lan);
        assert_eq!(s, d);
        assert!(!path.exists(), "load must not create settings file");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn legacy_settings_without_allow_lan_loads_as_false() {
        let path = temp_settings_path("legacy");
        let json = r#"{
            "mixed_listen": "127.0.0.1",
            "mixed_port": 17890,
            "clash_api_listen": "127.0.0.1",
            "clash_api_port": 19090,
            "selected_tag": null,
            "auto_set_system_proxy": true
        }"#;
        fs::write(&path, json).expect("write");
        let s = load_settings(&path).expect("legacy json without allow_lan");
        assert!(!s.allow_lan);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn legacy_settings_without_proxy_mode_loads_as_rule() {
        let path = temp_settings_path("legacy-mode");
        let json = r#"{
            "mixed_listen": "127.0.0.1",
            "mixed_port": 17890,
            "clash_api_listen": "127.0.0.1",
            "clash_api_port": 19090,
            "selected_tag": null,
            "auto_set_system_proxy": true
        }"#;
        fs::write(&path, json).expect("write");
        let s = load_settings(&path).expect("legacy json without proxy_mode");
        assert_eq!(s.proxy_mode, ProxyMode::Rule);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn proxy_mode_round_trips() {
        let path = temp_settings_path("mode-roundtrip");
        save_settings(
            &path,
            &AppSettings {
                proxy_mode: ProxyMode::Global,
                ..AppSettings::default()
            },
        )
        .expect("save global");
        assert_eq!(load_settings(&path).unwrap().proxy_mode, ProxyMode::Global);

        save_settings(
            &path,
            &AppSettings {
                proxy_mode: ProxyMode::Direct,
                ..AppSettings::default()
            },
        )
        .expect("save direct");
        assert_eq!(load_settings(&path).unwrap().proxy_mode, ProxyMode::Direct);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn clash_mode_name_covers_all_supported_modes() {
        assert_eq!(clash_mode_name(ProxyMode::Rule), "Rule");
        assert_eq!(clash_mode_name(ProxyMode::Global), "Global");
        assert_eq!(clash_mode_name(ProxyMode::Direct), "Direct");
    }

    /// The WinInet backend (slice 4b) made the Windows system proxy real, so the flag must
    /// be accepted on every platform (settings files carrying it must load).
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_accepts_auto_set_system_proxy() {
        let path = temp_settings_path("win-proxy");
        let settings = AppSettings {
            auto_set_system_proxy: true,
            ..AppSettings::default()
        };
        save_settings(&path, &settings).expect("win auto proxy accepted");
        let loaded = load_settings(&path).expect("reload");
        assert!(loaded.auto_set_system_proxy);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn reject_unspecified_listen_without_writing() {
        let path = temp_settings_path("bad-listen");
        fs::write(&path, b"keep-me").expect("seed");

        let bad_clash = AppSettings {
            clash_api_listen: "0.0.0.0".into(),
            ..AppSettings::default()
        };
        let err = save_settings(&path, &bad_clash).expect_err("reject clash 0.0.0.0");
        assert_eq!(err.code, "config.invalid");
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep-me");

        let bad_mixed = AppSettings {
            mixed_listen: "0.0.0.0".into(),
            ..AppSettings::default()
        };
        let err = save_settings(&path, &bad_mixed).expect_err("reject mixed 0.0.0.0");
        assert_eq!(err.code, "config.invalid");
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep-me");

        let bad_mixed_lan = AppSettings {
            mixed_listen: "192.168.1.1".into(),
            ..AppSettings::default()
        };
        let err = save_settings(&path, &bad_mixed_lan).expect_err("reject mixed lan");
        assert_eq!(err.code, "config.invalid");
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep-me");

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn allow_lan_relaxes_mixed_listen_validation() {
        let path = temp_settings_path("allow-lan");
        fs::write(&path, b"keep-me").expect("seed");

        let lan = AppSettings {
            allow_lan: true,
            mixed_listen: "0.0.0.0".into(),
            ..AppSettings::default()
        };
        save_settings(&path, &lan).expect("allow_lan accepts any mixed_listen");
        let on_disk = load_settings(&path).expect("reload");
        assert!(on_disk.allow_lan);

        let clash_lan = AppSettings {
            allow_lan: true,
            clash_api_listen: "0.0.0.0".into(),
            ..AppSettings::default()
        };
        let before = fs::read_to_string(&path).unwrap();
        let err = save_settings(&path, &clash_lan).expect_err("clash api stays loopback");
        assert_eq!(err.code, "config.invalid");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            before,
            "no disk write on reject"
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn reject_invalid_ports_without_writing() {
        let path = temp_settings_path("bad-port");
        fs::write(&path, b"keep-me").expect("seed");

        let bad = AppSettings {
            mixed_port: 80,
            ..AppSettings::default()
        };
        let err = save_settings(&path, &bad).expect_err("low port");
        assert_eq!(err.code, "config.invalid");
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep-me");

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn reject_out_of_range_port_in_settings_json() {
        let path = temp_settings_path("high-load");
        let json = r#"{
            "mixed_listen": "127.0.0.1",
            "mixed_port": 70000,
            "clash_api_listen": "127.0.0.1",
            "clash_api_port": 19090,
            "selected_tag": null,
            "auto_set_system_proxy": false
        }"#;
        fs::write(&path, json).expect("write");
        let err = load_settings(&path).expect_err("out of range port");
        assert_eq!(err.code, "config.invalid");
        assert!(err.message.contains("parse settings"));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
