//! Privileged helper daemon core (plan §5 T5, macOS production path).
//!
//! The daemon runs as root under launchd. It owns exactly one capability:
//! start the bundled sing-box with an allowlisted config path and terminate
//! it again (TERM→KILL with bounded grace). sing-box owns the adapter /
//! routes / DNS; `ice-tun-sys` coordinates and verifies (T0 lock §24.5.5),
//! so the helper never needs route / adapter / DNS primitives and its IPC
//! surface stays narrow (plan §7).
//!
//! Security model:
//!
//! - One request frame per connection; the client reconnects per command.
//! - Peer identity: the socket's `getpeereid` uid must equal the authorized
//!   user (the uid the installer recorded). Everything else is rejected
//!   before the frame is read.
//! - The request must carry the per-installation token (constant-time
//!   compare) and protocol version 1.
//! - `Start` accepts a config path only when it canonicalizes inside the
//!   data directory the daemon was installed with. The core binary path is
//!   fixed at install; the client never supplies it.
//!
//! The server logic is host-free (inject the peer uid and a fake core
//! binary), so the same code tests on Linux and macOS CI. On non-unix
//! platforms the crate builds as a stub so the workspace gate stays green
//! everywhere (the daemon is macOS-only).

#[cfg(unix)]
mod imp {
    use std::fs::OpenOptions;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use ice_tun_sys::error::{TunError, TunErrorCode};
    use ice_tun_sys::helper_protocol::{
        validate_config_path, HelperCommand, HelperRequest, HelperResponse, MAX_FRAME_BYTES,
        PROTOCOL_VERSION,
    };

    /// How long to wait for the elevated core to stay alive during startup
    /// (config/bind errors surface as an early exit) before accepting it.
    const STARTUP_LIVENESS_WAIT: Duration = Duration::from_millis(2000);
    const LIVENESS_POLL: Duration = Duration::from_millis(100);
    /// Bounded wait for the core to die after SIGTERM, then SIGKILL.
    const TERM_GRACE: Duration = Duration::from_secs(5);
    const KILL_GRACE: Duration = Duration::from_secs(2);
    /// Bounded time a peer may take to deliver its single request frame. A
    /// stalled or half-written frame must not block the single-threaded
    /// accept loop forever.
    const READ_TIMEOUT: Duration = Duration::from_secs(5);

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
    }

    /// Peer-identity probe. Production uses the real socket credential;
    /// tests inject a fixed uid so the dispatch logic stays host-free.
    pub trait PeerAuth: Sync {
        fn peer_uid(&self, stream: &UnixStream) -> Result<u32, TunError>;
    }

    /// Reads the peer uid from the socket (`getpeereid` on macOS; `SO_PEERCRED`
    /// on Linux). Used by `main`; tests inject [`FixedPeerAuth`].
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
                    TunErrorCode::PermissionRequired,
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
                    TunErrorCode::PermissionRequired,
                    format!("SO_PEERCRED: {}", std::io::Error::last_os_error()),
                ));
            }
            Ok(cred.uid)
        }
    }

    /// Test-only peer auth with a fixed uid.
    pub struct FixedPeerAuth(pub u32);

    impl PeerAuth for FixedPeerAuth {
        fn peer_uid(&self, _stream: &UnixStream) -> Result<u32, TunError> {
            Ok(self.0)
        }
    }

    /// Constant-time string compare (token check).
    fn constant_time_eq(a: &str, b: &str) -> bool {
        let a = a.as_bytes();
        let b = b.as_bytes();
        if a.len() != b.len() {
            return false;
        }
        a.iter()
            .zip(b.iter())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
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
                    TunErrorCode::PermissionRequired,
                    format!("peer uid {peer_uid} is not the authorized user {allowed}"),
                ));
            }
        }
        if request.v != PROTOCOL_VERSION {
            return Err(TunError::new(
                TunErrorCode::ApplyFailed,
                format!(
                    "protocol version mismatch: client {}, daemon {PROTOCOL_VERSION}",
                    request.v
                ),
            ));
        }
        if !constant_time_eq(&config.token, &request.token) {
            return Err(TunError::new(
                TunErrorCode::PermissionRequired,
                "invalid helper token",
            ));
        }
        Ok(())
    }

    /// One request per connection: read a frame, authenticate, dispatch, reply.
    /// The caller owns the core lifecycle (`CoreRunner`); `serve_connection`
    /// keeps it across frames is not needed because the client reconnects per
    /// command, so the runner is passed in and out.
    pub fn serve_connection(
        stream: UnixStream,
        config: &ServerConfig,
        peer_auth: &dyn PeerAuth,
        runner: &mut dyn CoreRunner,
    ) -> Result<(), TunError> {
        let peer_uid = peer_auth.peer_uid(&stream)?;
        // Bounded read: a peer that connects but never finishes its frame
        // must not stall the single-threaded accept loop.
        stream.set_read_timeout(Some(READ_TIMEOUT)).map_err(|e| {
            TunError::new(TunErrorCode::ApplyFailed, format!("set read timeout: {e}"))
        })?;
        let mut reader =
            BufReader::new(stream.try_clone().map_err(|e| {
                TunError::new(TunErrorCode::ApplyFailed, format!("clone stream: {e}"))
            })?);
        // Cap the request at MAX_FRAME_BYTES + 1 bytes: an oversized or
        // unterminated frame is rejected without buffering unbounded input
        // (the wire protocol caps every line at MAX_FRAME_BYTES).
        let mut line = String::new();
        let read = reader
            .by_ref()
            .take((MAX_FRAME_BYTES + 1) as u64)
            .read_line(&mut line)
            .map_err(|e| TunError::new(TunErrorCode::ApplyFailed, format!("read request: {e}")))?;
        if read == 0 {
            return Err(TunError::new(TunErrorCode::ApplyFailed, "empty request"));
        }
        if line.len() > MAX_FRAME_BYTES {
            return Err(TunError::new(
                TunErrorCode::ApplyFailed,
                format!("request frame exceeds {MAX_FRAME_BYTES} bytes"),
            ));
        }
        let request: HelperRequest = serde_json::from_str(line.trim_end()).map_err(|e| {
            TunError::new(TunErrorCode::ApplyFailed, format!("decode request: {e}"))
        })?;
        let response = if let Err(err) = authenticate(config, peer_uid, &request) {
            HelperResponse::err(&err)
        } else {
            match dispatch(config, &request.command, runner) {
                Ok(pid) => HelperResponse::ok(pid),
                Err(err) => HelperResponse::err(&err),
            }
        };
        let mut frame = serde_json::to_vec(&response).map_err(|e| {
            TunError::new(TunErrorCode::ApplyFailed, format!("encode response: {e}"))
        })?;
        if frame.len() > ice_tun_sys::helper_protocol::MAX_FRAME_BYTES {
            return Err(TunError::new(
                TunErrorCode::ApplyFailed,
                "response frame exceeds limit",
            ));
        }
        frame.push(b'\n');
        let mut writer = stream
            .try_clone()
            .map_err(|e| TunError::new(TunErrorCode::ApplyFailed, format!("clone stream: {e}")))?;
        writer.write_all(&frame).map_err(|e| {
            TunError::new(TunErrorCode::ApplyFailed, format!("write response: {e}"))
        })?;
        writer.flush().ok();
        Ok(())
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
                let pid = runner.start(&config.core_bin, &canonical, &config.core_log)?;
                Ok(Some(pid))
            }
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
    }

    /// Real runner: spawns the bundled sing-box as root and terminates it with
    /// TERM→KILL grace. Mirrors `SudoCoreCoordinator`'s bounded waits. Keeps
    /// the `Child` handle so the process is reaped (a zombie would otherwise
    /// stay "alive" for `kill(pid, 0)`).
    pub struct ProcessCoreRunner {
        child: Option<std::process::Child>,
    }

    impl ProcessCoreRunner {
        pub fn new() -> Self {
            Self { child: None }
        }
    }

    impl Default for ProcessCoreRunner {
        fn default() -> Self {
            Self::new()
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
                        TunErrorCode::ApplyFailed,
                        "core already running; stop it first",
                    ));
                }
            }
            if let Some(parent) = log.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    TunError::new(
                        TunErrorCode::ApplyFailed,
                        format!("create log dir {}: {e}", parent.display()),
                    )
                })?;
            }
            let log_file = OpenOptions::new()
                .create(true)
                .append(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(log)
                .map_err(|e| {
                    TunError::new(
                        TunErrorCode::ApplyFailed,
                        format!("open core log {}: {e}", log.display()),
                    )
                })?;
            let log_err = log_file.try_clone().map_err(|e| {
                TunError::new(TunErrorCode::ApplyFailed, format!("clone log handle: {e}"))
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
                        TunErrorCode::ApplyFailed,
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
                            TunErrorCode::HealthcheckFailed,
                            format!(
                                "core exited during startup (code {code}); check {}",
                                log.display()
                            ),
                        ));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        return Err(TunError::new(
                            TunErrorCode::ApplyFailed,
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
                return Ok(());
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
                            TunErrorCode::RestoreFailed,
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
                                    TunErrorCode::RestoreFailed,
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
                                        TunErrorCode::RestoreFailed,
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
                                TunErrorCode::RecoveryRequired,
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
            }
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
            }
            bin
        }

        /// In-process roundtrip: `serve_connection` on one end of a socketpair,
        /// the test drives the other end. The runner is shared across
        /// connections like the daemon's accept loop does.
        fn roundtrip(
            config: &ServerConfig,
            auth: &'static dyn PeerAuth,
            runner: Arc<std::sync::Mutex<ProcessCoreRunner>>,
            request: &ice_tun_sys::helper_protocol::HelperRequest,
        ) -> Result<ice_tun_sys::helper_protocol::HelperResponse, TunError> {
            let (client, server) = UnixStream::pair().expect("socketpair");
            let config = config.clone();
            std::thread::spawn(move || {
                let mut runner = runner.lock().expect("runner lock");
                let _ = serve_connection(server, &config, auth, &mut *runner);
            });
            let mut line = ice_tun_sys::helper_protocol::encode_request(request)?;
            line.push(b'\n');
            let mut writer = client.try_clone()?;
            writer.write_all(&line)?;
            writer.flush().ok();
            let mut reader = BufReader::new(client);
            let mut response = String::new();
            reader.read_line(&mut response)?;
            ice_tun_sys::helper_protocol::decode_response(response.as_bytes())
        }

        fn status_request(token: &str) -> ice_tun_sys::helper_protocol::HelperRequest {
            ice_tun_sys::helper_protocol::HelperRequest {
                v: PROTOCOL_VERSION,
                token: token.to_string(),
                command: HelperCommand::Status,
            }
        }

        #[test]
        fn wrong_uid_is_rejected_before_dispatch() {
            let dir = std::env::temp_dir();
            let config = fixture_config("tok", &dir);
            let runner = Arc::new(std::sync::Mutex::new(ProcessCoreRunner::new()));
            let response = roundtrip(&config, &PEER7, runner, &status_request("tok")).unwrap();
            assert!(!response.ok);
            assert_eq!(response.code.as_deref(), Some("tun.permission_required"));
        }

        #[test]
        fn wrong_token_is_rejected() {
            let dir = std::env::temp_dir();
            let config = fixture_config("right-token", &dir);
            let runner = Arc::new(std::sync::Mutex::new(ProcessCoreRunner::new()));
            let response =
                roundtrip(&config, &PEER42, runner, &status_request("wrong-token")).unwrap();
            assert!(!response.ok);
            assert_eq!(response.code.as_deref(), Some("tun.permission_required"));
        }

        #[test]
        fn wrong_version_is_rejected() {
            let dir = std::env::temp_dir();
            let config = fixture_config("tok", &dir);
            let runner = Arc::new(std::sync::Mutex::new(ProcessCoreRunner::new()));
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
            let runner = Arc::new(std::sync::Mutex::new(ProcessCoreRunner::new()));
            let response = roundtrip(&config, &PEER42, runner, &status_request("tok")).unwrap();
            assert!(response.ok);
            assert_eq!(response.pid, None);
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
            let runner = Arc::new(std::sync::Mutex::new(ProcessCoreRunner::new()));
            let mut req = status_request("tok");
            req.command = HelperCommand::Start {
                config: "/etc/hosts".into(),
            };
            let response = roundtrip(&config, &PEER42, runner, &req).unwrap();
            assert!(!response.ok);
            assert_eq!(response.code.as_deref(), Some("tun.permission_required"));
            std::fs::remove_dir_all(&dir).unwrap();
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
            std::fs::write(&config_path, b"{}").unwrap();

            // The fixture "core" ignores args and sleeps so liveness holds.
            let mut config = fixture_config("tok", &dir);
            config.core_bin = fixture_core_bin(&dir);
            let runner = Arc::new(std::sync::Mutex::new(ProcessCoreRunner::new()));

            let mut req = status_request("tok");
            req.command = HelperCommand::Start {
                config: config_path.to_string_lossy().into_owned(),
            };
            let response = roundtrip(&config, &PEER42, runner.clone(), &req).unwrap();
            assert!(response.ok, "start failed: {:?}", response.message);
            let pid = response.pid.expect("pid");

            let stop_req = ice_tun_sys::helper_protocol::HelperRequest {
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
            let runner = Arc::new(std::sync::Mutex::new(ProcessCoreRunner::new()));
            let (mut client, server) = UnixStream::pair().expect("socketpair");

            let config = config.clone();
            let runner = runner.clone();
            let handle = std::thread::spawn(move || {
                let mut runner = runner.lock().expect("runner lock");
                let _ = serve_connection(server, &config, &PEER42, &mut *runner);
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
            std::fs::write(&config_path, b"{}").unwrap();

            let mut config = fixture_config("tok", &dir);
            config.core_bin = fixture_core_bin(&dir);
            let runner = Arc::new(std::sync::Mutex::new(ProcessCoreRunner::new()));

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
            let stop_req = ice_tun_sys::helper_protocol::HelperRequest {
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
            std::fs::write(&config_path, b"{}").unwrap();

            let mut config = fixture_config("tok", &dir);
            config.core_bin = fixture_core_bin(&dir);
            let runner = Arc::new(std::sync::Mutex::new(ProcessCoreRunner::new()));

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

            let stop_req = ice_tun_sys::helper_protocol::HelperRequest {
                v: PROTOCOL_VERSION,
                token: "tok".into(),
                command: HelperCommand::Stop,
            };
            let resp = roundtrip(&config, &PEER42, runner, &stop_req).unwrap();
            assert!(resp.ok, "stop failed: {:?}", resp.message);
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }
} // mod imp

#[cfg(unix)]
pub use imp::{
    serve_connection, FixedPeerAuth, PeerAuth, ProcessCoreRunner, ServerConfig, SocketPeerAuth,
};
