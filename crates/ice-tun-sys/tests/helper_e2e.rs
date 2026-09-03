//! Host-free helper daemon end-to-end test (plan §5 T5).
//!
//! Starts the real `ice-helper` server logic (`serve_connection`) on a temp
//! Unix socket, drives it with the real client (`HelperCoreCoordinator`),
//! and proves the auth / allowlist / start / stop / status contract against
//! a fake core process. This is the shared exit gate for the helper IPC:
//! it runs on every Unix CI platform without root. The helper is a macOS
//! daemon built on Unix IPC, so the test does not compile on Windows.
#![cfg(unix)]

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ice_helper::{FixedPeerAuth, PeerAuth, ProcessCoreRunner, ServerConfig};
use ice_tun_sys::coordinator::CoreCoordinator;
use ice_tun_sys::error::TunErrorCode;
use ice_tun_sys::helper::HelperCoreCoordinator;

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ice-helper-e2e-{label}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture_core_bin(dir: &std::path::Path) -> PathBuf {
    let bin = dir.join("fake-core");
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(
        &bin,
        "#!/bin/sh\ntrap 'exit 0' TERM\nwhile :; do sleep 1; done\n",
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

struct ServerHandle {
    stop: Arc<std::sync::atomic::AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
    _socket_path: PathBuf,
}

fn spawn_server(config: Arc<ServerConfig>, socket_path: &std::path::Path) -> ServerHandle {
    let bind_path = socket_path.to_path_buf();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_flag = stop.clone();
    let join = std::thread::spawn(move || {
        let listener = UnixListener::bind(&bind_path).expect("bind");
        let auth = Arc::new(FixedPeerAuth(42));
        let runner = Arc::new(Mutex::new(ProcessCoreRunner::new(
            config.data_dir.clone(),
            config.core_bin.clone(),
        )));
        // Accept connections until the test signals shutdown; the client
        // reconnects per command.
        while !stop_flag.load(std::sync::atomic::Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let config = config.clone();
                    let auth = auth.clone();
                    let runner = runner.clone();
                    std::thread::spawn(move || {
                        let mut runner = runner.lock().unwrap();
                        let _ = ice_helper::serve_connection(stream, &config, &*auth, &mut *runner);
                    });
                }
                Err(_) => break,
            }
        }
    });
    ServerHandle {
        stop,
        join: Some(join),
        _socket_path: socket_path.to_path_buf(),
    }
}

fn stop_server(server: ServerHandle) {
    server.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    // Unblock accept() with a throwaway connection.
    let _ = UnixStream::connect(server._socket_path.as_path());
    if let Some(join) = server.join {
        join.join().unwrap();
    }
}

#[test]
fn helper_coordinator_end_to_end_start_stop() {
    let dir = temp_dir("e2e");
    let config_path = dir.join("config.json");
    std::fs::write(&config_path, b"{}").unwrap();
    let socket = dir.join("helper.sock");
    let core_bin = fixture_core_bin(&dir);

    let config = Arc::new(ServerConfig {
        token: "e2e-token".into(),
        data_dir: dir.clone(),
        core_bin,
        core_log: dir.join("core.log"),
        allowed_uid: Some(42),
    });
    let server = spawn_server(config, &socket);

    // Let the listener come up.
    for _ in 0..50 {
        if UnixStream::connect(&socket).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let mut coordinator =
        HelperCoreCoordinator::new(socket.clone(), "e2e-token".into(), dir.clone());

    // Start: returns the fake core pid.
    let pid = coordinator
        .start_with_config(&config_path)
        .expect("start via helper");
    assert!(pid > 0);

    // Status via a second start attempt must fail (already running).
    let err = coordinator
        .start_with_config(&config_path)
        .expect_err("second start must be rejected");
    assert_eq!(err.code, TunErrorCode::ApplyFailed);

    // Stop: idempotent, removes the core.
    coordinator.stop().expect("stop via helper");
    coordinator.stop().expect("stop is idempotent");

    stop_server(server);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Regression: the daemon replies to a Stop only after its own TERM→KILL
/// grace (5 s TERM + 2 s KILL in `ice-helper`) has finished, so a client
/// read bound shorter than that would abort the roundtrip while the daemon
/// is still killing a slow-to-exit core — falsely failing a TUN disable /
/// reclaim and forcing a fail-closed RecoveryRequired on a stop that
/// succeeds moments later. The Stop client must wait for the daemon's full
/// grace (this fixture core takes ~3.5 s to exit after SIGTERM, beyond the
/// generic 3 s IPC timeout).
#[test]
fn stop_waiting_for_slow_core_does_not_time_out() {
    let dir = temp_dir("slow");
    let config_path = dir.join("config.json");
    std::fs::write(&config_path, b"{}").unwrap();
    let socket = dir.join("helper.sock");
    let core_bin = dir.join("fake-core");
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(
        &core_bin,
        "#!/bin/sh\ntrap 'sleep 3.5; exit 0' TERM\nwhile :; do sleep 0.2; done\n",
    )
    .unwrap();
    std::fs::set_permissions(&core_bin, std::fs::Permissions::from_mode(0o755)).unwrap();

    let config = Arc::new(ServerConfig {
        token: "e2e-token".into(),
        data_dir: dir.clone(),
        core_bin,
        core_log: dir.join("core.log"),
        allowed_uid: Some(42),
    });
    let server = spawn_server(config, &socket);
    for _ in 0..50 {
        if UnixStream::connect(&socket).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let mut coordinator =
        HelperCoreCoordinator::new(socket.clone(), "e2e-token".into(), dir.clone());
    coordinator
        .start_with_config(&config_path)
        .expect("start via helper");

    // The core does not exit for ~3.5 s after SIGTERM; the Stop roundtrip
    // must stay open past the generic IPC timeout and wait for the daemon's
    // TERM grace instead of aborting.
    let started = std::time::Instant::now();
    coordinator
        .stop()
        .expect("stop must wait for the daemon grace");
    assert!(
        started.elapsed() >= Duration::from_millis(3000),
        "the slow core must keep the Stop roundtrip open past the generic IPC timeout"
    );

    stop_server(server);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn helper_rejects_unauthorized_token() {
    let dir = temp_dir("auth");
    let config_path = dir.join("config.json");
    std::fs::write(&config_path, b"{}").unwrap();
    let socket = dir.join("helper.sock");
    let core_bin = fixture_core_bin(&dir);

    let config = Arc::new(ServerConfig {
        token: "right-token".into(),
        data_dir: dir.clone(),
        core_bin,
        core_log: dir.join("core.log"),
        allowed_uid: Some(42),
    });
    let server = spawn_server(config, &socket);
    for _ in 0..50 {
        if UnixStream::connect(&socket).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let mut coordinator =
        HelperCoreCoordinator::new(socket.clone(), "wrong-token".into(), dir.clone());
    let err = coordinator
        .start_with_config(&config_path)
        .expect_err("wrong token must fail");
    assert_eq!(err.code, TunErrorCode::PermissionRequired);

    stop_server(server);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn helper_rejects_config_outside_data_dir_before_ipc() {
    let dir = temp_dir("outside");
    let socket = dir.join("helper.sock");
    let core_bin = fixture_core_bin(&dir);
    let config = Arc::new(ServerConfig {
        token: "tok".into(),
        data_dir: dir.clone(),
        core_bin,
        core_log: dir.join("core.log"),
        allowed_uid: Some(42),
    });
    let server = spawn_server(config, &socket);
    for _ in 0..50 {
        if UnixStream::connect(&socket).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let mut coordinator = HelperCoreCoordinator::new(socket.clone(), "tok".into(), dir.clone());
    let err = coordinator
        .start_with_config(std::path::Path::new("/etc/hosts"))
        .expect_err("outside path must fail");
    assert_eq!(err.code, TunErrorCode::PermissionRequired);

    stop_server(server);
    let _ = std::fs::remove_dir_all(&dir);
}

#[allow(dead_code)]
fn _unused(_: &dyn PeerAuth) {}
