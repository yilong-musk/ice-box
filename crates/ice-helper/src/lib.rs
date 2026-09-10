// SPDX-License-Identifier: GPL-3.0-or-later

//! Privileged helper daemon core (`docs/tun.md`, macOS production path).
//!
//! The daemon runs as root under launchd. It owns a narrow privileged
//! surface: start the bundled sing-box with an allowlisted config path,
//! stop it (TERM→KILL with bounded grace), apply validated `SetDns`
//! updates via `networksetup`, and truncate the fixed core log in place.
//! sing-box owns the adapter / routes; the helper never accepts a binary
//! path, interface name, or shell string from the client.
//!
//! Security model:
//!
//! - One request frame per connection; the client reconnects per command.
//! - Peer identity: the socket's `getpeereid` uid must equal the authorized
//!   user (the uid the installer recorded). Everything else is rejected
//!   before the frame is read.
//! - The request must carry the per-installation token (constant-time
//!   compare) and protocol version 2.
//! - `Start` accepts a config path only when it canonicalizes inside the
//!   data directory the daemon was installed with. The JSON is then
//!   sanitised (`ice-config-guard`) and written to a root-owned run dir;
//!   sing-box is started from that copy, never from the user-writable file.
//! - The core binary path is fixed at install; the client never supplies it.
//! - `SetDns` is validated (`validate_set_dns`) before `networksetup` runs.
//! - `TruncateCoreLog` empties the daemon's own core log path (never a
//!   client-supplied path) so the app can shrink a running elevated
//!   core's output.
//!
//! The server logic is host-free (inject the peer uid and a fake core
//! binary), so the same code tests on Linux and macOS CI. On non-unix
//! platforms the crate builds as a stub so the workspace gate stays green
//! everywhere (the daemon is macOS-only).

#[cfg(unix)]
mod imp {
    use std::ffi::CString;
    use std::fs::OpenOptions;
    use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use ice_tun_helper_proto::{
        validate_config_path, validate_set_dns, HelperCommand, HelperRequest, HelperResponse,
        MAX_FRAME_BYTES, PROTOCOL_VERSION,
    };
    use ice_types::{ErrorCode, TunError};

    /// How long to wait for the elevated core to stay alive during startup
    /// (config/bind errors surface as an early exit) before accepting it.
    const STARTUP_LIVENESS_WAIT: Duration = Duration::from_millis(2000);
    const LIVENESS_POLL: Duration = Duration::from_millis(100);
    /// Bounded wait for the core to die after SIGTERM, then SIGKILL.
    const TERM_GRACE: Duration = Duration::from_secs(5);
    const KILL_GRACE: Duration = Duration::from_secs(2);
    /// Bounded time a peer may take to deliver its single request frame. A
    /// stalled or half-written frame must not hold a daemon thread for long
    /// (the daemon serves each connection on its own thread with a bounded
    /// concurrent-connection cap).
    const READ_TIMEOUT: Duration = Duration::from_secs(2);

    /// Cap the core log at 20 MiB by dropping the oldest 5 MiB (CORE-7).
    fn rotated_log_path(path: &std::path::Path, n: u32) -> std::path::PathBuf {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".{n}"));
        match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
            _ => std::path::PathBuf::from(name),
        }
    }

    fn cap_core_log(path: &std::path::Path) {
        cap_core_log_at(path, 20 * 1024 * 1024, 3);
    }

    fn cap_core_log_at(path: &std::path::Path, max_bytes: u64, keep: u32) {
        let oversized = std::fs::metadata(path)
            .map(|m| m.len() > max_bytes)
            .unwrap_or(false);
        if oversized {
            let _ = retain_log_tail(path, max_bytes);
        }
        for i in 1..=keep {
            let _ = std::fs::remove_file(rotated_log_path(path, i));
        }
    }

    /// Keep in sync with `ice_core::trim_log_file`.
    fn retain_log_tail(path: &std::path::Path, max_bytes: u64) -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        let len = file.metadata()?.len();
        let start = if len <= 1 {
            0
        } else {
            let drop = (max_bytes / 4).max(1);
            let over = len.saturating_sub(max_bytes);
            drop.max(over).min(len - 1)
        };
        let kept = if start == 0 || len <= 1 {
            file.seek(SeekFrom::Start(0))?;
            let mut all = Vec::new();
            file.read_to_end(&mut all)?;
            all
        } else {
            let at_line_start = {
                file.seek(SeekFrom::Start(start - 1))?;
                let mut prev = [0u8; 1];
                file.read_exact(&mut prev)?;
                prev[0] == b'\n'
            };
            file.seek(SeekFrom::Start(start))?;
            let mut tail = Vec::new();
            file.read_to_end(&mut tail)?;
            if !at_line_start {
                if let Some(i) = tail.iter().position(|&b| b == b'\n') {
                    if i + 1 < tail.len() {
                        tail.drain(..=i);
                    }
                }
            }
            tail
        };
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&kept)?;
        file.flush()?;
        Ok(())
    }

    /// Empty the core log in place so a running sing-box keeps writing to the
    /// same inode, and drop rotated siblings. The path is the daemon's own
    /// `core_log` (never client-supplied).
    fn truncate_core_log_file(path: &std::path::Path) -> Result<(), TunError> {
        const KEEP: u32 = 3;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    TunError::new(
                        ErrorCode::TunApplyFailed,
                        format!("create log dir {}: {e}", parent.display()),
                    )
                })?;
            }
        }
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|e| {
                TunError::new(
                    ErrorCode::TunApplyFailed,
                    format!("truncate core log {}: {e}", path.display()),
                )
            })?;
        for i in 1..=KEEP {
            let _ = std::fs::remove_file(rotated_log_path(path, i));
        }
        Ok(())
    }

    /// Give the authorized user ownership of the core log so the unelevated
    /// app can truncate it in place (the directory stays root-owned, so the
    /// user cannot replace the path with a symlink).
    fn chown_core_log_to_allowed_uid(path: &std::path::Path, uid: u32) {
        let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
            return;
        };
        let rc = unsafe { libc::chown(c_path.as_ptr(), uid, 0) };
        if rc != 0 {
            tracing::warn!(
                path = %path.display(),
                uid,
                error = %std::io::Error::last_os_error(),
                "chown core log to allowed uid failed"
            );
        }
    }

    /// Immutable daemon configuration, set by the installer.
    #[derive(Debug, Clone)]
    pub struct ServerConfig {
        /// Per-installation token (constant-time compared).
        pub token: String,
        /// App data dir; `Start` config paths must canonicalize inside it.
        pub data_dir: PathBuf,
        /// The bundled sing-box binary (fixed at install; never client-supplied).
        pub core_bin: PathBuf,
        /// Where the core's stdout/stderr go (append).
        pub core_log: PathBuf,
        /// Peer uid authorized to talk to the helper. `None` = accept any peer
        /// (dev/test only; the installer always sets it).
        pub allowed_uid: Option<u32>,
        /// Root-owned directory for the sanitised config the core actually loads.
        pub protected_run_dir: PathBuf,
        /// Bundled resources dir; `route.rule_set[].path` must canonicalise here.
        pub resources_dir: PathBuf,
    }

    /// Peer-identity probe. Production uses the real socket credential;
    /// tests inject a fixed uid so the dispatch logic stays host-free.
    pub trait PeerAuth: Sync {
        fn peer_uid(&self, stream: &UnixStream) -> Result<u32, TunError>;
    }

    /// Reads the peer uid from the socket (`getpeereid` on macOS; `SO_PEERCRED`
    /// on Linux). Used by `main`; tests inject a fixed uid.
    pub struct SocketPeerAuth;

    #[cfg(target_os = "macos")]
    impl PeerAuth for SocketPeerAuth {
        fn peer_uid(&self, stream: &UnixStream) -> Result<u32, TunError> {
            use std::os::unix::io::AsRawFd;
            let mut uid: libc::uid_t = 0;
            let mut gid: libc::gid_t = 0;
            let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
            if rc != 0 {
                return Err(TunError::new(
                    ErrorCode::TunPermissionRequired,
                    format!("getpeereid: {}", std::io::Error::last_os_error()),
                ));
            }
            Ok(uid)
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    impl PeerAuth for SocketPeerAuth {
        fn peer_uid(&self, stream: &UnixStream) -> Result<u32, TunError> {
            use std::os::unix::io::AsRawFd;
            let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
            let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
            let rc = unsafe {
                libc::getsockopt(
                    stream.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_PEERCRED,
                    &mut cred as *mut libc::ucred as *mut libc::c_void,
                    &mut len,
                )
            };
            if rc != 0 {
                return Err(TunError::new(
                    ErrorCode::TunPermissionRequired,
                    format!("SO_PEERCRED: {}", std::io::Error::last_os_error()),
                ));
            }
            Ok(cred.uid)
        }
    }

    /// Test-only peer auth with a fixed uid.
    #[cfg(any(test, feature = "test-hooks"))]
    pub struct FixedPeerAuth(pub u32);

    #[cfg(any(test, feature = "test-hooks"))]
    impl PeerAuth for FixedPeerAuth {
        fn peer_uid(&self, _stream: &UnixStream) -> Result<u32, TunError> {
            Ok(self.0)
        }
    }

    /// Constant-time token compare (SEC-7).
    ///
    /// Both sides are hashed to a fixed 32-byte SHA-256 digest before
    /// `subtle::ConstantTimeEq`, so a length mismatch cannot take an early
    /// return. The installer token is already 64 hex chars; hashing also
    /// covers tests that use shorter fixtures.
    fn constant_time_eq(a: &str, b: &str) -> bool {
        use sha2::{Digest, Sha256};
        use subtle::ConstantTimeEq;
        let ha = Sha256::digest(a.as_bytes());
        let hb = Sha256::digest(b.as_bytes());
        ha.ct_eq(&hb).into()
    }

    /// Authenticate a connection: peer uid (when configured) and request token.
    fn authenticate(
        config: &ServerConfig,
        peer_uid: u32,
        request: &HelperRequest,
    ) -> Result<(), TunError> {
        if let Some(allowed) = config.allowed_uid {
            if peer_uid != allowed {
                return Err(TunError::new(
                    ErrorCode::TunPermissionRequired,
                    format!("peer uid {peer_uid} is not the authorized user {allowed}"),
                ));
            }
        }
        if request.v != PROTOCOL_VERSION {
            return Err(TunError::new(
                ErrorCode::TunApplyFailed,
                format!(
                    "protocol version mismatch: client {}, daemon {PROTOCOL_VERSION}",
                    request.v
                ),
            ));
        }
        if !constant_time_eq(&config.token, &request.token) {
            return Err(TunError::new(
                ErrorCode::TunPermissionRequired,
                "invalid helper token",
            ));
        }
        Ok(())
    }

    fn read_helper_request(stream: &UnixStream) -> Result<HelperRequest, TunError> {
        // Bounded read: a peer that connects but never finishes its frame
        // must not hold a daemon thread for long.
        stream.set_read_timeout(Some(READ_TIMEOUT)).map_err(|e| {
            TunError::new(ErrorCode::TunApplyFailed, format!("set read timeout: {e}"))
        })?;
        let mut reader =
            BufReader::new(stream.try_clone().map_err(|e| {
                TunError::new(ErrorCode::TunApplyFailed, format!("clone stream: {e}"))
            })?);
        // Cap the request at MAX_FRAME_BYTES + 1 bytes: an oversized or
        // unterminated frame is rejected without buffering unbounded input
        // (the wire protocol caps every line at MAX_FRAME_BYTES).
        let mut line = String::new();
        let read = reader
            .by_ref()
            .take((MAX_FRAME_BYTES + 1) as u64)
            .read_line(&mut line)
            .map_err(|e| TunError::new(ErrorCode::TunApplyFailed, format!("read request: {e}")))?;
        if read == 0 {
            return Err(TunError::new(ErrorCode::TunApplyFailed, "empty request"));
        }
        if line.len() > MAX_FRAME_BYTES {
            return Err(TunError::new(
                ErrorCode::TunApplyFailed,
                format!("request frame exceeds {MAX_FRAME_BYTES} bytes"),
            ));
        }
        serde_json::from_str(line.trim_end())
            .map_err(|e| TunError::new(ErrorCode::TunApplyFailed, format!("decode request: {e}")))
    }

    fn write_helper_response(
        stream: &UnixStream,
        response: HelperResponse,
    ) -> Result<(), TunError> {
        let mut frame = serde_json::to_vec(&response).map_err(|e| {
            TunError::new(ErrorCode::TunApplyFailed, format!("encode response: {e}"))
        })?;
        if frame.len() > ice_tun_helper_proto::MAX_FRAME_BYTES {
            return Err(TunError::new(
                ErrorCode::TunApplyFailed,
                "response frame exceeds limit",
            ));
        }
        frame.push(b'\n');
        let mut writer = stream
            .try_clone()
            .map_err(|e| TunError::new(ErrorCode::TunApplyFailed, format!("clone stream: {e}")))?;
        writer.write_all(&frame).map_err(|e| {
            TunError::new(ErrorCode::TunApplyFailed, format!("write response: {e}"))
        })?;
        writer.flush().ok();
        Ok(())
    }

    /// One request per connection: read a frame, authenticate, dispatch, reply.
    /// The caller already holds the runner (tests). Production uses
    /// [`serve_peer`], which authenticates before taking the mutex.
    pub fn serve_connection(
        stream: UnixStream,
        config: &ServerConfig,
        peer_auth: &dyn PeerAuth,
        runner: &mut dyn CoreRunner,
    ) -> Result<(), TunError> {
        let peer_uid = peer_auth.peer_uid(&stream)?;
        let request = read_helper_request(&stream)?;
        let response = if let Err(err) = authenticate(config, peer_uid, &request) {
            HelperResponse::err(&err)
        } else {
            match dispatch(config, &request.command, runner) {
                Ok(pid) => HelperResponse::ok(pid),
                Err(err) => HelperResponse::err(&err),
            }
        };
        write_helper_response(&stream, response)
    }

    /// Production accept-loop entry: authenticate the peer and frame, then
    /// take the runner mutex only for dispatch. An unauthenticated connection
    /// cannot stall Start/Stop/SetDns.
    pub fn serve_peer<R: CoreRunner>(
        stream: UnixStream,
        config: &ServerConfig,
        peer_auth: &dyn PeerAuth,
        runner: &Mutex<R>,
    ) -> Result<(), TunError> {
        let peer_uid = peer_auth.peer_uid(&stream)?;
        let request = read_helper_request(&stream)?;
        let response = if let Err(err) = authenticate(config, peer_uid, &request) {
            HelperResponse::err(&err)
        } else {
            let mut runner = runner
                .lock()
                .map_err(|_| TunError::new(ErrorCode::TunApplyFailed, "runner lock poisoned"))?;
            match dispatch(config, &request.command, &mut *runner) {
                Ok(pid) => HelperResponse::ok(pid),
                Err(err) => HelperResponse::err(&err),
            }
        };
        write_helper_response(&stream, response)
    }

    /// Dispatch one validated command onto the runner.
    fn dispatch(
        config: &ServerConfig,
        command: &HelperCommand,
        runner: &mut dyn CoreRunner,
    ) -> Result<Option<u32>, TunError> {
        match command {
            HelperCommand::Status => Ok(runner.running_pid()),
            HelperCommand::Stop => {
                runner.stop()?;
                Ok(None)
            }
            HelperCommand::Start { config: path } => {
                let canonical = validate_config_path(&config.data_dir, path)?;
                let protected = sanitize_user_config(config, &canonical)?;
                let pid = runner.start(&config.core_bin, &protected, &config.core_log)?;
                if let Some(uid) = config.allowed_uid {
                    chown_core_log_to_allowed_uid(&config.core_log, uid);
                }
                Ok(Some(pid))
            }
            HelperCommand::SetDns { service, servers } => {
                validate_set_dns(service, servers)?;
                runner.set_dns(service, servers)?;
                Ok(None)
            }
            HelperCommand::TruncateCoreLog => {
                truncate_core_log_file(&config.core_log)?;
                if let Some(uid) = config.allowed_uid {
                    chown_core_log_to_allowed_uid(&config.core_log, uid);
                }
                Ok(None)
            }
        }
    }

    /// Read the user-writable config, sanitise it, and write a copy under the
    /// helper-owned run directory. The elevated core is started from that
    /// copy so a swapped `config.json` cannot pass gadgets through.
    fn sanitize_user_config(
        config: &ServerConfig,
        user_path: &std::path::Path,
    ) -> Result<PathBuf, TunError> {
        let raw = ice_config_guard::read_config_file(user_path).map_err(|err| {
            let code = if err.message.contains("open config")
                || err.message.contains("read config")
                || err.message.contains("stat config")
            {
                ErrorCode::TunApplyFailed
            } else {
                ErrorCode::TunConfigRejected
            };
            TunError::new(code, err.to_string())
        })?;
        let mut cfg: serde_json::Value = serde_json::from_slice(&raw).map_err(|err| {
            TunError::new(
                ErrorCode::TunConfigRejected,
                format!("config is not JSON: {err}"),
            )
        })?;
        let ctx = ice_config_guard::GuardContext {
            data_dir: config.data_dir.clone(),
            resources_dir: config.resources_dir.clone(),
            log_output: Some(config.core_log.clone()),
            cache_file_path: Some(config.protected_run_dir.join("cache.db")),
        };
        ice_config_guard::sanitize_for_elevated_core(&mut cfg, &ctx)
            .map_err(|err| TunError::new(ErrorCode::TunConfigRejected, err.to_string()))?;
        std::fs::create_dir_all(&config.protected_run_dir).map_err(|err| {
            TunError::new(
                ErrorCode::TunApplyFailed,
                format!(
                    "create protected run dir {}: {err}",
                    config.protected_run_dir.display()
                ),
            )
        })?;
        let dest = config.protected_run_dir.join("config.json");
        let bytes = serde_json::to_vec(&cfg).map_err(|err| {
            TunError::new(
                ErrorCode::TunApplyFailed,
                format!("encode sanitised config: {err}"),
            )
        })?;
        std::fs::write(&dest, bytes).map_err(|err| {
            TunError::new(
                ErrorCode::TunApplyFailed,
                format!("write sanitised config {}: {err}", dest.display()),
            )
        })?;
        let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o600));
        Ok(dest)
    }

    /// Run `networksetup -setdnsservers <service> <servers...>` as root. An
    /// empty `servers` list clears the override ("Empty" = DHCP fallback).
    /// Callers must run [`validate_set_dns`] first; the command uses an argv
    /// list, never a shell.
    fn set_system_dns(service: &str, servers: &[String]) -> Result<(), TunError> {
        let mut args = vec!["-setdnsservers", service];
        if servers.is_empty() {
            args.push("Empty");
        } else {
            args.extend(servers.iter().map(String::as_str));
        }
        let status = Command::new("networksetup")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|err| {
                TunError::new(
                    ErrorCode::TunApplyFailed,
                    format!("run networksetup {args:?}: {err}"),
                )
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(TunError::new(
                ErrorCode::TunApplyFailed,
                format!("networksetup -setdnsservers {service} failed (exit {status:?})"),
            ))
        }
    }

    /// The core lifecycle the daemon controls. Abstracted so tests inject a
    /// fake (or the real binary with a tiny fixture script).
    pub trait CoreRunner {
        /// Spawn `sing-box run -c <config>` with output to `log`; wait bounded
        /// for startup liveness; return the pid.
        fn start(
            &mut self,
            bin: &std::path::Path,
            config: &std::path::Path,
            log: &std::path::Path,
        ) -> Result<u32, TunError>;
        /// TERM→KILL with bounded grace; idempotent.
        fn stop(&mut self) -> Result<(), TunError>;
        /// The current core pid, if any. Reaps a core that exited on its own
        /// (an unreaped zombie would keep `kill(pid, 0)` reporting alive, and
        /// a stale pid would wrongly reject the next Start).
        fn running_pid(&mut self) -> Option<u32>;
        /// Apply `networksetup -setdnsservers` for one service. The daemon
        /// validates arguments before calling this.
        fn set_dns(&mut self, service: &str, servers: &[String]) -> Result<(), TunError>;
    }

    /// Real runner: spawns the bundled sing-box as root and terminates it with
    /// TERM→KILL grace. Mirrors `SudoCoreCoordinator`'s bounded waits. Keeps
    /// the `Child` handle so the process is reaped (a zombie would otherwise
    /// stay "alive" for `kill(pid, 0)`).
    ///
    /// A daemon restart loses the `Child` handle while the spawned core keeps
    /// running (the core is not a child of launchd), so `stop` also reclaims a
    /// leftover core the app recorded in its pid file (`sing-box.pid`): the
    /// helper is the only component allowed to terminate a root-owned core, and
    /// without this fallback an orphan would hold the inbound/Clash API ports
    /// and make the next Start fail with `bind: address already in use`.
    pub struct ProcessCoreRunner {
        child: Option<std::process::Child>,
        /// App data dir; contains the `sing-box.pid` file the app writes.
        data_dir: PathBuf,
        /// The bundled core binary path (fixed at install). Used to confirm a
        /// pid-file pid belongs to this installation's core before killing it,
        /// so an unrelated process is never terminated.
        core_bin: PathBuf,
    }

    impl ProcessCoreRunner {
        pub fn new(data_dir: PathBuf, core_bin: PathBuf) -> Self {
            Self {
                child: None,
                data_dir,
                core_bin,
            }
        }
    }

    impl Default for ProcessCoreRunner {
        fn default() -> Self {
            Self::new(PathBuf::new(), PathBuf::new())
        }
    }

    impl CoreRunner for ProcessCoreRunner {
        fn start(
            &mut self,
            bin: &std::path::Path,
            config: &std::path::Path,
            log: &std::path::Path,
        ) -> Result<u32, TunError> {
            if let Some(child) = self.child.as_mut() {
                // The saved handle may be stale: a core that exited on its
                // own leaves the child un-reaped (a zombie). Reap it so a new
                // Start is accepted; a genuinely running core keeps rejecting.
                if matches!(child.try_wait(), Ok(Some(_))) {
                    self.child = None;
                } else {
                    return Err(TunError::new(
                        ErrorCode::TunApplyFailed,
                        "core already running; stop it first",
                    ));
                }
            }
            if let Some(parent) = log.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    TunError::new(
                        ErrorCode::TunApplyFailed,
                        format!("create log dir {}: {e}", parent.display()),
                    )
                })?;
            }
            cap_core_log(log);
            let log_file = OpenOptions::new()
                .create(true)
                .append(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(log)
                .map_err(|e| {
                    TunError::new(
                        ErrorCode::TunApplyFailed,
                        format!("open core log {}: {e}", log.display()),
                    )
                })?;
            let log_err = log_file.try_clone().map_err(|e| {
                TunError::new(ErrorCode::TunApplyFailed, format!("clone log handle: {e}"))
            })?;
            let mut child = Command::new(bin)
                .arg("run")
                .arg("-c")
                .arg(config)
                .stdin(Stdio::null())
                .stdout(Stdio::from(log_file))
                .stderr(Stdio::from(log_err))
                .spawn()
                .map_err(|e| {
                    TunError::new(
                        ErrorCode::TunApplyFailed,
                        format!("spawn {} run -c {}: {e}", bin.display(), config.display()),
                    )
                })?;
            let pid = child.id();

            // Bounded liveness wait: catch immediate config/bind errors.
            let deadline = Instant::now() + STARTUP_LIVENESS_WAIT;
            loop {
                match child.try_wait() {
                    Ok(Some(code)) => {
                        return Err(TunError::new(
                            ErrorCode::TunHealthcheckFailed,
                            format!(
                                "core exited during startup (code {code}); check {}",
                                log.display()
                            ),
                        ));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        return Err(TunError::new(
                            ErrorCode::TunApplyFailed,
                            format!("poll core: {e}"),
                        ));
                    }
                }
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(LIVENESS_POLL);
            }
            self.child = Some(child);
            tracing::info!(pid, "core started via privileged helper");
            Ok(pid)
        }

        fn stop(&mut self) -> Result<(), TunError> {
            if self.child.is_none() {
                // No live handle: either nothing was ever started, or the
                // daemon restarted and the core outlived it. Reclaim the
                // leftover core from the app's pid file so an orphan cannot
                // hold the ports for the next Start.
                return self.stop_orphan_from_pid_file();
            }
            let result: Result<(), TunError> = (|| {
                let child = self.child.as_mut().expect("child checked above");
                let pid = child.id();
                let term = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
                if term != 0 {
                    let err = std::io::Error::last_os_error();
                    if err.raw_os_error() == Some(libc::ESRCH) {
                        // Already gone; reap and report clean.
                        let _ = child.wait();
                        Ok(())
                    } else {
                        Err(TunError::new(
                            ErrorCode::TunRestoreFailed,
                            format!("kill TERM {pid}: {err}"),
                        ))
                    }
                } else {
                    let deadline = Instant::now() + TERM_GRACE;
                    let mut terminated = false;
                    loop {
                        match child.try_wait() {
                            Ok(Some(_)) => {
                                terminated = true;
                                break;
                            }
                            Ok(None) => {}
                            Err(e) => {
                                return Err(TunError::new(
                                    ErrorCode::TunRestoreFailed,
                                    format!("wait core {pid}: {e}"),
                                ));
                            }
                        }
                        if Instant::now() >= deadline {
                            break;
                        }
                        std::thread::sleep(LIVENESS_POLL);
                    }
                    if terminated {
                        Ok(())
                    } else {
                        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                        let deadline = Instant::now() + KILL_GRACE;
                        loop {
                            match child.try_wait() {
                                Ok(Some(_)) => {
                                    terminated = true;
                                    break;
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    return Err(TunError::new(
                                        ErrorCode::TunRestoreFailed,
                                        format!("wait core {pid}: {e}"),
                                    ));
                                }
                            }
                            if Instant::now() >= deadline {
                                break;
                            }
                            std::thread::sleep(LIVENESS_POLL);
                        }
                        if terminated {
                            Ok(())
                        } else {
                            // Keep the handle on failure so a later
                            // Status/Stop request can still observe and retry
                            // cleanup of the live process.
                            Err(TunError::new(
                                ErrorCode::TunRecoveryRequired,
                                format!("core (pid {pid}) survived TERM and KILL"),
                            ))
                        }
                    }
                }
            })();
            if result.is_ok() {
                self.child = None;
            }
            result
        }

        fn running_pid(&mut self) -> Option<u32> {
            let child = self.child.as_mut()?;
            match child.try_wait() {
                // The core exited since the last command; drop the stale
                // handle (the reap also prevents a zombie pid from looking
                // alive forever).
                Ok(Some(_)) => {
                    self.child = None;
                    None
                }
                Ok(None) => Some(child.id()),
                // Poll errors are transient (EINTR); keep reporting the pid so
                // a caller does not treat a live core as stopped.
                Err(_) => Some(child.id()),
            }
        }

        fn set_dns(&mut self, service: &str, servers: &[String]) -> Result<(), TunError> {
            set_system_dns(service, servers)
        }
    }

    impl ProcessCoreRunner {
        /// Reclaim a leftover core the app recorded in `<data_dir>/sing-box.pid`.
        /// Confirms the pid is still alive and matches this installation's core
        /// binary before terminating it (TERM→KILL with bounded grace). No-op
        /// when there is no pid file, the process is gone, or the pid is an
        /// unrelated process.
        fn stop_orphan_from_pid_file(&self) -> Result<(), TunError> {
            let pid_file = self.data_dir.join("sing-box.pid");
            let raw = match std::fs::read_to_string(&pid_file) {
                Ok(raw) => raw,
                Err(_) => return Ok(()),
            };
            let Ok(pid) = raw.trim().parse::<u32>() else {
                return Ok(());
            };
            if !pid_alive(pid) {
                // The core already exited; nothing to reclaim.
                return Ok(());
            }
            if !pid_matches_core(pid, &self.core_bin) {
                // Unrelated process; never terminate it.
                tracing::warn!(
                    pid,
                    path = %pid_file.display(),
                    "pid file points at a process that is not this install's core; leaving it alone"
                );
                return Ok(());
            }
            tracing::info!(
                pid,
                path = %pid_file.display(),
                "reclaiming leftover core recorded in the pid file"
            );
            terminate_pid(pid, &self.core_bin.to_string_lossy())
        }
    }

    /// Unix liveness probe: `kill(pid, 0)` — 0 / EPERM mean alive (possibly
    /// root-owned), ESRCH means gone.
    fn pid_alive(pid: u32) -> bool {
        let rc = unsafe { libc::kill(pid as i32, 0) };
        rc == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }

    /// Whether `pid` is *running* (not a zombie). `kill(pid, 0)` stays 0 for a
    /// reaped-but-not-waited zombie, so termination must treat a zombie as gone
    /// or it would look like the core survived TERM/KILL forever.
    fn pid_running(pid: u32) -> bool {
        if !pid_alive(pid) {
            return false;
        }
        let output = match Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "stat="])
            .output()
        {
            Ok(output) if output.status.success() => output,
            _ => return true, // cannot determine; assume still running
        };
        !String::from_utf8_lossy(&output.stdout).trim().contains('Z')
    }

    /// Whether `pid`'s command line carries `core_bin`'s path — i.e. it is
    /// this installation's bundled core (the installer pins the location).
    fn pid_matches_core(pid: u32, core_bin: &std::path::Path) -> bool {
        let output = match Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .output()
        {
            Ok(output) => output,
            Err(_) => return false,
        };
        if !output.status.success() {
            return false;
        }
        String::from_utf8_lossy(&output.stdout).contains(core_bin.to_string_lossy().as_ref())
    }

    /// TERM→KILL a pid with bounded grace, probing liveness via `kill(pid, 0)`.
    /// Used to reclaim a leftover core when no `Child` handle is available
    /// (the daemon restarted and lost it).
    fn terminate_pid(pid: u32, desc: &str) -> Result<(), TunError> {
        let _ = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        let deadline = Instant::now() + TERM_GRACE;
        while Instant::now() < deadline {
            if !pid_running(pid) {
                return Ok(());
            }
            std::thread::sleep(LIVENESS_POLL);
        }
        let _ = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        let deadline = Instant::now() + KILL_GRACE;
        while Instant::now() < deadline {
            if !pid_running(pid) {
                return Ok(());
            }
            std::thread::sleep(LIVENESS_POLL);
        }
        if pid_running(pid) {
            Err(TunError::new(
                ErrorCode::TunRecoveryRequired,
                format!("orphan core (pid {pid}, {desc}) survived TERM and KILL"),
            ))
        } else {
            Ok(())
        }
    }

    /// Unix liveness probe: `kill(pid, 0)`.
    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::Read;
        use std::os::unix::net::UnixStream;
        use std::sync::Arc;

        /// Unix liveness probe: `kill(pid, 0)`.
        fn pid_is_alive(pid: u32) -> bool {
            let rc = unsafe { libc::kill(pid as i32, 0) };
            rc == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
        }

        static PEER42: FixedPeerAuth = FixedPeerAuth(42);
        static PEER7: FixedPeerAuth = FixedPeerAuth(7);

        fn fixture_config(token: &str, data_dir: &std::path::Path) -> ServerConfig {
            ServerConfig {
                token: token.to_string(),
                data_dir: data_dir.to_path_buf(),
                core_bin: PathBuf::from("/bin/sleep"),
                core_log: std::env::temp_dir().join("ice-helper-test.log"),
                allowed_uid: Some(42),
                protected_run_dir: data_dir.join("protected-run"),
                resources_dir: data_dir.join("resources"),
            }
        }

        fn write_allowed_config(path: &std::path::Path) {
            let json = serde_json::to_vec(&ice_config_guard::minimal_allowed_config()).unwrap();
            std::fs::write(path, json).unwrap();
        }

        /// Build a runner scoped to a fixture config's data dir / core binary.
        fn runner_for(config: &ServerConfig) -> ProcessCoreRunner {
            ProcessCoreRunner::new(config.data_dir.clone(), config.core_bin.clone())
        }

        /// Write an executable fixture "core" that ignores arguments, traps
        /// SIGTERM, and stays alive until terminated.
        fn fixture_core_bin(dir: &std::path::Path) -> PathBuf {
            let bin = dir.join("fake-core");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::write(
                    &bin,
                    "#!/bin/sh\ntrap 'exit 0' TERM\nwhile :; do sleep 1; done\n",
                )
                .unwrap();
                std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
                // execve returns ETXTBSY while the freshly written inode is
                // still considered busy (its write access lingers a few
                // milliseconds after the last write-open closes). Settle
                // briefly so a parallel spawn cannot race that window; a
                // 10 ms gap already measured 0 failures over 48k trials.
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            bin
        }

        /// In-process roundtrip: `serve_peer` on one end of a socketpair,
        /// the test drives the other end. The runner is shared across
        /// connections like the daemon's accept loop does.
        fn roundtrip<R: CoreRunner + Send + 'static>(
            config: &ServerConfig,
            auth: &'static dyn PeerAuth,
            runner: Arc<std::sync::Mutex<R>>,
            request: &ice_tun_helper_proto::HelperRequest,
        ) -> Result<ice_tun_helper_proto::HelperResponse, TunError> {
            let (client, server) = UnixStream::pair().expect("socketpair");
            let config = config.clone();
            std::thread::spawn(move || {
                let _ = serve_peer(server, &config, auth, &runner);
            });
            let mut line = ice_tun_helper_proto::encode_request(request)?;
            line.push(b'\n');
            let mut writer = client.try_clone()?;
            writer.write_all(&line)?;
            writer.flush().ok();
            let mut reader = BufReader::new(client);
            let mut response = String::new();
            reader.read_line(&mut response)?;
            ice_tun_helper_proto::decode_response(response.as_bytes())
        }

        fn status_request(token: &str) -> ice_tun_helper_proto::HelperRequest {
            ice_tun_helper_proto::HelperRequest {
                v: PROTOCOL_VERSION,
                token: token.to_string(),
                command: HelperCommand::Status,
            }
        }

        #[test]
        fn wrong_uid_is_rejected_before_dispatch() {
            let dir = std::env::temp_dir();
            let config = fixture_config("tok", &dir);
            let runner = Arc::new(std::sync::Mutex::new(runner_for(&config)));
            let response = roundtrip(&config, &PEER7, runner, &status_request("tok")).unwrap();
            assert!(!response.ok);
            assert_eq!(response.code.as_deref(), Some("tun.permission_required"));
        }

        #[test]
        fn unauthenticated_request_does_not_wait_for_runner_lock() {
            let dir = std::env::temp_dir();
            let config = fixture_config("tok", &dir);
            let runner = Arc::new(std::sync::Mutex::new(runner_for(&config)));
            let _held = runner.lock().expect("hold runner");
            let started = Instant::now();
            let response = roundtrip(&config, &PEER7, Arc::clone(&runner), &status_request("tok"))
                .expect("auth should complete without the runner mutex");
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "unauthenticated peers must not wait on the runner lock"
            );
            assert!(!response.ok);
            assert_eq!(response.code.as_deref(), Some("tun.permission_required"));
        }

        #[test]
        fn wrong_token_is_rejected() {
            let dir = std::env::temp_dir();
            let config = fixture_config("right-token", &dir);
            let runner = Arc::new(std::sync::Mutex::new(runner_for(&config)));
            let response =
                roundtrip(&config, &PEER42, runner, &status_request("wrong-token")).unwrap();
            assert!(!response.ok);
            assert_eq!(response.code.as_deref(), Some("tun.permission_required"));
        }

        #[test]
        fn wrong_version_is_rejected() {
            let dir = std::env::temp_dir();
            let config = fixture_config("tok", &dir);
            let runner = Arc::new(std::sync::Mutex::new(runner_for(&config)));
            let mut req = status_request("tok");
            req.v = 999;
            let response = roundtrip(&config, &PEER42, runner, &req).unwrap();
            assert!(!response.ok);
            assert_eq!(response.code.as_deref(), Some("tun.apply_failed"));
        }

        #[test]
        fn status_ok_reports_no_running_core() {
            let dir = std::env::temp_dir();
            let config = fixture_config("tok", &dir);
            let runner = Arc::new(std::sync::Mutex::new(runner_for(&config)));
            let response = roundtrip(&config, &PEER42, runner, &status_request("tok")).unwrap();
            assert!(response.ok);
            assert_eq!(response.pid, None);
        }

        #[test]
        fn truncate_core_log_empties_current_and_drops_rotations() {
            let dir = std::env::temp_dir().join(format!(
                "ice-helper-truncate-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let mut config = fixture_config("tok", &dir);
            config.core_log = dir.join("core.log");
            std::fs::write(&config.core_log, vec![b'x'; 64]).unwrap();
            std::fs::write(dir.join("core.log.1"), b"old").unwrap();
            let runner = Arc::new(std::sync::Mutex::new(runner_for(&config)));
            let mut req = status_request("tok");
            req.command = HelperCommand::TruncateCoreLog;
            let response = roundtrip(&config, &PEER42, runner, &req).unwrap();
            assert!(response.ok, "{response:?}");
            assert_eq!(std::fs::read(&config.core_log).unwrap(), b"");
            assert!(!dir.join("core.log.1").exists());
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn cap_core_log_drops_oldest_quarter_and_siblings() {
            let dir = std::env::temp_dir().join(format!(
                "ice-helper-cap-log-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("core.log");
            std::fs::write(&path, vec![b'x'; 64]).unwrap();
            std::fs::write(dir.join("core.log.1"), b"old").unwrap();
            cap_core_log_at(&path, 32, 3);
            assert_eq!(std::fs::read(&path).unwrap(), vec![b'x'; 32]);
            assert!(!dir.join("core.log.1").exists());
            std::fs::write(&path, vec![b'y'; 8]).unwrap();
            cap_core_log_at(&path, 32, 3);
            assert_eq!(std::fs::read(&path).unwrap()[0], b'y');
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn start_rejects_config_outside_data_dir() {
            let dir = std::env::temp_dir().join(format!(
                "ice-helper-outside-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let config = fixture_config("tok", &dir);
            let runner = Arc::new(std::sync::Mutex::new(runner_for(&config)));
            let mut req = status_request("tok");
            req.command = HelperCommand::Start {
                config: "/etc/hosts".into(),
            };
            let response = roundtrip(&config, &PEER42, runner, &req).unwrap();
            assert!(!response.ok);
            assert_eq!(response.code.as_deref(), Some("tun.permission_required"));
            std::fs::remove_dir_all(&dir).unwrap();
        }

        struct RecordingDnsRunner {
            last_dns: Option<(String, Vec<String>)>,
        }

        impl CoreRunner for RecordingDnsRunner {
            fn start(
                &mut self,
                _bin: &std::path::Path,
                _config: &std::path::Path,
                _log: &std::path::Path,
            ) -> Result<u32, TunError> {
                Err(TunError::new(
                    ErrorCode::TunApplyFailed,
                    "RecordingDnsRunner does not start a core",
                ))
            }

            fn stop(&mut self) -> Result<(), TunError> {
                Ok(())
            }

            fn running_pid(&mut self) -> Option<u32> {
                None
            }

            fn set_dns(&mut self, service: &str, servers: &[String]) -> Result<(), TunError> {
                self.last_dns = Some((service.to_string(), servers.to_vec()));
                Ok(())
            }
        }

        fn set_dns_request(service: &str, servers: &[&str]) -> ice_tun_helper_proto::HelperRequest {
            ice_tun_helper_proto::HelperRequest {
                v: PROTOCOL_VERSION,
                token: "tok".into(),
                command: HelperCommand::SetDns {
                    service: service.into(),
                    servers: servers.iter().map(|s| (*s).to_string()).collect(),
                },
            }
        }

        #[test]
        fn set_dns_rejects_invalid_service_server_and_count() {
            let dir = std::env::temp_dir();
            let config = fixture_config("tok", &dir);
            let runner = Arc::new(std::sync::Mutex::new(RecordingDnsRunner { last_dns: None }));

            let response = roundtrip(
                &config,
                &PEER42,
                runner.clone(),
                &set_dns_request("Wi-Fi; rm -rf /", &["1.1.1.1"]),
            )
            .unwrap();
            assert!(!response.ok);
            assert_eq!(response.code.as_deref(), Some("tun.invalid_argument"));
            assert!(runner.lock().unwrap().last_dns.is_none());

            let response = roundtrip(
                &config,
                &PEER42,
                runner.clone(),
                &set_dns_request("Wi-Fi", &["8.8.8.8 evil"]),
            )
            .unwrap();
            assert!(!response.ok);
            assert_eq!(response.code.as_deref(), Some("tun.invalid_argument"));
            assert!(runner.lock().unwrap().last_dns.is_none());

            let five = ["1.1.1.1", "1.0.0.1", "8.8.8.8", "8.8.4.4", "9.9.9.9"];
            let response = roundtrip(
                &config,
                &PEER42,
                runner.clone(),
                &set_dns_request("Wi-Fi", &five),
            )
            .unwrap();
            assert!(!response.ok);
            assert_eq!(response.code.as_deref(), Some("tun.invalid_argument"));
            assert!(runner.lock().unwrap().last_dns.is_none());
        }

        #[test]
        fn set_dns_accepts_validated_payload() {
            let dir = std::env::temp_dir();
            let config = fixture_config("tok", &dir);
            let runner = Arc::new(std::sync::Mutex::new(RecordingDnsRunner { last_dns: None }));
            let response = roundtrip(
                &config,
                &PEER42,
                runner.clone(),
                &set_dns_request("Wi-Fi", &["1.1.1.1"]),
            )
            .unwrap();
            assert!(response.ok, "set_dns failed: {:?}", response.message);
            assert_eq!(
                runner.lock().unwrap().last_dns,
                Some(("Wi-Fi".into(), vec!["1.1.1.1".into()]))
            );
        }

        #[test]
        fn start_and_stop_roundtrip_with_fake_core() {
            let dir = std::env::temp_dir().join(format!(
                "ice-helper-start-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let config_path = dir.join("config.json");
            write_allowed_config(&config_path);

            // The fixture "core" ignores args and sleeps so liveness holds.
            let mut config = fixture_config("tok", &dir);
            config.core_bin = fixture_core_bin(&dir);
            let runner = Arc::new(std::sync::Mutex::new(runner_for(&config)));

            let mut req = status_request("tok");
            req.command = HelperCommand::Start {
                config: config_path.to_string_lossy().into_owned(),
            };
            let response = roundtrip(&config, &PEER42, runner.clone(), &req).unwrap();
            assert!(response.ok, "start failed: {:?}", response.message);
            let pid = response.pid.expect("pid");

            let stop_req = ice_tun_helper_proto::HelperRequest {
                v: PROTOCOL_VERSION,
                token: "tok".into(),
                command: HelperCommand::Stop,
            };
            let response = roundtrip(&config, &PEER42, runner, &stop_req).unwrap();
            assert!(response.ok, "stop failed: {:?}", response.message);
            assert!(!pid_is_alive(pid), "core must be gone after stop");

            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn start_rejects_tor_outbound() {
            let dir = std::env::temp_dir().join(format!(
                "ice-helper-tor-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let config_path = dir.join("config.json");
            let mut cfg = ice_config_guard::minimal_allowed_config();
            cfg["outbounds"] = serde_json::json!([{
                "type": "tor",
                "tag": "evil",
                "executable_path": "/usr/bin/tor"
            }]);
            std::fs::write(&config_path, serde_json::to_vec(&cfg).unwrap()).unwrap();

            let mut config = fixture_config("tok", &dir);
            config.core_bin = fixture_core_bin(&dir);
            let runner = Arc::new(std::sync::Mutex::new(runner_for(&config)));

            let mut req = status_request("tok");
            req.command = HelperCommand::Start {
                config: config_path.to_string_lossy().into_owned(),
            };
            let response = roundtrip(&config, &PEER42, runner, &req).unwrap();
            assert!(!response.ok);
            assert_eq!(response.code.as_deref(), Some("tun.config_rejected"));
            assert!(
                response
                    .message
                    .as_deref()
                    .unwrap_or("")
                    .contains("/outbounds/0/type"),
                "{:?}",
                response.message
            );

            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn constant_time_eq_works() {
            assert!(constant_time_eq("abc", "abc"));
            assert!(!constant_time_eq("abc", "abd"));
            assert!(!constant_time_eq("abc", "abcd"));
            assert!(!constant_time_eq("", "a"));
            assert!(constant_time_eq("", ""));
        }

        #[test]
        fn oversized_request_frame_is_rejected_without_a_response() {
            let dir = std::env::temp_dir().join(format!(
                "ice-helper-oversize-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let config = fixture_config("tok", &dir);
            let runner = Arc::new(std::sync::Mutex::new(runner_for(&config)));
            let (mut client, server) = UnixStream::pair().expect("socketpair");

            let config = config.clone();
            let runner = runner.clone();
            let handle = std::thread::spawn(move || {
                let _ = serve_peer(server, &config, &PEER42, &runner);
            });

            // A well-formed request whose config field pushes the line over
            // the 16 KiB cap. The daemon must reject it without buffering
            // the whole payload or dispatching anything.
            let oversized = format!(
                "{{\"v\":1,\"token\":\"tok\",\"cmd\":\"start\",\"config\":\"{}\"}}\n",
                "x".repeat(MAX_FRAME_BYTES)
            );
            assert!(oversized.len() > MAX_FRAME_BYTES);
            let mut writer = client.try_clone().unwrap();
            writer.write_all(oversized.as_bytes()).unwrap();
            writer.flush().ok();

            // The connection is closed with no response frame.
            client
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut buf = String::new();
            let result = client.read_to_string(&mut buf);
            assert!(
                result.is_err() || buf.is_empty(),
                "oversized request must not receive a response frame"
            );
            handle.join().expect("serve thread");
            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn start_twice_is_rejected() {
            let dir = std::env::temp_dir().join(format!(
                "ice-helper-twice-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let config_path = dir.join("config.json");
            write_allowed_config(&config_path);

            let mut config = fixture_config("tok", &dir);
            config.core_bin = fixture_core_bin(&dir);
            let runner = Arc::new(std::sync::Mutex::new(runner_for(&config)));

            let mut req = status_request("tok");
            req.command = HelperCommand::Start {
                config: config_path.to_string_lossy().into_owned(),
            };
            let first = roundtrip(&config, &PEER42, runner.clone(), &req).unwrap();
            assert!(first.ok);

            // Second connection against the same runner: still holding the pid.
            let mut req2 = status_request("tok");
            req2.command = HelperCommand::Start {
                config: config_path.to_string_lossy().into_owned(),
            };
            let second = roundtrip(&config, &PEER42, runner.clone(), &req2).unwrap();
            assert!(!second.ok, "second start must be rejected");

            // Cleanup: TERM the running sleep.
            let stop_req = ice_tun_helper_proto::HelperRequest {
                v: PROTOCOL_VERSION,
                token: "tok".into(),
                command: HelperCommand::Stop,
            };
            let _ = roundtrip(&config, &PEER42, runner, &stop_req).unwrap();
            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn status_reaps_exited_core_and_start_can_retry() {
            let dir = std::env::temp_dir().join(format!(
                "ice-helper-reap-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let config_path = dir.join("config.json");
            write_allowed_config(&config_path);

            let mut config = fixture_config("tok", &dir);
            config.core_bin = fixture_core_bin(&dir);
            let runner = Arc::new(std::sync::Mutex::new(runner_for(&config)));

            let mut start_req = status_request("tok");
            start_req.command = HelperCommand::Start {
                config: config_path.to_string_lossy().into_owned(),
            };
            let resp = roundtrip(&config, &PEER42, runner.clone(), &start_req).unwrap();
            assert!(resp.ok, "start failed: {:?}", resp.message);
            let pid = resp.pid.expect("pid");
            assert!(pid_is_alive(pid));

            // The core dies on its own (SIGKILL; the fixture only traps TERM):
            // the daemon must notice on the next Status instead of reporting
            // the stale pid forever and rejecting the next Start.
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            let mut reaped = false;
            for _ in 0..100 {
                let resp =
                    roundtrip(&config, &PEER42, runner.clone(), &status_request("tok")).unwrap();
                if resp.pid.is_none() {
                    reaped = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(reaped, "status must report the exited core as gone");

            let resp = roundtrip(&config, &PEER42, runner.clone(), &start_req).unwrap();
            assert!(resp.ok, "start after reap must succeed: {:?}", resp.message);
            let pid2 = resp.pid.expect("pid2");
            assert_ne!(pid2, pid, "a fresh process must be spawned");

            let stop_req = ice_tun_helper_proto::HelperRequest {
                v: PROTOCOL_VERSION,
                token: "tok".into(),
                command: HelperCommand::Stop,
            };
            let resp = roundtrip(&config, &PEER42, runner, &stop_req).unwrap();
            assert!(resp.ok, "stop failed: {:?}", resp.message);
            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn stop_with_lost_handle_reclaims_pid_file_core() {
            let dir = std::env::temp_dir().join(format!(
                "ice-helper-orphan-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let fake_bin = fixture_core_bin(&dir);
            let mut config = fixture_config("tok", &dir);
            config.core_bin = fake_bin.clone();

            // Simulate a leftover core from a previous daemon session: it runs
            // as a detached process whose handle is gone (daemon restarted).
            let child = std::process::Command::new(&fake_bin).spawn().unwrap();
            let pid = child.id();
            drop(child);
            std::fs::write(dir.join("sing-box.pid"), pid.to_string()).unwrap();

            // A fresh runner holds no handle; Stop must reclaim via the pid file.
            let mut runner = runner_for(&config);
            runner.stop().unwrap();
            assert!(
                !pid_running(pid),
                "orphan core must be reclaimed via the pid file"
            );

            std::fs::remove_dir_all(&dir).unwrap();
        }

        #[test]
        fn stop_leaves_unrelated_pid_file_process_alone() {
            let dir = std::env::temp_dir().join(format!(
                "ice-helper-unrelated-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let config = fixture_config("tok", &dir);

            // A live process that is NOT this install's core binary.
            let mut child = std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .unwrap();
            let pid = child.id();
            std::fs::write(dir.join("sing-box.pid"), pid.to_string()).unwrap();

            let mut runner = runner_for(&config);
            runner.stop().unwrap();
            assert!(
                pid_alive(pid),
                "stop must not terminate a pid that is not the core binary"
            );

            let _ = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            let _ = child.wait();
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }
} // mod imp

#[cfg(all(unix, any(test, feature = "test-hooks")))]
pub use imp::FixedPeerAuth;
#[cfg(unix)]
pub use imp::{
    serve_connection, serve_peer, PeerAuth, ProcessCoreRunner, ServerConfig, SocketPeerAuth,
};
