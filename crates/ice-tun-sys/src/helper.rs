//! App-side client for the privileged helper daemon (plan §5 T5).
//!
//! [`HelperCoreCoordinator`] implements [`CoreCoordinator`] over the helper
//! wire protocol ([`crate::helper_protocol`]): the macOS production path
//! runs the bundled core elevated inside the installed launchd helper instead
//! of the dev `sudo` runner. The client is deliberately dumb — it connects,
//! sends one frame, reads one frame, and maps the result. All policy
//! (path allowlisting, peer identity, command set) lives in the daemon.
//!
//! Unreachable / unauthorized helpers fail closed with
//! `tun.permission_required` before any OS mutation; `create_backend`
//! probes the helper once at construction and falls back to the fail-closed
//! [`crate::coordinator::DeferredCoreCoordinator`] when it is absent.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::coordinator::CoreCoordinator;
use crate::error::{TunError, TunErrorCode};
use crate::helper_protocol::{
    decode_response, encode_request, validate_config_path, HelperCommand, HelperRequest,
    HelperResponse, DEFAULT_SOCKET_PATH,
};

/// Env override for the helper socket path (dev runners, tests, and the
/// live acceptance script point at a non-default location).
pub const ENV_HELPER_SOCKET: &str = "ICE_BOX_TUN_HELPER_SOCKET";
/// Env override for the per-installation token (dev / acceptance). The
/// production client reads the token file the installer wrote into the app
/// data dir (`helper-token`, mode 0644, root-owned).
pub const ENV_HELPER_TOKEN: &str = "ICE_BOX_TUN_HELPER_TOKEN";
/// Token file name inside the app data dir, written by the installer.
pub const HELPER_TOKEN_FILE: &str = "helper-token";
/// Bounded connect / read / write timeouts so a dead helper fails fast
/// instead of hanging a capture transition.
const IPC_TIMEOUT: Duration = Duration::from_secs(3);
/// Short read bound for UI status probes: a dead-but-present daemon must
/// never stall a 2 s status poll.
const STATUS_PROBE_TIMEOUT: Duration = Duration::from_millis(200);

/// Resolve the helper socket path: `ICE_BOX_TUN_HELPER_SOCKET` when set,
/// otherwise the well-known path the daemon binds.
pub fn helper_socket_path() -> PathBuf {
    std::env::var(ENV_HELPER_SOCKET)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_SOCKET_PATH))
}

/// Resolve the per-installation token: `ICE_BOX_TUN_HELPER_TOKEN` when set,
/// otherwise `<data_dir>/helper-token` (written by the installer). A missing
/// token is a `permission_required` outcome — the helper was never installed
/// or authorized.
pub fn helper_token(data_dir: &Path) -> Result<String, TunError> {
    if let Ok(token) = std::env::var(ENV_HELPER_TOKEN) {
        if !token.is_empty() {
            return Ok(token);
        }
    }
    let token_file = data_dir.join(HELPER_TOKEN_FILE);
    let raw = std::fs::read_to_string(&token_file).map_err(|e| {
        TunError::new(
            TunErrorCode::PermissionRequired,
            format!(
                "privileged helper not installed or not authorized (no {}): {e}",
                token_file.display()
            ),
        )
    })?;
    let token = raw.trim();
    if token.is_empty() {
        return Err(TunError::new(
            TunErrorCode::PermissionRequired,
            format!("helper token file is empty: {}", token_file.display()),
        ));
    }
    Ok(token.to_string())
}

/// One request/response roundtrip over a fresh connection. The daemon
/// handles one frame per connection, so the client reconnects per command.
pub fn roundtrip(socket: &Path, request: &HelperRequest) -> Result<HelperResponse, TunError> {
    roundtrip_with_timeout(socket, request, IPC_TIMEOUT)
}

/// [`roundtrip`] with an explicit I/O bound. The short-bound variant is for
/// read-only UI status probes.
pub fn roundtrip_with_timeout(
    socket: &Path,
    request: &HelperRequest,
    timeout: Duration,
) -> Result<HelperResponse, TunError> {
    let stream = UnixStream::connect(socket).map_err(|e| {
        TunError::new(
            TunErrorCode::PermissionRequired,
            format!(
                "privileged helper unreachable at {}: {e} (install and authorize the helper, or use the dev sudo path)",
                socket.display()
            )
        )
    })?;
    stream.set_read_timeout(Some(timeout)).ok();
    stream.set_write_timeout(Some(timeout)).ok();

    let mut writer = stream.try_clone().map_err(|e| {
        TunError::new(
            TunErrorCode::ApplyFailed,
            format!("clone helper socket: {e}"),
        )
    })?;
    let mut line = encode_request(request)?;
    line.push(b'\n');
    writer.write_all(&line).map_err(|e| {
        TunError::new(
            TunErrorCode::ApplyFailed,
            format!("write helper request: {e}"),
        )
    })?;
    writer.flush().ok();

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response).map_err(|e| {
        TunError::new(
            TunErrorCode::ApplyFailed,
            format!("read helper response: {e}"),
        )
    })?;
    decode_response(response.as_bytes())
}

/// [`CoreCoordinator`] backed by the privileged helper daemon.
pub struct HelperCoreCoordinator {
    socket: PathBuf,
    token: String,
    data_dir: PathBuf,
}

impl HelperCoreCoordinator {
    /// `data_dir` is the app data root; the daemon must have been installed
    /// with that same directory so its path allowlist matches.
    pub fn new(socket: PathBuf, token: String, data_dir: PathBuf) -> Self {
        Self {
            socket,
            token,
            data_dir,
        }
    }

    /// Create the coordinator from the environment (dev/acceptance) or the
    /// installed token file; fails with `permission_required` when the
    /// helper is absent.
    pub fn from_data_dir(data_dir: &Path) -> Result<Self, TunError> {
        Ok(Self {
            socket: helper_socket_path(),
            token: helper_token(data_dir)?,
            data_dir: data_dir.to_path_buf(),
        })
    }

    fn request(&self, command: HelperCommand) -> Result<HelperResponse, TunError> {
        roundtrip(
            &self.socket,
            &HelperRequest {
                v: crate::helper_protocol::PROTOCOL_VERSION,
                token: self.token.clone(),
                command,
            },
        )
    }
}

impl CoreCoordinator for HelperCoreCoordinator {
    fn start_with_config(&mut self, config_path: &Path) -> Result<u32, TunError> {
        // Local allowlist preflight (the daemon re-validates; this fails fast
        // with a clearer error before any IPC).
        let _canonical = validate_config_path(&self.data_dir, &config_path.to_string_lossy())?;
        let response = self.request(HelperCommand::Start {
            config: config_path.to_string_lossy().into_owned(),
        })?;
        match &response {
            HelperResponse {
                ok: true,
                pid: Some(pid),
                ..
            } => Ok(*pid),
            HelperResponse {
                ok: true,
                pid: None,
                ..
            } => Err(TunError::new(
                TunErrorCode::ApplyFailed,
                "helper started the core but returned no pid",
            )),
            HelperResponse { ok: false, .. } => Err(response.into_error().unwrap_or_else(|| {
                TunError::new(TunErrorCode::ApplyFailed, "helper rejected the start")
            })),
        }
    }

    fn stop(&mut self) -> Result<(), TunError> {
        let response = self.request(HelperCommand::Stop)?;
        match response.into_error() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    fn set_dns(&mut self, service: &str, servers: &[String]) -> Result<(), TunError> {
        let response = self.request(HelperCommand::SetDns {
            service: service.to_string(),
            servers: servers.to_vec(),
        })?;
        match response.into_error() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}

/// Probe whether an authorized helper is reachable right now. Read-only:
/// connects and sends a `Status` frame; never mutates the OS. Used by
/// `create_backend` to pick the helper coordinator over the fail-closed
/// deferred one.
pub fn helper_reachable(socket: &Path, token: &str) -> bool {
    let request = HelperRequest {
        v: crate::helper_protocol::PROTOCOL_VERSION,
        token: token.to_string(),
        command: HelperCommand::Status,
    };
    match roundtrip(socket, &request) {
        Ok(response) => response.ok,
        Err(_) => false,
    }
}

/// UI-status probe: the same read-only `Status` roundtrip with a short
/// timeout so a dead-but-present daemon fails fast (status is polled every
/// ~2 s). Returns false when the helper is absent, unauthorized, or stuck.
pub fn helper_reachable_bounded(socket: &Path, token: &str) -> bool {
    let request = HelperRequest {
        v: crate::helper_protocol::PROTOCOL_VERSION,
        token: token.to_string(),
        command: HelperCommand::Status,
    };
    match roundtrip_with_timeout(socket, &request, STATUS_PROBE_TIMEOUT) {
        Ok(response) => response.ok,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_protocol::PROTOCOL_VERSION;

    /// Tests that read/modify process env must run serialized.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn token_from_env_overrides_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(ENV_HELPER_TOKEN, "env-token");
        let dir = std::env::temp_dir();
        assert_eq!(helper_token(&dir).unwrap(), "env-token");
        std::env::remove_var(ENV_HELPER_TOKEN);
    }

    #[test]
    fn token_from_file_is_trimmed() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(ENV_HELPER_TOKEN);
        let dir = std::env::temp_dir().join(format!(
            "ice-helper-token-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(HELPER_TOKEN_FILE), "  file-token\n").unwrap();
        assert_eq!(helper_token(&dir).unwrap(), "file-token");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_token_is_permission_required() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(ENV_HELPER_TOKEN);
        let dir = std::env::temp_dir().join(format!(
            "ice-helper-notoken-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let err = helper_token(&dir).unwrap_err();
        assert_eq!(err.code, TunErrorCode::PermissionRequired);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unreachable_socket_reports_permission_required() {
        let request = HelperRequest {
            v: PROTOCOL_VERSION,
            token: "t".into(),
            command: HelperCommand::Status,
        };
        let err = roundtrip(Path::new("/nonexistent/ice-box-helper.sock"), &request).unwrap_err();
        assert_eq!(err.code, TunErrorCode::PermissionRequired);
        assert!(!helper_reachable(
            Path::new("/nonexistent/ice-box-helper.sock"),
            "t"
        ));
    }
}
