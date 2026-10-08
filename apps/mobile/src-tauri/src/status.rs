// SPDX-License-Identifier: GPL-3.0-or-later

//! Phone status snapshot. Desktop capture flags stay off so the shared UI
//! hides system proxy, the TUN card, the helper, and login items.

use ice_config::{AppError, UiMessage};
use ice_core::CoreStatus;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TunnelView {
    pub phase: TunnelPhase,
    pub permission: VpnPermission,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunnelPhase {
    Stopped,
    Connecting,
    Connected,
    Disconnecting,
    Error,
}

impl TunnelPhase {
    pub fn parse(raw: &str) -> Self {
        match raw {
            "connecting" => Self::Connecting,
            "connected" => Self::Connected,
            "disconnecting" => Self::Disconnecting,
            "error" => Self::Error,
            _ => Self::Stopped,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Disconnecting => "disconnecting",
            Self::Error => "error",
        }
    }

    pub fn is_live(self) -> bool {
        matches!(self, Self::Connecting | Self::Connected)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VpnPermission {
    Unknown,
    Granted,
    Denied,
}

impl VpnPermission {
    pub fn parse(raw: &str) -> Self {
        match raw {
            "granted" => Self::Granted,
            "denied" => Self::Denied,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Granted => "granted",
            Self::Denied => "denied",
        }
    }
}

/// Map the tunnel plugin's phase onto the core status the shared Home page reads.
pub fn core_status_for(phase: TunnelPhase) -> CoreStatus {
    match phase {
        TunnelPhase::Connected => CoreStatus::Running,
        TunnelPhase::Connecting => CoreStatus::Starting,
        TunnelPhase::Disconnecting => CoreStatus::Stopping,
        TunnelPhase::Error => CoreStatus::Error,
        TunnelPhase::Stopped => CoreStatus::Stopped,
    }
}

#[derive(Clone, Serialize)]
struct CoreState {
    status: CoreStatus,
    message: Option<UiMessage>,
    inbound_host: Option<String>,
    inbound_port: Option<u16>,
}

#[derive(Clone, Serialize)]
struct MemoryUsage {
    app_bytes: Option<u64>,
    core_bytes: Option<u64>,
    total_bytes: u64,
}

#[derive(Clone, Serialize)]
struct ProbeFreshness {
    checked_at_ms: Option<u64>,
    age_ms: Option<u64>,
    stale: bool,
    error: Option<UiMessage>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum TrafficCapture {
    Inactive,
    Tun,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum TunStatus {
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceGuidance {
    pub battery_unrestricted: bool,
    pub private_dns_strict: bool,
    pub always_on_vpn: bool,
}

#[derive(Clone, Serialize)]
pub struct SelectedOutbound {
    pub tag: String,
    pub outbound_type: String,
    pub group_now: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct StatusResponse {
    revision: u64,
    sampled_at_ms: u64,
    refresh_pending: bool,
    diagnostics: ProbeFreshness,
    workers: Vec<serde_json::Value>,
    core: CoreState,
    subscription_count: usize,
    memory: MemoryUsage,
    proxy_recovery_warning: Vec<UiMessage>,
    system_proxy_applied: Option<bool>,
    system_proxy_recorded: Option<bool>,
    system_proxy_available: bool,
    traffic_capture: TrafficCapture,
    configured_tun: bool,
    tun_status: TunStatus,
    tun_interface: Option<String>,
    tun_error: Option<AppError>,
    capture_transition_id: Option<String>,
    tun_available: bool,
    tun_unavailable_reason: Option<UiMessage>,
    tun_ui_hidden: bool,
    helper_installed: bool,
    helper_supported: bool,
    launch_at_login_supported: bool,
    tray_display_supported: bool,
    helper_stale: bool,
    tun_elevation_ready: bool,
    vpn_permission: &'static str,
    tunnel_status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    battery_unrestricted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    private_dns_strict: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    always_on_vpn: Option<bool>,
    has_nodes: bool,
    selected_outbound: Option<SelectedOutbound>,
}

pub fn status_response(
    view: TunnelView,
    message: Option<&str>,
    memory_bytes: Option<u64>,
    subscription_count: usize,
    guidance: Option<DeviceGuidance>,
    has_nodes: bool,
    selected_outbound: Option<SelectedOutbound>,
) -> StatusResponse {
    let core_status = core_status_for(view.phase);
    let core_message = match view.phase {
        TunnelPhase::Error => Some(UiMessage::raw(message.unwrap_or("tunnel error"))),
        _ => None,
    };
    let core_bytes = if view.phase == TunnelPhase::Connected {
        memory_bytes
    } else {
        None
    };
    StatusResponse {
        revision: 0,
        sampled_at_ms: now_ms(),
        refresh_pending: false,
        diagnostics: ProbeFreshness {
            checked_at_ms: None,
            age_ms: None,
            stale: false,
            error: None,
        },
        workers: Vec::new(),
        core: CoreState {
            status: core_status,
            message: core_message,
            inbound_host: None,
            inbound_port: None,
        },
        subscription_count,
        memory: MemoryUsage {
            app_bytes: None,
            core_bytes,
            total_bytes: core_bytes.unwrap_or(0),
        },
        proxy_recovery_warning: Vec::new(),
        system_proxy_applied: None,
        system_proxy_recorded: None,
        system_proxy_available: false,
        traffic_capture: if view.phase == TunnelPhase::Connected {
            TrafficCapture::Tun
        } else {
            TrafficCapture::Inactive
        },
        configured_tun: false,
        tun_status: TunStatus::Disabled,
        tun_interface: None,
        tun_error: None,
        capture_transition_id: None,
        tun_available: false,
        tun_unavailable_reason: None,
        tun_ui_hidden: true,
        helper_installed: false,
        helper_supported: false,
        launch_at_login_supported: false,
        tray_display_supported: false,
        helper_stale: false,
        tun_elevation_ready: true,
        vpn_permission: view.permission.as_str(),
        tunnel_status: view.phase.as_str(),
        battery_unrestricted: guidance.map(|item| item.battery_unrestricted),
        private_dns_strict: guidance.map(|item| item.private_dns_strict),
        always_on_vpn: guidance.map(|item| item.always_on_vpn),
        has_nodes,
        selected_outbound,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_phase_maps_onto_the_shared_core_status() {
        assert_eq!(core_status_for(TunnelPhase::Connected), CoreStatus::Running);
        assert_eq!(
            core_status_for(TunnelPhase::Connecting),
            CoreStatus::Starting
        );
        assert_eq!(
            core_status_for(TunnelPhase::Disconnecting),
            CoreStatus::Stopping
        );
        assert_eq!(core_status_for(TunnelPhase::Error), CoreStatus::Error);
        assert_eq!(core_status_for(TunnelPhase::Stopped), CoreStatus::Stopped);
        assert_eq!(
            core_status_for(TunnelPhase::parse("anything")),
            CoreStatus::Stopped
        );
    }

    #[test]
    fn connected_status_reports_tun_and_hides_desktop_capture() {
        let status = status_response(
            TunnelView {
                phase: TunnelPhase::Connected,
                permission: VpnPermission::Granted,
            },
            None,
            Some(4096),
            2,
            None,
            false,
            None,
        );
        let json = serde_json::to_value(&status).expect("status");
        assert_eq!(json["core"]["status"], "running");
        assert_eq!(json["traffic_capture"], "tun");
        assert_eq!(json["tunnel_status"], "connected");
        assert_eq!(json["vpn_permission"], "granted");
        assert_eq!(json["memory"]["core_bytes"], 4096);
        assert_eq!(json["system_proxy_available"], false);
        assert_eq!(json["tun_ui_hidden"], true);
        assert_eq!(json["tun_available"], false);
        assert_eq!(json["helper_supported"], false);
        assert_eq!(json["launch_at_login_supported"], false);
        assert!(json.get("battery_unrestricted").is_none());
        assert!(json.get("private_dns_strict").is_none());
        assert!(json.get("always_on_vpn").is_none());
        assert_eq!(json["tray_display_supported"], false);
        assert_eq!(json["tun_elevation_ready"], true);
    }
}
