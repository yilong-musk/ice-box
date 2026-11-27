// SPDX-License-Identifier: GPL-3.0-or-later

//! Stable CLI exit codes for Agent and automation callers.

use ice_runtime::mcp::BridgeError;

/// Command succeeded.
pub const EXIT_OK: i32 = 0;
/// Application or runtime error returned by the service.
pub const EXIT_ERROR: i32 = 1;
/// Invalid CLI arguments, usage error, or malformed JSON input.
pub const EXIT_USAGE: i32 = 2;
/// Local control service / daemon is not running or unreachable.
pub const EXIT_NOT_RUNNING: i32 = 3;
/// Control token rejected or endpoint configuration malformed.
pub const EXIT_AUTH: i32 = 4;
/// Privileged helper / administrator elevation required.
pub const EXIT_PERMISSION: i32 = 5;
/// Deadline exceeded while waiting for readiness or RPC response.
pub const EXIT_TIMEOUT: i32 = 6;

/// Map a structured [`BridgeError`] to a deterministic process exit code.
pub fn exit_code_for_bridge_error(err: &BridgeError) -> i32 {
    match err {
        BridgeError::NotRunning(_) => EXIT_NOT_RUNNING,
        BridgeError::AuthOrConfig(_) => EXIT_AUTH,
        BridgeError::Transport(msg) => {
            let lower = msg.to_ascii_lowercase();
            if lower.contains("timed out") || lower.contains("timeout") {
                EXIT_TIMEOUT
            } else {
                EXIT_ERROR
            }
        }
        BridgeError::ToolError { code, .. } => exit_code_for_error_code(code),
    }
}

/// Map an `ice-box` application error code string onto a CLI exit code.
pub fn exit_code_for_error_code(code: &str) -> i32 {
    match code {
        "tun.permission_required"
        | "tun.helper_install_failed"
        | "tun.helper_install_cancelled"
        | "tun.elevation_cancelled"
        | "tun.elevation_requires_admin" => EXIT_PERMISSION,
        "core.health_timeout" | "sub.fetch_timeout" => EXIT_TIMEOUT,
        _ => EXIT_ERROR,
    }
}
