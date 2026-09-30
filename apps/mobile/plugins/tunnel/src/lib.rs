// SPDX-License-Identifier: GPL-3.0-or-later

//! VPN tunnel plugin.
//!
//! Android implements the commands in `android/`. iOS will implement the same
//! command names later. Desktop builds of this crate return
//! [`TunnelError::Unavailable`] so the mobile shell still type-checks on a
//! host that is not Android.

use serde::{Deserialize, Serialize};
use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

#[cfg(target_os = "android")]
use tauri::plugin::PluginHandle;

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "com.yilongmusk.icebox.tunnel";

/// Android system facts the phone Settings page explains.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DeviceStatus {
    pub battery_unrestricted: bool,
    pub private_dns_strict: bool,
    pub always_on_vpn: bool,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
struct UrlRequest<'a> {
    url: &'a str,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
struct PathRequest<'a> {
    path: &'a str,
}

#[derive(Debug, thiserror::Error)]
pub enum TunnelError {
    #[error("the tunnel plugin is only available on Android")]
    Unavailable,
    #[error("{0}")]
    Plugin(String),
}

/// Snapshot the Kotlin side returns from `status`.
///
/// `phase` matches [`ice_app::TunnelPhase`]. `permission` matches
/// [`ice_app::VpnPermission`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TunnelSnapshot {
    pub phase: String,
    pub permission: String,
    pub package_name: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub memory_bytes: Option<u64>,
}

#[cfg(target_os = "android")]
#[derive(Debug, Clone, Deserialize)]
struct PermissionResponse {
    permission: String,
}

#[cfg(target_os = "android")]
#[derive(Debug, Clone, Deserialize)]
struct DirResponse {
    path: String,
}

#[cfg(target_os = "android")]
#[derive(Debug, Clone, Deserialize)]
struct MemoryResponse {
    #[serde(default)]
    bytes: Option<u64>,
}

pub struct Tunnel<R: Runtime> {
    #[cfg(target_os = "android")]
    handle: PluginHandle<R>,
    #[cfg(not(target_os = "android"))]
    _marker: std::marker::PhantomData<fn() -> R>,
}

impl<R: Runtime> Tunnel<R> {
    /// Ask the system for VPN consent. Resolves after the user answers.
    pub fn prepare(&self) -> Result<String, TunnelError> {
        #[cfg(target_os = "android")]
        {
            let response: PermissionResponse = self
                .handle
                .run_mobile_plugin("prepare", ())
                .map_err(|err| TunnelError::Plugin(err.to_string()))?;
            return Ok(response.permission);
        }
        #[cfg(not(target_os = "android"))]
        {
            Err(TunnelError::Unavailable)
        }
    }

    pub fn start(&self) -> Result<(), TunnelError> {
        self.unit_command("start")
    }

    pub fn stop(&self) -> Result<(), TunnelError> {
        self.unit_command("stop")
    }

    pub fn reload(&self) -> Result<(), TunnelError> {
        self.unit_command("reload")
    }

    pub fn status(&self) -> Result<TunnelSnapshot, TunnelError> {
        #[cfg(target_os = "android")]
        {
            return self
                .handle
                .run_mobile_plugin("status", ())
                .map_err(|err| TunnelError::Plugin(err.to_string()));
        }
        #[cfg(not(target_os = "android"))]
        {
            Err(TunnelError::Unavailable)
        }
    }

    /// Directory both the host and the `:tunnel` process can read.
    pub fn shared_dir(&self) -> Result<std::path::PathBuf, TunnelError> {
        #[cfg(target_os = "android")]
        {
            let response: DirResponse = self
                .handle
                .run_mobile_plugin("shared_dir", ())
                .map_err(|err| TunnelError::Plugin(err.to_string()))?;
            return Ok(std::path::PathBuf::from(response.path));
        }
        #[cfg(not(target_os = "android"))]
        {
            Err(TunnelError::Unavailable)
        }
    }

    pub fn memory(&self) -> Result<Option<u64>, TunnelError> {
        #[cfg(target_os = "android")]
        {
            let response: MemoryResponse = self
                .handle
                .run_mobile_plugin("memory", ())
                .map_err(|err| TunnelError::Plugin(err.to_string()))?;
            return Ok(response.bytes);
        }
        #[cfg(not(target_os = "android"))]
        {
            Err(TunnelError::Unavailable)
        }
    }

    /// Battery-optimization, Private DNS, and always-on VPN flags.
    /// Android only; other targets return [`TunnelError::Unavailable`].
    pub fn device_status(&self) -> Result<DeviceStatus, TunnelError> {
        #[cfg(target_os = "android")]
        {
            return self
                .handle
                .run_mobile_plugin("device_status", ())
                .map_err(|err| TunnelError::Plugin(err.to_string()));
        }
        #[cfg(not(target_os = "android"))]
        {
            Err(TunnelError::Unavailable)
        }
    }

    pub fn request_battery_exemption(&self) -> Result<(), TunnelError> {
        self.unit_command("request_battery_exemption")
    }

    pub fn open_network_settings(&self) -> Result<(), TunnelError> {
        self.unit_command("open_network_settings")
    }

    pub fn open_vpn_settings(&self) -> Result<(), TunnelError> {
        self.unit_command("open_vpn_settings")
    }

    /// Open an already-validated https URL in the system browser.
    pub fn open_https_url(&self, url: &str) -> Result<(), TunnelError> {
        #[cfg(target_os = "android")]
        {
            let _: serde_json::Value = self
                .handle
                .run_mobile_plugin("open_https_url", UrlRequest { url })
                .map_err(|err| TunnelError::Plugin(err.to_string()))?;
            return Ok(());
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = url;
            Err(TunnelError::Unavailable)
        }
    }

    /// Hand a downloaded APK to the system installer. The path must be the
    /// file this app just wrote under its private updates directory.
    pub fn install_local_apk(&self, path: &std::path::Path) -> Result<(), TunnelError> {
        #[cfg(target_os = "android")]
        {
            let path = path.to_string_lossy();
            let _: serde_json::Value = self
                .handle
                .run_mobile_plugin("install_apk", PathRequest { path: &path })
                .map_err(|err| TunnelError::Plugin(err.to_string()))?;
            return Ok(());
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = path;
            Err(TunnelError::Unavailable)
        }
    }

    fn unit_command(&self, command: &str) -> Result<(), TunnelError> {
        #[cfg(target_os = "android")]
        {
            let _: serde_json::Value = self
                .handle
                .run_mobile_plugin(command, ())
                .map_err(|err| TunnelError::Plugin(err.to_string()))?;
            return Ok(());
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = command;
            Err(TunnelError::Unavailable)
        }
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("tunnel")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            {
                let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "TunnelPlugin")?;
                app.manage(Tunnel { handle });
            }
            #[cfg(not(target_os = "android"))]
            {
                let _ = api;
                app.manage(Tunnel::<R> {
                    _marker: std::marker::PhantomData,
                });
            }
            Ok(())
        })
        .build()
}
