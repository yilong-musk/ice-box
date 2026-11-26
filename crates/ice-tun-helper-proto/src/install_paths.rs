// SPDX-License-Identifier: GPL-3.0-or-later

//! Fixed paths and env keys of the privileged helper installation.
//!
//! Shared contract between the installer modes of `crates/ice-helper`
//! (which perform the privileged mutations) and the app-side installer in
//! `apps/desktop/src-tauri` (which locates the uninstall binary). The daemon
//! itself reads the same env keys from the launchd plist (macOS) or systemd
//! unit (Linux); see `crates/ice-helper/src/install.rs` for the full flow.

/// Root-owned helper binary destination.
#[cfg(not(target_os = "linux"))]
pub const HELPER_BIN_DEST: &str = "/Library/PrivilegedHelperTools/com.yilong-musk.icebox.helper";
/// Root-owned helper binary destination (Linux FHS, separate from dpkg `/usr/lib`).
#[cfg(target_os = "linux")]
pub const HELPER_BIN_DEST: &str = "/usr/local/libexec/ice-box/ice-helper";

/// Launchd plist destination (macOS) or systemd unit destination (Linux).
#[cfg(not(target_os = "linux"))]
pub const PLIST_DEST: &str = "/Library/LaunchDaemons/com.yilong-musk.icebox.helper.plist";
#[cfg(target_os = "linux")]
pub const PLIST_DEST: &str = "/etc/systemd/system/ice-box-helper.service";

/// Linux systemd service unit name and path.
pub const SYSTEMD_UNIT_NAME: &str = "ice-box-helper.service";
pub const SYSTEMD_UNIT_DEST: &str = "/etc/systemd/system/ice-box-helper.service";
/// Linux root-owned copy of bundled resources (`geoip/*.srs`) so AppImage
/// ephemeral `/tmp/.mount_*` paths are never relied upon after exit.
pub const RESOURCES_DEST_DIR: &str = "/usr/local/libexec/ice-box/resources";

/// Well-known helper socket path.
#[cfg(not(target_os = "linux"))]
pub const SOCKET_PATH: &str = "/var/run/ice-box-helper.sock";
#[cfg(target_os = "linux")]
pub const SOCKET_PATH: &str = "/run/ice-box-helper.sock";

/// Root-owned directory holding the bundled core.
#[cfg(not(target_os = "linux"))]
pub const CORE_BIN_DEST_DIR: &str = "/Library/PrivilegedHelperTools/com.yilong-musk.icebox";
#[cfg(target_os = "linux")]
pub const CORE_BIN_DEST_DIR: &str = "/usr/local/libexec/ice-box";

/// Root-owned core binary destination (pinned SHA-256 in the service config).
#[cfg(not(target_os = "linux"))]
pub const CORE_BIN_DEST: &str = "/Library/PrivilegedHelperTools/com.yilong-musk.icebox/sing-box";
#[cfg(target_os = "linux")]
pub const CORE_BIN_DEST: &str = "/usr/local/libexec/ice-box/sing-box";

/// Root-owned directory for the sanitised runtime config the helper starts.
#[cfg(not(target_os = "linux"))]
pub const CORE_RUN_DIR: &str = "/Library/PrivilegedHelperTools/com.yilong-musk.icebox/run";
#[cfg(target_os = "linux")]
pub const CORE_RUN_DIR: &str = "/usr/local/libexec/ice-box/run";

/// Root-owned core log (the elevated core's stdout/stderr).
pub const CORE_LOG_DEST: &str = "/var/log/ice-box-core.log";
/// Root-owned helper daemon log.
pub const HELPER_LOG_DEST: &str = "/var/log/ice-box-helper.log";
/// launchd label of the helper daemon (macOS).
pub const LAUNCHD_LABEL: &str = "com.yilong-musk.icebox.helper";
/// Token file name inside the app data dir (read by the app-side client).
pub const TOKEN_FILE_NAME: &str = "helper-token";

/// Env keys the installer writes into the launchd plist / systemd unit; the
/// daemon reads exactly these.
pub const ENV_TOKEN: &str = "ICE_HELPER_TOKEN";
pub const ENV_DATA_DIR: &str = "ICE_HELPER_DATA_DIR";
pub const ENV_CORE_BIN: &str = "ICE_HELPER_CORE_BIN";
pub const ENV_CORE_BIN_SHA256: &str = "ICE_HELPER_CORE_BIN_SHA256";
pub const ENV_CORE_LOG: &str = "ICE_HELPER_CORE_LOG";
pub const ENV_ALLOWED_UID: &str = "ICE_HELPER_ALLOWED_UID";
pub const ENV_SOCKET: &str = "ICE_BOX_TUN_HELPER_SOCKET";
/// Bundled resources directory (geoip `.srs` files). Recorded at install;
/// `Start` sanitises `rule_set` paths against it.
pub const ENV_RESOURCES_DIR: &str = "ICE_HELPER_RESOURCES_DIR";
