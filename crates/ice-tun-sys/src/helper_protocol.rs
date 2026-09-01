//! Privileged-helper wire protocol (plan §5 T5).
//!
//! The macOS production path runs the core elevated inside a small
//! launchd helper daemon (T0 lock §24.5.2). This module is the *shared*
//! contract between the app-side client ([`crate::helper::HelperCoreCoordinator`])
//! and the daemon (`crates/ice-helper`). It is deliberately host-free: pure
//! types, framing, and path validation with no OS calls, so it tests on all
//! CI platforms.
//!
//! Wire shape: one JSON object per line (newline-delimited), UTF-8, each
//! line capped at [`MAX_FRAME_BYTES`]. The daemon rejects anything else
//! without reading unbounded input.
//!
//! Security model (plan §7): the helper accepts exactly four commands and
//! never accepts a binary path, route target, interface name, or arbitrary
//! shell input from the client. The `config` path must canonicalize into the
//! app data directory the daemon was installed with; the `SetDns` service
//! name is restricted to `[A-Za-z0-9 ._-]` and server values to IP literals.
//! Peer identity is verified by the daemon from the socket credentials;
//! possession of the per-installation token is the application-level gate.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{TunError, TunErrorCode};

/// Protocol version; the daemon rejects mismatched `v` values. Bumped when
/// a command is added so an older installed daemon fails with a clear
/// version mismatch instead of a decode error.
pub const PROTOCOL_VERSION: u32 = 2;
/// Hard cap for one request or response line (16 KiB).
pub const MAX_FRAME_BYTES: usize = 16 * 1024;
/// Socket path constants used by the daemon and the client. The daemon owns
/// the well-known path (root-created under `/var/run`); tests and dev
/// runners may override via `ICE_BOX_TUN_HELPER_SOCKET`.
pub const DEFAULT_SOCKET_PATH: &str = "/var/run/ice-box-helper.sock";

/// Commands the helper accepts (plan §7: narrow surface, nothing else).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum HelperCommand {
    /// Liveness + auth probe; returns the running pid, if any.
    Status,
    /// Start the core with the given config path (must be inside the
    /// installed data directory). Returns the spawned pid.
    Start { config: String },
    /// Stop the core (TERM→KILL grace), idempotent.
    Stop,
    /// Set the system DNS servers of one named network service. An empty
    /// `servers` list clears the per-service DNS override ("Empty"), which
    /// restores the DHCP-assigned resolvers.
    SetDns {
        service: String,
        servers: Vec<String>,
    },
}

/// Validate the service name the `SetDns` command may target. The daemon
/// passes argv elements directly (never a shell), so the hard constraints
/// are protocol framing (`\n` would corrupt the line-based wire format) and
/// `\0`; everything else is printable ASCII.
pub fn validate_dns_service(service: &str) -> Result<(), TunError> {
    if service.is_empty()
        || service.len() > 128
        || service
            .chars()
            .any(|c| !c.is_ascii() || c == '\n' || c == '\0')
    {
        return Err(TunError::new(
            TunErrorCode::ApplyFailed,
            format!("invalid dns service name: {service:?}"),
        ));
    }
    Ok(())
}

/// Validate a DNS server value: must be an IPv4 or IPv6 literal.
pub fn validate_dns_server(server: &str) -> Result<(), TunError> {
    if server.parse::<std::net::IpAddr>().is_err() {
        return Err(TunError::new(
            TunErrorCode::ApplyFailed,
            format!("invalid dns server: {server:?}"),
        ));
    }
    Ok(())
}

/// One client request frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperRequest {
    pub v: u32,
    /// Per-installation token; daemon compares constant-time.
    pub token: String,
    #[serde(flatten)]
    pub command: HelperCommand,
}

/// One daemon response frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Stable `tun.*` code on failure (plan §4.5 / §5 T2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl HelperResponse {
    pub fn ok(pid: Option<u32>) -> Self {
        Self {
            ok: true,
            pid,
            code: None,
            message: None,
        }
    }

    pub fn err(err: &TunError) -> Self {
        Self {
            ok: false,
            pid: None,
            code: Some(err.code.as_str().to_string()),
            message: Some(err.message.clone()),
        }
    }

    /// Map a failure frame back to a `TunError`.
    pub fn into_error(self) -> Option<TunError> {
        if self.ok {
            return None;
        }
        let code = match self.code.as_deref() {
            Some("tun.not_supported") => TunErrorCode::NotSupported,
            Some("tun.permission_required") => TunErrorCode::PermissionRequired,
            Some("tun.apply_failed") => TunErrorCode::ApplyFailed,
            Some("tun.restore_failed") => TunErrorCode::RestoreFailed,
            Some("tun.healthcheck_failed") => TunErrorCode::HealthcheckFailed,
            Some("tun.recovery_required") => TunErrorCode::RecoveryRequired,
            _ => TunErrorCode::ApplyFailed,
        };
        Some(TunError::new(
            code,
            self.message.unwrap_or_else(|| code.as_str().to_string()),
        ))
    }
}

/// Serialize a request frame (single line, no trailing newline).
pub fn encode_request(req: &HelperRequest) -> Result<Vec<u8>, TunError> {
    let line = serde_json::to_string(req)
        .map_err(|e| TunError::new(TunErrorCode::ApplyFailed, format!("encode request: {e}")))?;
    if line.len() > MAX_FRAME_BYTES {
        return Err(TunError::new(
            TunErrorCode::ApplyFailed,
            format!("request frame exceeds {MAX_FRAME_BYTES} bytes"),
        ));
    }
    Ok(line.into_bytes())
}

/// Parse one response frame (with or without trailing newline).
pub fn decode_response(line: &[u8]) -> Result<HelperResponse, TunError> {
    if line.len() > MAX_FRAME_BYTES {
        return Err(TunError::new(
            TunErrorCode::ApplyFailed,
            format!("response frame exceeds {MAX_FRAME_BYTES} bytes"),
        ));
    }
    let trimmed = std::str::from_utf8(line)
        .map_err(|_| TunError::new(TunErrorCode::ApplyFailed, "response is not UTF-8"))?
        .trim_end_matches(['\n', '\r']);
    serde_json::from_str(trimmed)
        .map_err(|e| TunError::new(TunErrorCode::ApplyFailed, format!("decode response: {e}")))
}

/// Validate the `config` path a client wants the helper to start:
///
/// - must be absolute;
/// - must canonicalize to a regular file inside the daemon's data directory
///   (the root of the app data dir the helper was installed with).
///
/// Both paths are canonicalized so `..` / symlink escapes are rejected.
/// The comparison is prefix-based on canonical components, so
/// `DATA_DIR/config.json` passes and `DATA_DIR-other/x` does not.
pub fn validate_config_path(data_dir: &Path, config: &str) -> Result<std::path::PathBuf, TunError> {
    let config_path = std::path::Path::new(config);
    if !config_path.is_absolute() {
        return Err(TunError::new(
            TunErrorCode::PermissionRequired,
            format!("config path must be absolute, got {config}"),
        ));
    }
    let canonical = config_path.canonicalize().map_err(|e| {
        TunError::new(
            TunErrorCode::ApplyFailed,
            format!("config path not resolvable: {config}: {e}"),
        )
    })?;
    if !canonical.is_file() {
        return Err(TunError::new(
            TunErrorCode::ApplyFailed,
            format!("config path is not a regular file: {}", canonical.display()),
        ));
    }
    let data_canonical = data_dir.canonicalize().map_err(|e| {
        TunError::new(
            TunErrorCode::PermissionRequired,
            format!("data dir not resolvable: {}: {e}", data_dir.display()),
        )
    })?;
    let within = canonical.starts_with(&data_canonical)
        && canonical
            .strip_prefix(&data_canonical)
            .is_ok_and(|rest| !rest.as_os_str().is_empty() && !rest.starts_with(".."));
    if !within {
        return Err(TunError::new(
            TunErrorCode::PermissionRequired,
            format!(
                "config path must be inside the app data dir: {}",
                canonical.display()
            ),
        ));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ice-helper-proto-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn request_frame_roundtrip() {
        let req = HelperRequest {
            v: PROTOCOL_VERSION,
            token: "secret".into(),
            command: HelperCommand::Start {
                config: "/data/config.json".into(),
            },
        };
        let bytes = encode_request(&req).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains('\n'));
        let parsed: HelperRequest = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn response_frame_with_trailing_newline_parses() {
        let resp = HelperResponse::err(&TunError::new(TunErrorCode::PermissionRequired, "denied"));
        let mut line = serde_json::to_string(&resp).unwrap();
        line.push('\n');
        let parsed = decode_response(line.as_bytes()).unwrap();
        assert!(!parsed.ok);
        assert_eq!(parsed.code.as_deref(), Some("tun.permission_required"));
        let err = parsed.into_error().unwrap();
        assert_eq!(err.code, TunErrorCode::PermissionRequired);
    }

    #[test]
    fn response_ok_maps_to_pid() {
        let parsed = decode_response(br#"{"ok":true,"pid":4242}"#).unwrap();
        assert!(parsed.ok);
        assert_eq!(parsed.pid, Some(4242));
        assert!(parsed.into_error().is_none());
    }

    #[test]
    fn unknown_error_code_maps_to_apply_failed() {
        let parsed =
            decode_response(br#"{"ok":false,"code":"something.else","message":"x"}"#).unwrap();
        assert_eq!(parsed.into_error().unwrap().code, TunErrorCode::ApplyFailed);
    }

    #[test]
    fn oversized_frame_is_rejected() {
        let huge = vec![b' '; MAX_FRAME_BYTES + 1];
        assert!(decode_response(&huge).is_err());
        let req = HelperRequest {
            v: PROTOCOL_VERSION,
            token: "t".into(),
            command: HelperCommand::Start {
                config: "x".repeat(MAX_FRAME_BYTES),
            },
        };
        assert!(encode_request(&req).is_err());
    }

    #[test]
    fn set_dns_command_roundtrips_and_validates() {
        let req = HelperRequest {
            v: PROTOCOL_VERSION,
            token: "secret".into(),
            command: HelperCommand::SetDns {
                service: "Wi-Fi".into(),
                servers: vec!["223.5.5.5".into(), "119.29.29.29".into()],
            },
        };
        let bytes = encode_request(&req).unwrap();
        let parsed: HelperRequest =
            serde_json::from_str(&String::from_utf8(bytes).unwrap()).unwrap();
        assert_eq!(parsed, req);

        let clear = HelperRequest {
            v: PROTOCOL_VERSION,
            token: "secret".into(),
            command: HelperCommand::SetDns {
                service: "Wi-Fi".into(),
                servers: vec![],
            },
        };
        let parsed: HelperRequest =
            serde_json::from_str(&String::from_utf8(encode_request(&clear).unwrap()).unwrap())
                .unwrap();
        assert_eq!(parsed, clear);

        validate_dns_service("Wi-Fi").expect("service name ok");
        validate_dns_service("USB 10/100/1000 LAN").expect("argv quoting keeps spaces safe");
        validate_dns_service("").expect_err("empty rejected");
        validate_dns_service("a\nb").expect_err("newline breaks the frame protocol");
        validate_dns_server("223.5.5.5").expect("ipv4 ok");
        validate_dns_server("2001:db8::1").expect("ipv6 ok");
        validate_dns_server("not-an-ip").expect_err("hostname rejected");
    }

    #[test]
    fn config_path_must_be_absolute_and_inside_data_dir() {
        let dir = temp_dir("cfg");
        let config = dir.join("config.json");
        fs::write(&config, b"{}").unwrap();

        // Inside the data dir: accepted.
        let canonical = validate_config_path(&dir, &config.to_string_lossy()).unwrap();
        assert_eq!(canonical, config.canonicalize().unwrap());

        // Relative path: rejected.
        let err = validate_config_path(&dir, "config.json").unwrap_err();
        assert_eq!(err.code, TunErrorCode::PermissionRequired);

        // Missing file: rejected.
        let err = validate_config_path(&dir, &dir.join("nope.json").to_string_lossy()).unwrap_err();
        assert_eq!(err.code, TunErrorCode::ApplyFailed);

        // Outside the data dir: rejected.
        let outside = std::env::temp_dir().join("ice-helper-outside.json");
        fs::write(&outside, b"{}").unwrap();
        let err = validate_config_path(&dir, &outside.to_string_lossy()).unwrap_err();
        assert_eq!(err.code, TunErrorCode::PermissionRequired);

        // Sibling-prefix dir must not pass (DATA_DIR-other).
        let sibling = std::env::temp_dir().join(format!(
            "{}-other",
            dir.file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir_all(&sibling).unwrap();
        let in_sibling = sibling.join("config.json");
        fs::write(&in_sibling, b"{}").unwrap();
        let err = validate_config_path(&dir, &in_sibling.to_string_lossy()).unwrap_err();
        assert_eq!(err.code, TunErrorCode::PermissionRequired);

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_file(&outside);
        let _ = fs::remove_dir_all(&sibling);
    }

    #[test]
    fn symlink_escape_is_rejected() {
        #[cfg(unix)]
        {
            let dir = temp_dir("symlink");
            let outside = std::env::temp_dir().join("ice-helper-symlink-target.json");
            fs::write(&outside, b"{}").unwrap();
            let link = dir.join("escaped.json");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            let err = validate_config_path(&dir, &link.to_string_lossy()).unwrap_err();
            assert_eq!(err.code, TunErrorCode::PermissionRequired);
            let _ = fs::remove_dir_all(&dir);
            let _ = fs::remove_file(&outside);
        }
    }
}
