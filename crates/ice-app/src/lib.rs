// SPDX-License-Identifier: GPL-3.0-or-later

//! Headless application surface shared by the desktop shell and the mobile shell.
//!
//! Desktop capture (system proxy, the TUN helper, elevation) stays in the
//! desktop shell. Subscription storage and settings persistence stay there
//! too, because those services borrow `AppState`, which also owns the capture
//! controller. This crate is the seam those shells share: the core lifecycle
//! trait, and the mobile config entry.

pub use ice_engine::{AppError, BuildInput, ConfigError, HostPlatform};

/// Lifecycle of the proxy core, as seen by shared application code.
///
/// The desktop process controller stays in the shell. The mobile tunnel
/// plugin will implement this. Shared code does not start a process or a
/// VPN itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePhase {
    Stopped,
    Preparing,
    Running,
    Reloading,
    Failed,
}

/// Last reported core lifecycle. `message` is a stable detail for `Failed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeState {
    pub phase: RuntimePhase,
    pub message: Option<String>,
}

/// VPN tunnel lifecycle reported by the platform plugin.
///
/// The strings match `StatusResponse.tunnel_status` in the shared UI contract.
/// Android and iOS map their own service states onto these values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelPhase {
    Stopped,
    Connecting,
    Connected,
    Disconnecting,
    Error,
}

/// Whether the user has granted the platform VPN consent.
///
/// The strings match `StatusResponse.vpn_permission`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VpnPermission {
    Unknown,
    Granted,
    Denied,
}

/// Host-defined core lifecycle.
///
/// `prepare` obtains VPN permission on mobile and is a no-op check on desktop.
/// `reload` asks a running core to read the config again.
pub trait CoreRuntime {
    fn prepare(&mut self) -> Result<(), AppError>;
    fn start(&mut self) -> Result<(), AppError>;
    fn stop(&mut self) -> Result<(), AppError>;
    fn reload(&mut self) -> Result<(), AppError>;
    fn state(&self) -> RuntimeState;
}

/// Build the sing-box config for Android or iOS.
///
/// `shared_root` is required even when the platform maps it to the same
/// directory as the host-private root. `capture_intent` is ignored: mobile
/// capture is always TUN. An empty profile becomes the direct-only config.
pub fn build_mobile_config(input: &BuildInput) -> Result<serde_json::Value, ConfigError> {
    if !input.platform.is_mobile() {
        return Err(ConfigError::invalid(
            "mobile config requires HostPlatform::Android or HostPlatform::Ios",
        ));
    }
    if input
        .shared_root
        .as_ref()
        .is_none_or(|root| root.as_os_str().is_empty())
    {
        return Err(ConfigError::invalid("mobile config requires a shared root"));
    }
    ice_engine::build_mobile_config(input)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use ice_engine::{
        BuildInput, CaptureIntent, GroupSelections, HostPlatform, LocalTemplate, NormalizedProfile,
        RuleOverrides,
    };

    use super::{
        build_mobile_config, CoreRuntime, RuntimePhase, RuntimeState, TunnelPhase, VpnPermission,
    };

    fn input(platform: HostPlatform, shared_root: Option<PathBuf>) -> BuildInput {
        BuildInput {
            template: LocalTemplate::default(),
            profile: Arc::new(NormalizedProfile::from_nodes_only(vec![])),
            selected_tag: None,
            geoip_rule_set_dir: None,
            group_selections: GroupSelections::new(),
            rule_overrides: RuleOverrides::default(),
            capture_intent: CaptureIntent::Diagnostic,
            tun_exclude_package: Some("com.yilongmusk.icebox".into()),
            shared_root,
            platform,
        }
    }

    #[test]
    fn android_direct_config_uses_the_shared_root_and_package() {
        let cfg = build_mobile_config(&input(
            HostPlatform::Android,
            Some(PathBuf::from("/shared")),
        ))
        .expect("android");
        assert_eq!(
            cfg["inbounds"][0]["exclude_package"][0],
            "com.yilongmusk.icebox"
        );
        assert_eq!(cfg["inbounds"][0]["type"], "tun");
        assert!(cfg["inbounds"][0].get("interface_name").is_none());
        assert_eq!(cfg["experimental"]["cache_file"]["enabled"], true);
        assert!(cfg["log"]["output"]
            .as_str()
            .unwrap()
            .ends_with("sing-box.log"));
    }

    #[test]
    fn mobile_config_rejects_desktop_and_a_missing_root() {
        assert!(
            build_mobile_config(&input(HostPlatform::MacOs, Some(PathBuf::from("/shared"))))
                .is_err()
        );
        assert!(build_mobile_config(&input(HostPlatform::Android, None)).is_err());
        assert!(build_mobile_config(&input(HostPlatform::Ios, Some(PathBuf::new()))).is_err());
    }

    struct RecordingRuntime {
        phase: RuntimePhase,
        calls: Vec<&'static str>,
    }

    impl CoreRuntime for RecordingRuntime {
        fn prepare(&mut self) -> Result<(), ice_engine::AppError> {
            self.calls.push("prepare");
            self.phase = RuntimePhase::Preparing;
            Ok(())
        }

        fn start(&mut self) -> Result<(), ice_engine::AppError> {
            self.calls.push("start");
            self.phase = RuntimePhase::Running;
            Ok(())
        }

        fn stop(&mut self) -> Result<(), ice_engine::AppError> {
            self.calls.push("stop");
            self.phase = RuntimePhase::Stopped;
            Ok(())
        }

        fn reload(&mut self) -> Result<(), ice_engine::AppError> {
            self.calls.push("reload");
            self.phase = RuntimePhase::Reloading;
            Ok(())
        }

        fn state(&self) -> RuntimeState {
            RuntimeState {
                phase: self.phase,
                message: None,
            }
        }
    }

    #[test]
    fn core_lifecycle_is_implemented_by_the_host() {
        let mut runtime = RecordingRuntime {
            phase: RuntimePhase::Stopped,
            calls: Vec::new(),
        };
        runtime.prepare().unwrap();
        runtime.start().unwrap();
        runtime.reload().unwrap();
        runtime.stop().unwrap();
        assert_eq!(runtime.calls, ["prepare", "start", "reload", "stop"]);
        assert_eq!(runtime.state().phase, RuntimePhase::Stopped);
    }

    #[test]
    fn tunnel_status_serializes_to_the_status_snapshot_strings() {
        assert_eq!(
            serde_json::to_value(TunnelPhase::Connecting).unwrap(),
            serde_json::json!("connecting")
        );
        assert_eq!(
            serde_json::to_value(VpnPermission::Denied).unwrap(),
            serde_json::json!("denied")
        );
        assert_eq!(
            serde_json::from_value::<TunnelPhase>(serde_json::json!("disconnecting")).unwrap(),
            TunnelPhase::Disconnecting
        );
        assert_eq!(
            serde_json::from_value::<VpnPermission>(serde_json::json!("granted")).unwrap(),
            VpnPermission::Granted
        );
    }

    #[test]
    fn manifest_has_no_desktop_capture_deps() {
        let manifest = include_str!("../Cargo.toml");
        for name in [
            "tauri",
            "ice-proxy-sys",
            "ice-tun",
            "ice-helper",
            "ice-elevate",
        ] {
            assert!(
                !manifest.contains(name),
                "{name} must not appear in ice-app"
            );
        }
    }
}
