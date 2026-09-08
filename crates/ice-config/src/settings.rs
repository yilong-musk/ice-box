// SPDX-License-Identifier: GPL-3.0-or-later

//! `settings.json` load / save (architecture §6.1).

use std::fs;
use std::path::Path;

use crate::atomic::write_json_atomic;
use crate::error::{AppError, ErrorCode};
use crate::HostPlatform;

pub use ice_types::{
    clash_mode_name, default_auto_set_system_proxy, tun_interface_name_valid, AppSettings,
    CoreLogLevel, LanguagePreference, ProxyMode, SettingsPatch, TunSettings, TunSettingsPatch,
    TUN_DEFAULT_IPV4_ADDRESS, TUN_DEFAULT_IPV6_ADDRESS, TUN_DEFAULT_MTU, TUN_DEFAULT_STACK,
};

/// Result of loading `settings.json`, including a one-shot recovery diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadSettingsOutcome {
    pub settings: AppSettings,
    /// Set when a corrupt / invalid file was renamed aside and defaults loaded.
    pub reset_reason: Option<String>,
}

/// Missing file → architecture §6.1 defaults (does not create the file).
/// Parse / validation failure: rename to `settings.json.invalid-<timestamp>`
/// and return defaults plus `reset_reason` (`settings.reset`).
pub fn load_settings(path: &Path) -> Result<AppSettings, AppError> {
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    match try_load_settings(path) {
        Ok(settings) => Ok(settings),
        Err(SettingsLoadError::Io(err)) => Err(AppError::new(
            ErrorCode::ConfigInvalid,
            format!("read settings: {err}"),
        )),
        Err(SettingsLoadError::Invalid(_)) => Ok(load_settings_detailed(path).settings),
    }
}

/// Like [`load_settings`], but surfaces whether the file was reset.
pub fn load_settings_detailed(path: &Path) -> LoadSettingsOutcome {
    if !path.exists() {
        return LoadSettingsOutcome {
            settings: AppSettings::default(),
            reset_reason: None,
        };
    }
    match try_load_settings(path) {
        Ok(settings) => LoadSettingsOutcome {
            settings,
            reset_reason: None,
        },
        Err(SettingsLoadError::Io(reason)) => {
            tracing::warn!(path = %path.display(), reason = %reason, "settings.json could not be read");
            LoadSettingsOutcome {
                settings: AppSettings::default(),
                reset_reason: None,
            }
        }
        Err(SettingsLoadError::Invalid(reason)) => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let backup_name = format!("settings.json.invalid-{ts}");
            let backup = path.with_file_name(backup_name);
            match fs::rename(path, &backup) {
                Ok(()) => tracing::warn!(
                    path = %path.display(),
                    backup = %backup.display(),
                    reason = %reason,
                    "invalid settings.json reset to defaults"
                ),
                Err(err) => tracing::error!(
                    path = %path.display(),
                    error = %err,
                    reason = %reason,
                    "failed to quarantine invalid settings.json"
                ),
            }
            LoadSettingsOutcome {
                settings: AppSettings::default(),
                reset_reason: Some(reason),
            }
        }
    }
}

enum SettingsLoadError {
    Io(String),
    Invalid(String),
}

fn try_load_settings(path: &Path) -> Result<AppSettings, SettingsLoadError> {
    let raw = fs::read_to_string(path).map_err(|e| SettingsLoadError::Io(e.to_string()))?;
    let settings: AppSettings = serde_json::from_str(&raw)
        .map_err(|e| SettingsLoadError::Invalid(format!("parse settings: {e}")))?;
    settings
        .validate()
        .map_err(|e| SettingsLoadError::Invalid(format!("validate settings: {e}")))?;
    Ok(settings)
}

/// Validate then atomically write. Invalid listens are rejected (no disk write).
///
/// Generic (non-macOS) interface-name rules. Prefer [`save_settings_for`] when
/// the host platform is known so macOS `utun<N>` is enforced.
pub fn save_settings(path: &Path, settings: &AppSettings) -> Result<(), AppError> {
    save_settings_for(path, settings, HostPlatform::Linux)
}

pub fn save_settings_for(
    path: &Path,
    settings: &AppSettings,
    platform: HostPlatform,
) -> Result<(), AppError> {
    settings.validate_for(platform)?;
    write_json_atomic(path, settings).map_err(AppError::from)
}

/// Persist only the last user-desired proxy-service state.
///
/// No-op when the value is unchanged, including a missing `settings.json`
/// whose load default is already `false` (does not create the file).
/// Quit / crash cleanup must not call this with `false`: stopping capture on
/// exit is not a user-off.
pub fn set_proxy_service_enabled(path: &Path, enabled: bool) -> Result<(), AppError> {
    set_proxy_service_enabled_for(path, enabled, HostPlatform::Linux)
}

pub fn set_proxy_service_enabled_for(
    path: &Path,
    enabled: bool,
    platform: HostPlatform,
) -> Result<(), AppError> {
    let mut settings = load_settings(path)?;
    if settings.proxy_service_enabled == enabled {
        return Ok(());
    }
    settings.proxy_service_enabled = enabled;
    save_settings_for(path, &settings, platform)
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
        assert_eq!(s.auto_set_system_proxy, default_auto_set_system_proxy());
        assert!(!s.allow_lan);
        assert_eq!(s, d);
        assert!(!path.exists(), "load must not create settings file");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn default_auto_set_system_proxy_matches_real_backends() {
        assert!(
            !default_auto_set_system_proxy(),
            "system proxy is home-button controlled, not auto on Start"
        );
        assert_eq!(
            AppSettings::default().auto_set_system_proxy,
            default_auto_set_system_proxy()
        );
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
    fn legacy_settings_without_language_loads_as_system() {
        let path = temp_settings_path("legacy-language");
        let json = r#"{
            "mixed_listen": "127.0.0.1",
            "mixed_port": 17890,
            "clash_api_listen": "127.0.0.1",
            "clash_api_port": 19090,
            "selected_tag": null,
            "auto_set_system_proxy": true
        }"#;
        fs::write(&path, json).expect("write");
        let s = load_settings(&path).expect("legacy json without language");
        assert_eq!(s.language, LanguagePreference::System);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn language_round_trips_through_save_and_load() {
        let path = temp_settings_path("language-roundtrip");
        for language in [LanguagePreference::Zh, LanguagePreference::En] {
            save_settings(
                &path,
                &AppSettings {
                    language,
                    ..AppSettings::default()
                },
            )
            .expect("save language");
            assert_eq!(load_settings(&path).unwrap().language, language);
        }
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn language_default_is_system() {
        assert_eq!(AppSettings::default().language, LanguagePreference::System);
    }

    #[test]
    fn check_app_updates_defaults_on_for_legacy_files() {
        let path = temp_settings_path("legacy-check-app-updates");
        let json = r#"{
            "mixed_listen": "127.0.0.1",
            "mixed_port": 17890,
            "clash_api_listen": "127.0.0.1",
            "clash_api_port": 19090,
            "selected_tag": null,
            "auto_set_system_proxy": true
        }"#;
        fs::write(&path, json).expect("write");
        let s = load_settings(&path).expect("legacy json without check_app_updates");
        assert!(
            s.check_app_updates,
            "missing flag must mean on so existing installs keep auto-check"
        );
        assert!(AppSettings::default().check_app_updates);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn legacy_settings_without_proxy_service_enabled_loads_as_false() {
        let path = temp_settings_path("legacy-proxy-service");
        let json = r#"{
            "mixed_listen": "127.0.0.1",
            "mixed_port": 17890,
            "clash_api_listen": "127.0.0.1",
            "clash_api_port": 19090,
            "selected_tag": null,
            "auto_set_system_proxy": true
        }"#;
        fs::write(&path, json).expect("write");
        let s = load_settings(&path).expect("legacy json without proxy_service_enabled");
        assert!(
            !s.proxy_service_enabled,
            "missing flag must mean off so launch stays core-only"
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn proxy_service_enabled_round_trips_through_save_and_load() {
        let path = temp_settings_path("proxy-service-roundtrip");
        save_settings(
            &path,
            &AppSettings {
                proxy_service_enabled: true,
                ..AppSettings::default()
            },
        )
        .expect("save");
        assert!(load_settings(&path).unwrap().proxy_service_enabled);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn set_proxy_service_enabled_does_not_create_file_when_already_off() {
        let path = temp_settings_path("proxy-service-noop");
        assert!(!path.exists());
        set_proxy_service_enabled(&path, false).expect("noop");
        assert!(!path.exists(), "default-off must not create settings.json");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn set_proxy_service_enabled_preserves_other_fields() {
        let path = temp_settings_path("proxy-service-preserve");
        save_settings(
            &path,
            &AppSettings {
                mixed_port: 18080,
                selected_tag: Some("n1".into()),
                ..AppSettings::default()
            },
        )
        .expect("seed");
        set_proxy_service_enabled(&path, true).expect("enable flag");
        let loaded = load_settings(&path).expect("reload");
        assert!(loaded.proxy_service_enabled);
        assert_eq!(loaded.mixed_port, 18080);
        assert_eq!(loaded.selected_tag.as_deref(), Some("n1"));
        set_proxy_service_enabled(&path, false).expect("disable flag");
        let loaded = load_settings(&path).expect("reload");
        assert!(!loaded.proxy_service_enabled);
        assert_eq!(loaded.mixed_port, 18080);
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
        let loaded = load_settings_detailed(&path);
        assert_eq!(loaded.settings, AppSettings::default());
        assert!(
            loaded
                .reset_reason
                .as_ref()
                .is_some_and(|r| r.contains("parse settings")),
            "reset reason: {:?}",
            loaded.reset_reason
        );
        assert!(
            path.parent().unwrap().read_dir().unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("settings.json.invalid-")),
            "corrupt file must be quarantined"
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn corrupt_settings_json_resets_to_defaults() {
        let path = temp_settings_path("corrupt");
        fs::write(&path, b"{not-json").expect("write");
        let loaded = load_settings_detailed(&path);
        assert_eq!(loaded.settings.mixed_port, 17890);
        assert!(loaded.reset_reason.is_some());
        assert!(!path.exists() || load_settings(&path).unwrap() == AppSettings::default());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    // --- TUN settings (slice T1, plan §4.1) ---

    #[test]
    fn legacy_settings_without_tun_loads_disabled_with_locked_defaults() {
        let path = temp_settings_path("legacy-tun");
        let json = r#"{
            "mixed_listen": "127.0.0.1",
            "mixed_port": 17890,
            "clash_api_listen": "127.0.0.1",
            "clash_api_port": 19090,
            "selected_tag": null,
            "auto_set_system_proxy": true
        }"#;
        fs::write(&path, json).expect("write");
        let s = load_settings(&path).expect("legacy json without tun");
        assert!(
            !s.tun.enabled,
            "missing TUN fields must mean disabled (no silent migration)"
        );
        assert_eq!(s.tun, TunSettings::default());
        assert_eq!(s.tun.ipv4_address, "10.0.0.1/30");
        assert_eq!(s.tun.ipv6_address, "fdfe:dcba:9876::1/126");
        assert_eq!(s.tun.mtu, 9000);
        assert_eq!(s.tun.stack, "gvisor");
        assert!(s.tun.auto_route);
        assert!(s.tun.strict_route);
        assert!(s.tun.dns_hijack, "dns hijack is the locked default");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn tun_enabled_round_trips_through_save_and_load() {
        let path = temp_settings_path("tun-roundtrip");
        let settings = AppSettings {
            tun: TunSettings {
                enabled: true,
                interface_name: Some("utun420".into()),
                ..TunSettings::default()
            },
            ..AppSettings::default()
        };
        save_settings(&path, &settings).expect("save tun settings");
        let loaded = load_settings(&path).expect("reload");
        assert!(loaded.tun.enabled);
        assert_eq!(loaded.tun.interface_name.as_deref(), Some("utun420"));
        assert_eq!(loaded.tun, settings.tun);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn tun_validation_rejects_bad_cidrs_without_writing() {
        let path = temp_settings_path("tun-bad-cidr");
        fs::write(&path, b"keep-me").expect("seed");
        let base = || AppSettings {
            tun: TunSettings {
                enabled: true,
                ..TunSettings::default()
            },
            ..AppSettings::default()
        };

        let cases = [
            "10.0.0.1",              // not a CIDR
            "10.0.0.1/",             // empty prefix
            "10.0.0.1/33",           // prefix out of range
            "10.0.0.1/0",            // /0 interface address rejected
            "10.0.0.1/24/x",         // extra segment
            "999.1.1.1/24",          // bad octet
            "fdfe:dcba:9876::1",     // v6 without prefix
            "fdfe:dcba:9876::1/129", // v6 prefix out of range
            "fdfe:dcba:9876::1/0",   // v6 /0 rejected
            "not-an-address/24",     // junk address
        ];
        for cidr in cases {
            let bad = AppSettings {
                tun: TunSettings {
                    ipv4_address: cidr.into(),
                    ..TunSettings::default()
                },
                ..base()
            };
            let err = save_settings(&path, &bad).expect_err("reject bad v4 cidr");
            assert_eq!(err.code, "config.invalid", "case: {cidr}");
            assert_eq!(fs::read_to_string(&path).unwrap(), "keep-me");
        }
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn tun_validation_rejects_bad_ipv6_and_mtu_and_stack() {
        let path = temp_settings_path("tun-bad-rest");
        fs::write(&path, b"keep-me").expect("seed");
        let mut settings = AppSettings {
            tun: TunSettings {
                enabled: true,
                ..TunSettings::default()
            },
            ..AppSettings::default()
        };

        settings.tun.ipv6_address = "10.0.0.1/24".into();
        let err = save_settings(&path, &settings).expect_err("v4 address in v6 field");
        assert_eq!(err.code, "config.invalid");
        assert!(err.message.contains("tun.ipv6_address"));

        settings.tun = TunSettings::default();
        settings.tun.mtu = 576;
        let err = save_settings(&path, &settings).expect_err("mtu below minimum");
        assert_eq!(err.code, "config.invalid");
        assert!(err.message.contains("tun.mtu"));

        settings.tun = TunSettings::default();
        settings.tun.stack = "tap".into();
        let err = save_settings(&path, &settings).expect_err("unknown stack");
        assert_eq!(err.code, "config.invalid");
        assert!(err.message.contains("tun.stack"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep-me");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn tun_validation_rejects_bad_interface_names() {
        let path = temp_settings_path("tun-bad-iface");
        fs::write(&path, b"keep-me").expect("seed");
        let mut settings = AppSettings {
            tun: TunSettings {
                enabled: true,
                ..TunSettings::default()
            },
            ..AppSettings::default()
        };
        for name in [
            "",
            " ",
            "with space",
            "a/b",
            "a\\b",
            "a\nb",
            &"x".repeat(65),
        ] {
            settings.tun.interface_name = Some(name.into());
            let err = save_settings(&path, &settings).expect_err("reject bad interface name");
            assert_eq!(err.code, "config.invalid", "case: {name:?}");
            assert_eq!(fs::read_to_string(&path).unwrap(), "keep-me");
        }
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn macos_tun_interface_name_requires_utun_numeric_suffix() {
        let path = temp_settings_path("tun-macos-iface");
        fs::write(&path, b"keep-me").expect("seed");
        let mut settings = AppSettings {
            tun: TunSettings {
                enabled: true,
                ..TunSettings::default()
            },
            ..AppSettings::default()
        };
        for name in ["tun0", "utun", "utunx", "utun-1", "Utun4"] {
            settings.tun.interface_name = Some(name.into());
            let err = save_settings_for(&path, &settings, HostPlatform::MacOs)
                .expect_err("reject non-utun<N>");
            assert_eq!(err.code, "config.invalid", "case: {name}");
        }
        for name in ["utun0", "utun420", "utun0007"] {
            settings.tun.interface_name = Some(name.into());
            save_settings_for(&path, &settings, HostPlatform::MacOs).expect("accept utun<N>");
        }
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn non_macos_accepts_arbitrary_sane_interface_names() {
        // Platform-exact checks belong to each platform backend (T2); the shared
        // validator only enforces the sanity rules off-macOS.
        assert!(tun_interface_name_valid("utun420", false));
        assert!(tun_interface_name_valid("wintun-ice-box", false));
        assert!(tun_interface_name_valid("Tun0", false));
        assert!(!tun_interface_name_valid("with space", false));
        assert!(!tun_interface_name_valid("a/b", false));
        assert!(!tun_interface_name_valid("tun0", true));
        assert!(tun_interface_name_valid("utun420", true));
    }

    #[test]
    fn tun_defaults_are_locked_and_never_enable_capture() {
        let d = TunSettings::default();
        assert!(!d.enabled);
        assert_eq!(d.ipv4_address, TUN_DEFAULT_IPV4_ADDRESS);
        assert_eq!(d.ipv6_address, TUN_DEFAULT_IPV6_ADDRESS);
        assert_eq!(d.mtu, TUN_DEFAULT_MTU);
        assert_eq!(d.stack, TUN_DEFAULT_STACK);
        assert!(d.auto_route && d.strict_route);
        assert!(d.dns_hijack, "dns hijack is the locked default");
    }

    #[test]
    fn settings_patch_merges_disjoint_fields() {
        let base = AppSettings {
            mixed_port: 17890,
            allow_lan: false,
            selected_tag: Some("a".into()),
            proxy_service_enabled: true,
            tun: TunSettings {
                enabled: false,
                ..TunSettings::default()
            },
            ..AppSettings::default()
        };
        let from_home = SettingsPatch {
            tun: Some(TunSettingsPatch {
                enabled: Some(true),
                ..TunSettingsPatch::default()
            }),
            ..SettingsPatch::default()
        };
        let from_settings = SettingsPatch {
            mixed_port: Some(18080),
            allow_lan: Some(true),
            ..SettingsPatch::default()
        };
        let after_home = base.apply_patch(&from_home);
        let merged = after_home.apply_patch(&from_settings);
        assert!(merged.tun.enabled);
        assert_eq!(merged.mixed_port, 18080);
        assert!(merged.allow_lan);
        assert_eq!(merged.selected_tag.as_deref(), Some("a"));
        assert!(
            merged.proxy_service_enabled,
            "start/stop flag is preserved when the patch omits it"
        );
        let from_home_power = SettingsPatch {
            proxy_service_enabled: Some(false),
            ..SettingsPatch::default()
        };
        assert!(
            !merged.apply_patch(&from_home_power).proxy_service_enabled,
            "an explicit patch can persist Home start/stop"
        );
    }

    #[test]
    fn settings_patch_null_clears_selected_tag() {
        let base = AppSettings {
            selected_tag: Some("keep".into()),
            ..AppSettings::default()
        };
        let patch: SettingsPatch = serde_json::from_str(r#"{"selected_tag":null}"#).expect("patch");
        let next = base.apply_patch(&patch);
        assert_eq!(next.selected_tag, None);
    }
}
