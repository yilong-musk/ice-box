//! Privileged helper daemon entry (plan §5 T5, macOS production path).
//!
//! Runs as root under launchd. Binds a Unix socket, authenticates each peer
//! by socket credential + per-installation token, and serves the narrow
//! Start / Stop / Status contract. The core binary and data dir come from
//! the environment the installer records in the launchd plist; the client
//! can never supply them.
//!
//! The same binary also implements the privileged `install` / `uninstall`
//! modes used by the in-app authorization dialog (ice-elevate) and the
//! manual/CI scripts: there is exactly one implementation of the installation
//! logic. See `install.rs`.
//!
//! The daemon never enables capture itself, never touches routes, adapters,
//! or DNS, and never accepts arbitrary commands (plan §7): sing-box owns the
//! TUN resources; this process only runs and terminates it.
//!
//! Non-unix builds are a stub so the workspace gate stays green on Windows
//! CI; the daemon is macOS-only.

#[cfg(not(unix))]
fn main() {
    eprintln!("ice-helper is a macOS-only daemon");
    std::process::exit(1);
}

#[cfg(unix)]
mod unix_main {
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::process::exit;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use ice_helper::{ProcessCoreRunner, ServerConfig, SocketPeerAuth};
    use ice_tun_sys::error::{TunError, TunErrorCode};

    use crate::install::{
        ENV_ALLOWED_UID, ENV_CORE_BIN, ENV_CORE_BIN_SHA256, ENV_CORE_LOG, ENV_DATA_DIR, ENV_SOCKET,
        ENV_TOKEN,
    };

    /// Upper bound on concurrently served connections. The socket is
    /// world-connectable, so any local process can open one; a cap keeps an
    /// idle-connection flood from exhausting threads (each connection holds a
    /// thread only for its read bound).
    const MAX_CONNECTIONS: usize = 16;

    fn env_required(key: &str) -> Result<String, String> {
        std::env::var(key).map_err(|_| format!("missing required env {key}"))
    }

    fn load_config() -> Result<ServerConfig, String> {
        let allowed_uid = match std::env::var(ENV_ALLOWED_UID) {
            Ok(value) => Some(
                value
                    .parse::<u32>()
                    .map_err(|_| format!("{ENV_ALLOWED_UID} is not a uid: {value}"))?,
            ),
            Err(_) => None,
        };
        let token = env_required(ENV_TOKEN)?;
        let core_bin = env_required(ENV_CORE_BIN)?;
        // The installer pins the SHA-256 of the binary it copied into the
        // root-owned location; a tampered or replaced core is refused before
        // anything is ever executed as root.
        let expected_sha256 = env_required(ENV_CORE_BIN_SHA256)?;
        let core_log = env_required(ENV_CORE_LOG)?;
        let data_dir = env_required(ENV_DATA_DIR)?;
        if !PathBuf::from(&core_bin).is_file() {
            return Err(format!("core binary not found: {core_bin}"));
        }
        if !PathBuf::from(&data_dir).is_dir() {
            return Err(format!("data dir not found: {data_dir}"));
        }
        let actual_sha256 = crate::install::sha256_of_file(PathBuf::from(&core_bin).as_path())?;
        if actual_sha256 != expected_sha256 {
            return Err(format!(
                "core binary {} does not match the pinned sha256 (expected {expected_sha256}, got {actual_sha256}); refusing to start",
                core_bin
            ));
        }
        Ok(ServerConfig {
            token,
            data_dir: PathBuf::from(data_dir),
            core_bin: PathBuf::from(core_bin),
            core_log: PathBuf::from(core_log),
            allowed_uid,
        })
    }

    /// Serve one connection: read a single request frame, dispatch, respond.
    /// The runner is shared across connections (one core at a time).
    fn serve_connection(
        stream: UnixStream,
        config: &ServerConfig,
        auth: &SocketPeerAuth,
        runner: &mut ProcessCoreRunner,
    ) -> Result<(), TunError> {
        ice_helper::serve_connection(stream, config, auth, runner)
    }

    pub(crate) fn run_daemon() {
        let config = match load_config() {
            Ok(config) => config,
            Err(err) => {
                eprintln!("ice-helper: {err}");
                exit(1);
            }
        };

        let socket_path = std::env::var(ENV_SOCKET)
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(ice_tun_sys::helper_protocol::DEFAULT_SOCKET_PATH));
        if socket_path.exists() {
            // Stale socket from a previous run (daemon crashed without cleanup).
            let _ = std::fs::remove_file(&socket_path);
        }
        let listener = match UnixListener::bind(&socket_path) {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("ice-helper: bind {}: {err}", socket_path.display());
                exit(1);
            }
        };
        // World-connectable socket: the desktop app runs as the normal user
        // while the daemon runs as root, so a root-only 0600 socket would
        // reject it before authentication. Authorization happens *on top* of
        // the connection: peer uid (socket credential) + per-installation
        // token, so an unauthenticated peer gets nothing.
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o666));
        tracing::info!(
            socket = %socket_path.display(),
            "ice-helper serving (pid {})",
            std::process::id()
        );

        let runner = Arc::new(Mutex::new(ProcessCoreRunner::new()));
        // The accept loop never blocks on a peer: each connection is served
        // on its own thread (commands serialize on the runner mutex), so a
        // stalled or unauthenticated connection cannot stall the daemon. The
        // concurrent-connection cap bounds thread usage.
        let active = Arc::new(AtomicUsize::new(0));
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    if active.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                        // Fail-closed: excess connections are dropped without
                        // a frame; the app reconnects per command.
                        tracing::debug!("connection limit reached; dropping excess connection");
                        continue;
                    }
                    active.fetch_add(1, Ordering::SeqCst);
                    let config = config.clone();
                    let runner = Arc::clone(&runner);
                    let active = Arc::clone(&active);
                    std::thread::spawn(move || {
                        let result = match runner.lock() {
                            Ok(mut runner) => {
                                let auth = SocketPeerAuth;
                                serve_connection(stream, &config, &auth, &mut runner)
                            }
                            Err(_) => Err(TunError::new(
                                TunErrorCode::ApplyFailed,
                                "runner lock poisoned",
                            )),
                        };
                        active.fetch_sub(1, Ordering::SeqCst);
                        if let Err(err) = result {
                            tracing::debug!(error = %err, "connection failed");
                        }
                    });
                }
                Err(err) => {
                    tracing::error!(error = %err, "accept failed");
                }
            }
        }
    }
}

#[cfg(unix)]
mod install;

#[cfg(unix)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str);
    match mode {
        Some("install") => {
            // install <data-dir> <core-src> <allowed-uid>
            let (data_dir, core_src, allowed_uid) = match (args.get(2), args.get(3), args.get(4)) {
                (Some(dir), Some(core), Some(uid)) => match uid.parse::<u32>() {
                    Ok(uid) => (dir.clone(), core.clone(), uid),
                    Err(_) => {
                        println!(
                            "{}",
                            install::result_line(false, &format!("invalid allowed uid: {uid}"))
                        );
                        std::process::exit(1);
                    }
                },
                _ => {
                    println!(
                        "{}",
                        install::result_line(
                            false,
                            "usage: ice-helper install <data-dir> <core-src> <allowed-uid>"
                        )
                    );
                    std::process::exit(1);
                }
            };
            let result = install::install(
                std::path::Path::new(&data_dir),
                std::path::Path::new(&core_src),
                allowed_uid,
            );
            match result {
                Ok(()) => {
                    println!("{}", install::result_line(true, "helper installed"));
                    std::process::exit(0);
                }
                Err(err) => {
                    println!("{}", install::result_line(false, &err));
                    std::process::exit(1);
                }
            }
        }
        Some("uninstall") => {
            // uninstall <data-dir>
            let Some(data_dir) = args.get(2) else {
                println!(
                    "{}",
                    install::result_line(false, "usage: ice-helper uninstall <data-dir>")
                );
                std::process::exit(1);
            };
            match install::uninstall(std::path::Path::new(data_dir)) {
                Ok(()) => {
                    println!("{}", install::result_line(true, "helper uninstalled"));
                    std::process::exit(0);
                }
                Err(err) => {
                    println!("{}", install::result_line(false, &err));
                    std::process::exit(1);
                }
            }
        }
        _ => unix_main::run_daemon(),
    }
}
