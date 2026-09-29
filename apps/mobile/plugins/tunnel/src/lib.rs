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
