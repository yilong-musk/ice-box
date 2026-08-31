//! Fixed paths and env keys of the privileged helper installation.
//!
//! Shared contract between the installer modes of `crates/ice-helper`
//! (which perform the privileged mutations) and the app-side installer in
//! `apps/desktop/src-tauri` (which locates the uninstall binary). The daemon
//! itself reads the same env keys from the launchd plist; see
//! `crates/ice-helper/src/install.rs` for the full install flow.

/// Root-owned helper binary destination.
pub const HELPER_BIN_DEST: &str = "/Library/PrivilegedHelperTools/com.yilong-musk.icebox.helper";
/// Launchd plist destination.
pub const PLIST_DEST: &str = "/Library/LaunchDaemons/com.yilong-musk.icebox.helper.plist";
/// Well-known helper socket path.
pub const SOCKET_PATH: &str = "/var/run/ice-box-helper.sock";
/// Root-owned directory holding the bundled core.
pub const CORE_BIN_DEST_DIR: &str = "/Library/PrivilegedHelperTools/com.yilong-musk.icebox";
/// Root-owned core binary destination (pinned SHA-256 in the plist).
pub const CORE_BIN_DEST: &str = "/Library/PrivilegedHelperTools/com.yilong-musk.icebox/sing-box";
/// Root-owned core log (the elevated core's stdout/stderr).
pub const CORE_LOG_DEST: &str = "/var/log/ice-box-core.log";
/// Root-owned helper daemon log.
pub const HELPER_LOG_DEST: &str = "/var/log/ice-box-helper.log";
/// launchd label of the helper daemon.
pub const LAUNCHD_LABEL: &str = "com.yilong-musk.icebox.helper";
/// Token file name inside the app data dir (read by the app-side client).
pub const TOKEN_FILE_NAME: &str = "helper-token";

/// Env keys the installer writes into the launchd plist; the daemon reads
/// exactly these.
pub const ENV_TOKEN: &str = "ICE_HELPER_TOKEN";
pub const ENV_DATA_DIR: &str = "ICE_HELPER_DATA_DIR";
pub const ENV_CORE_BIN: &str = "ICE_HELPER_CORE_BIN";
pub const ENV_CORE_BIN_SHA256: &str = "ICE_HELPER_CORE_BIN_SHA256";
pub const ENV_CORE_LOG: &str = "ICE_HELPER_CORE_LOG";
pub const ENV_ALLOWED_UID: &str = "ICE_HELPER_ALLOWED_UID";
pub const ENV_SOCKET: &str = "ICE_BOX_TUN_HELPER_SOCKET";
