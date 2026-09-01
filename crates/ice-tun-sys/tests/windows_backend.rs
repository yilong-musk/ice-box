//! Windows backend tests (plan §5 T2 shared exit gate; `windows_tun_ready`
//! pending — host-free on every CI platform).
//!
//! The backend logic runs against a fake `WindowsHost` (simulated `netsh` /
//! `route print` state) and a fake `CoreCoordinator` that starts/stops the
//! "elevated core" by mutating the same fake host. Proves: prepare validation
//! and adapter-name collision fallback; journaled apply with rollback on
//! journal-write failure; verify semantics (index identity, exact addresses,
//! full-route lock, control path); fail-closed restore; kill residue
//! recovery; and the dev-runner factory wiring.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use ice_tun_sys::backend::RecoveryOutcome;
use ice_tun_sys::coordinator::CoreCoordinator;
use ice_tun_sys::error::{TunError, TunErrorCode};
use ice_tun_sys::journal::{steps, JournalState, TunJournal};
use ice_tun_sys::routes;
use ice_tun_sys::windows::{
    parse_netsh_interfaces, WindowsInterfaceState, WindowsTunBackend, DEFAULT_WINTUN_NAME,
};
#[cfg(target_os = "windows")]
use ice_tun_sys::WindowsHost;
use ice_tun_sys::{create_backend, AppliedTun, TunBackend, TunConfig, TunStack};

const OWNER: &str = "ice-box:test-install-1";
/// Fake adapter interface index (the Windows identity token).
const FAKE_INDEX: u32 = 17;

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ice-tun-windows-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn journal_path(dir: &std::path::Path) -> PathBuf {
    dir.join("tun-state.json")
}

fn config_path(dir: &std::path::Path) -> PathBuf {
    dir.join("config.json")
}

fn write_tun_config(dir: &std::path::Path, addresses: &[&str]) -> PathBuf {
    let path = config_path(dir);
    fs::write(
        &path,
        serde_json::json!({
            "inbounds": [
                { "type": "mixed", "tag": "mixed-in" },
                {
                    "type": "tun",
                    "tag": "tun-in",
                    "interface_name": "Wintun",
                    "address": addresses,
                    "auto_route": true,
                    "strict_route": true
                }
            ]
        })
        .to_string(),
    )
    .unwrap();
    path
}

fn win_config() -> TunConfig {
    TunConfig {
        interface_name: Some(DEFAULT_WINTUN_NAME.into()),
        addresses: vec!["10.0.0.1/30".into(), "fdfe:dcba:9876::1/126".into()],
        mtu: 9000,
        stack: TunStack::Gvisor,
        auto_route: true,
        strict_route: true,
        dns_hijack: false,
    }
}

/// Destinations the fake core installs for the adapter (the sample set the
/// backend probes; the real set is a T0 spike item).
fn fake_owned_destinations() -> Vec<&'static str> {
    vec![
        "10.0.0.0/8",
        "172.16.0.0/12",
        "192.168.0.0/16",
        "100.64.0.0/10",
        "fdfe:dcba:9876::/126",
    ]
}

/// Simulated OS state: interfaces + route table (destination → interface
/// identity: an IP for v4 routes, the interface index as a string for v6).
#[derive(Default)]
struct HostState {
    interfaces: Vec<(String, WindowsInterfaceState)>,
    routes: Vec<(String, String)>,
}

/// Fake `WindowsHost` sharing one `HostState` with the fake coordinator.
#[derive(Clone, Default)]
struct FakeHost {
    state: Arc<Mutex<HostState>>,
}

impl FakeHost {
    fn add_wintun(&self, name: &str, addresses: &[String]) {
        let mut state = self.state.lock().unwrap();
        state.interfaces.push((
            name.to_string(),
            WindowsInterfaceState {
                up: true,
                addresses: addresses.to_vec(),
                index: Some(FAKE_INDEX),
            },
        ));
        let v4_ip = addresses
            .iter()
            .find(|address| !address.contains(':'))
            .map(|address| routes::address_key(address).to_string())
            .unwrap_or_else(|| "10.0.0.1".to_string());
        for destination in fake_owned_destinations() {
            let identity = if destination.contains(':') {
                FAKE_INDEX.to_string()
            } else {
                v4_ip.clone()
            };
            state.routes.push((destination.to_string(), identity));
        }
        // Loopback always resolves via 127.0.0.1, never via the tun.
        state
            .routes
            .push(("127.0.0.0/8".to_string(), "127.0.0.1".to_string()));
    }

    fn remove_wintun(&self, name: &str) {
        let mut state = self.state.lock().unwrap();
        state.interfaces.retain(|(existing, _)| existing != name);
        state.routes.retain(|(_, identity)| identity == "127.0.0.1");
    }

    /// Hard-kill clean model: the adapter disappears and the kernel flushes
    /// its routes with it (the macOS kill-9 behavior; the Windows spike must
    /// confirm whether Windows leaves residue).
    fn simulate_kill_clean(&self, name: &str) {
        let mut state = self.state.lock().unwrap();
        state.interfaces.retain(|(existing, _)| existing != name);
        state.routes.retain(|(_, identity)| identity == "127.0.0.1");
    }

    /// Residue model: the adapter is gone but owned routes survive (what the
    /// Windows spike must check — the journal + recovery handle either way).
    fn simulate_adapter_gone_routes_remain(&self, name: &str) {
        let mut state = self.state.lock().unwrap();
        state.interfaces.retain(|(existing, _)| existing != name);
    }

    fn has_wintun(&self, name: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .interfaces
            .iter()
            .any(|(existing, _)| existing == name)
    }

    fn resolve(&self, query: &str) -> Option<String> {
        let (addr, _) = query.split_once('/').unwrap_or((query, ""));
        if addr.parse::<std::net::Ipv4Addr>().is_err()
            && addr.parse::<std::net::Ipv6Addr>().is_err()
        {
            return None;
        }
        let state = self.state.lock().unwrap();
        let table: Vec<(String, u32)> = state
            .routes
            .iter()
            .map(|(dest, _)| {
                let (net, bits) = dest
                    .split_once('/')
                    .map(|(n, p)| (n.to_string(), p.parse::<u32>().unwrap_or(0)))
                    .unwrap_or((dest.clone(), 0));
                (net, bits)
            })
            .collect();
        let index = routes::longest_prefix_route(&table, addr)?;
        Some(state.routes[index].1.clone())
    }
}

impl ice_tun_sys::WindowsHost for FakeHost {
    fn list_interface_names(&self) -> Result<Vec<String>, TunError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .interfaces
            .iter()
            .map(|(name, _)| name.clone())
            .collect())
    }

    fn interface_state(&self, name: &str) -> Result<Option<WindowsInterfaceState>, TunError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .interfaces
            .iter()
            .find(|(existing, _)| existing == name)
            .map(|(_, state)| state.clone()))
    }

    fn route_interface(&self, destination: &str) -> Result<Option<String>, TunError> {
        Ok(self.resolve(destination))
    }
}

/// Fake elevated-core coordinator: "starts" sing-box by creating the Wintun
/// adapter on the shared host (addresses read from the config file, mirroring
/// the real flow) and "stops" it by removing it — unless `remove_on_stop` is
/// false (a core that refuses to die / a stuck helper).
struct FakeCoreCoordinator {
    host: FakeHost,
    start_failure: Option<TunErrorCode>,
    stop_failure: Option<TunErrorCode>,
    remove_on_stop: bool,
    /// When false, the adapter is created without its routes (a core that
    /// never converged): apply must fail closed.
    install_routes: bool,
    started: bool,
}

impl FakeCoreCoordinator {
    fn new(host: FakeHost) -> Self {
        Self {
            host,
            start_failure: None,
            stop_failure: None,
            remove_on_stop: true,
            install_routes: true,
            started: false,
        }
    }

    fn tun_addresses(config_path: &Path) -> Result<Vec<String>, TunError> {
        let raw = fs::read_to_string(config_path).map_err(|err| {
            TunError::new(TunErrorCode::ApplyFailed, format!("read config: {err}"))
        })?;
        let value: serde_json::Value = serde_json::from_str(&raw).map_err(|err| {
            TunError::new(TunErrorCode::ApplyFailed, format!("parse config: {err}"))
        })?;
        value
            .get("inbounds")
            .and_then(|v| v.as_array())
            .and_then(|inbounds| {
                inbounds
                    .iter()
                    .find(|i| i.get("type").and_then(|v| v.as_str()) == Some("tun"))
            })
            .and_then(|tun| tun.get("address").and_then(|v| v.as_array()))
            .map(|addresses| {
                addresses
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .ok_or_else(|| {
                TunError::new(
                    TunErrorCode::ApplyFailed,
                    "config has no tun inbound with addresses",
                )
            })
    }
}

impl CoreCoordinator for FakeCoreCoordinator {
    fn start_with_config(&mut self, config_path: &Path) -> Result<u32, TunError> {
        if let Some(code) = self.start_failure {
            return Err(TunError::new(code, "injected start failure"));
        }
        let addresses = Self::tun_addresses(config_path)?;
        if self.install_routes {
            self.host.add_wintun(DEFAULT_WINTUN_NAME, &addresses);
        } else {
            // Adapter without its route table: the backend must not claim it.
            let mut state = self.host.state.lock().unwrap();
            state.interfaces.push((
                DEFAULT_WINTUN_NAME.to_string(),
                WindowsInterfaceState {
                    up: true,
                    addresses: addresses.clone(),
                    index: Some(FAKE_INDEX),
                },
            ));
            state
                .routes
                .push(("127.0.0.0/8".to_string(), "127.0.0.1".to_string()));
        }
        self.started = true;
        Ok(4242)
    }

    fn stop(&mut self) -> Result<(), TunError> {
        if let Some(code) = self.stop_failure {
            return Err(TunError::new(code, "injected stop failure"));
        }
        if self.remove_on_stop {
            self.host.remove_wintun(DEFAULT_WINTUN_NAME);
        }
        self.started = false;
        Ok(())
    }

    fn set_dns(&mut self, _service: &str, _servers: &[String]) -> Result<(), TunError> {
        Err(TunError::new(
            TunErrorCode::ApplyFailed,
            "dns not supported by the windows fake coordinator",
        ))
    }
}

fn backend(
    dir: &std::path::Path,
    host: FakeHost,
    coordinator: FakeCoreCoordinator,
) -> WindowsTunBackend {
    WindowsTunBackend::new(
        OWNER,
        Box::new(host),
        Box::new(coordinator),
        config_path(dir),
    )
    .with_journal(journal_path(dir))
}

fn seed_preparing_journal(dir: &std::path::Path) {
    let mut journal = TunJournal::new("t-enable".into(), OWNER.into());
    journal
        .record(
            &journal_path(dir),
            JournalState::Preparing,
            steps::JOURNAL_PREPARING,
            |_| {},
        )
        .expect("journal preparing");
}

fn apply_ok(
    dir: &std::path::Path,
    host: &FakeHost,
    coordinator: FakeCoreCoordinator,
) -> AppliedTun {
    seed_preparing_journal(dir);
    write_tun_config(dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let prepared = backend(dir, host.clone(), coordinator)
        .prepare(&win_config())
        .expect("prepare");
    backend(dir, host.clone(), FakeCoreCoordinator::new(host.clone()))
        .apply(&prepared)
        .expect("apply")
}

// --- prepare ---

#[test]
fn prepare_resolves_default_name_when_free() {
    let dir = temp_dir("prepare-none");
    let host = FakeHost::default();
    let prepared = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()))
        .prepare(&TunConfig {
            interface_name: None,
            ..win_config()
        })
        .expect("default name");
    assert_eq!(
        prepared.config.interface_name.as_deref(),
        Some(DEFAULT_WINTUN_NAME)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn prepare_uses_requested_name_when_free() {
    let dir = temp_dir("prepare-name");
    let host = FakeHost::default();
    let prepared = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()))
        .prepare(&win_config())
        .expect("prepare");
    assert_eq!(prepared.config.interface_name.as_deref(), Some("Wintun"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn prepare_collision_probes_numbered_variant() {
    let dir = temp_dir("prepare-collision");
    let host = FakeHost::default();
    host.add_wintun("Wintun", &["10.0.0.1/30".into()]);
    let prepared = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()))
        .prepare(&win_config())
        .expect("collision fallback");
    assert_eq!(
        prepared.config.interface_name.as_deref(),
        Some("Wintun 2"),
        "occupied name must fall back to a numbered variant"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn prepare_rejects_ipv4_only_and_missing_ipv4() {
    let dir = temp_dir("prepare-families");
    let host = FakeHost::default();
    let bk = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()));

    let ipv4_only = TunConfig {
        addresses: vec!["10.0.0.1/30".into()],
        ..win_config()
    };
    let err = bk.prepare(&ipv4_only).expect_err("ipv4-only leaks IPv6");
    assert_eq!(err.code, TunErrorCode::ApplyFailed);
    assert!(err.message.contains("IPv6"));

    let ipv6_only = TunConfig {
        addresses: vec!["fdfe:dcba:9876::1/126".into()],
        ..win_config()
    };
    let err = bk.prepare(&ipv6_only).expect_err("ipv4 is mandatory");
    assert_eq!(err.code, TunErrorCode::ApplyFailed);
    assert!(err.message.contains("IPv4"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn prepare_rejects_bad_addresses_mtu_and_interface_name() {
    let dir = temp_dir("prepare-bad");
    let host = FakeHost::default();
    let bk = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()));

    for cidr in ["10.0.0.1", "10.0.0.1/33", "10.0.0.1/0", "999.1.1.1/24"] {
        let bad = TunConfig {
            addresses: vec![cidr.into(), "fdfe:dcba:9876::1/126".into()],
            ..win_config()
        };
        let err = bk.prepare(&bad).expect_err("bad cidr");
        assert_eq!(err.code, TunErrorCode::ApplyFailed, "case: {cidr}");
    }
    for cidr in ["fdfe:dcba:9876::1", "fdfe:dcba:9876::1/129", "10.0.0.1/24"] {
        let bad = TunConfig {
            addresses: vec!["10.0.0.1/30".into(), cidr.into()],
            ..win_config()
        };
        let err = bk.prepare(&bad).expect_err("bad v6 cidr");
        assert_eq!(err.code, TunErrorCode::ApplyFailed, "case: {cidr}");
    }

    let low_mtu = TunConfig {
        mtu: 576,
        ..win_config()
    };
    assert_eq!(
        bk.prepare(&low_mtu).expect_err("low mtu").code,
        TunErrorCode::ApplyFailed
    );

    for name in ["bad/name", "bad:name", "bad*name", "bad\nname"] {
        let bad_name = TunConfig {
            interface_name: Some(name.into()),
            ..win_config()
        };
        let err = bk.prepare(&bad_name).expect_err("bad adapter name");
        assert_eq!(err.code, TunErrorCode::ApplyFailed, "case: {name}");
    }
    let _ = fs::remove_dir_all(&dir);
}

// --- apply / verify ---

#[test]
fn apply_journals_granular_steps_and_returns_observed_ownership() {
    let dir = temp_dir("apply-ok");
    let host = FakeHost::default();
    seed_preparing_journal(&dir);
    write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);

    let coordinator = FakeCoreCoordinator::new(host.clone());
    let mut bk = backend(&dir, host.clone(), coordinator);
    let prepared = bk.prepare(&win_config()).expect("prepare");
    let applied = bk.apply(&prepared).expect("apply");

    assert_eq!(applied.interface_name.as_deref(), Some("Wintun"));
    assert_eq!(applied.interface_id.as_deref(), Some("17"));
    assert_eq!(
        applied.addresses,
        win_config()
            .addresses
            .iter()
            .map(|cidr| ice_tun_sys::CidrRecord {
                cidr: cidr.clone(),
                owned: true
            })
            .collect::<Vec<_>>()
    );
    assert!(
        !applied.routes.is_empty() && applied.routes.iter().all(|r| r.owned),
        "every observed route is recorded as owned"
    );
    assert!(
        applied.dns_before.is_none() && applied.dns_after.is_none(),
        "Windows DNS ownership is an open spike item; nothing claimed yet"
    );

    let journal = TunJournal::load(&journal_path(&dir))
        .unwrap()
        .expect("journal");
    assert_eq!(journal.last_completed_step, steps::ROUTES_ADDED);
    assert_eq!(journal.interface_name.as_deref(), Some("Wintun"));
    assert_eq!(journal.interface_id.as_deref(), Some("17"));
    assert_eq!(journal.addresses, applied.addresses);
    assert_eq!(journal.routes, applied.routes);

    let health = bk.verify(&applied).expect("verify");
    assert!(
        health.all_ok(),
        "applied capture must be healthy: {health:?}"
    );
    assert!(!health.nothing_owned);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn apply_propagates_permission_required_without_records() {
    let dir = temp_dir("apply-permission");
    let host = FakeHost::default();
    seed_preparing_journal(&dir);
    write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);

    let mut coordinator = FakeCoreCoordinator::new(host.clone());
    coordinator.start_failure = Some(TunErrorCode::PermissionRequired);
    let mut bk = backend(&dir, host.clone(), coordinator);
    let prepared = bk.prepare(&win_config()).expect("prepare");
    let err = bk.apply(&prepared).expect_err("permission required");
    assert_eq!(err.code, TunErrorCode::PermissionRequired);

    let journal = TunJournal::load(&journal_path(&dir))
        .unwrap()
        .expect("journal");
    assert_eq!(
        journal.last_completed_step,
        steps::JOURNAL_PREPARING,
        "no granular mutation records may exist after a refused start"
    );
    assert!(!host.has_wintun("Wintun"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn apply_journal_write_failure_stops_core_and_rolls_back() {
    let dir = temp_dir("apply-rollback");
    // Force every journal write to fail: the journal path's parent is a file.
    let blocker = dir.join("blocker");
    fs::write(&blocker, b"x").unwrap();
    let broken_journal = blocker.join("tun-state.json");

    let host = FakeHost::default();
    let coordinator = FakeCoreCoordinator::new(host.clone());
    let mut bk = WindowsTunBackend::new(
        OWNER,
        Box::new(host.clone()),
        Box::new(coordinator),
        config_path(&dir),
    )
    .with_journal(broken_journal);
    write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let prepared = bk.prepare(&win_config()).expect("prepare");
    let err = bk.apply(&prepared).expect_err("journal write failure");
    assert_eq!(err.code, TunErrorCode::ApplyFailed);

    assert!(
        !host.has_wintun("Wintun"),
        "an unjournaled resource must not survive: the core must be stopped"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn apply_journal_write_and_stop_failure_is_recovery_required() {
    let dir = temp_dir("apply-rollback-uncertain");
    let blocker = dir.join("blocker");
    fs::write(&blocker, b"x").unwrap();
    let broken_journal = blocker.join("tun-state.json");

    let host = FakeHost::default();
    let mut coordinator = FakeCoreCoordinator::new(host.clone());
    coordinator.stop_failure = Some(TunErrorCode::RestoreFailed);
    let mut bk = WindowsTunBackend::new(
        OWNER,
        Box::new(host.clone()),
        Box::new(coordinator),
        config_path(&dir),
    )
    .with_journal(broken_journal);
    write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let prepared = bk.prepare(&win_config()).expect("prepare");
    let err = bk.apply(&prepared).expect_err("uncertain cleanup");
    assert_eq!(err.code, TunErrorCode::RecoveryRequired);
    assert!(
        host.has_wintun("Wintun"),
        "stuck core leaves ownership uncertain"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn verify_reports_missing_interface_as_not_owned() {
    let dir = temp_dir("verify-missing");
    let host = FakeHost::default();
    let applied = apply_ok(&dir, &host, FakeCoreCoordinator::new(host.clone()));
    let bk = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()));

    let health = bk.verify(&applied).expect("verify");
    assert!(health.all_ok());

    host.simulate_kill_clean("Wintun");
    let health = bk.verify(&applied).expect("verify after kill");
    assert!(!health.interface_up);
    assert!(!health.routes_owned);
    assert!(
        health.nothing_owned,
        "kernel flushed the routes with the interface; nothing owned"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn verify_orphaned_routes_keep_nothing_owned_false() {
    let dir = temp_dir("verify-orphaned");
    let host = FakeHost::default();
    let applied = apply_ok(&dir, &host, FakeCoreCoordinator::new(host.clone()));
    let bk = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()));

    host.simulate_adapter_gone_routes_remain("Wintun");
    let health = bk.verify(&applied).expect("verify with orphaned routes");
    assert!(!health.interface_up);
    assert!(
        health.routes_owned,
        "owned routes still resolve to the (gone) adapter identity"
    );
    assert!(
        !health.nothing_owned,
        "orphaned routes mean cleanup is NOT verified"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn apply_fails_closed_when_routes_do_not_converge() {
    let dir = temp_dir("apply-noroutes");
    let host = FakeHost::default();
    seed_preparing_journal(&dir);
    write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);

    let mut coordinator = FakeCoreCoordinator::new(host.clone());
    coordinator.install_routes = false; // the adapter appears without routes
    let mut bk = backend(&dir, host.clone(), coordinator);
    let prepared = bk.prepare(&win_config()).expect("prepare");
    let err = bk.apply(&prepared).expect_err("routes never converge");
    assert_eq!(err.code, TunErrorCode::HealthcheckFailed);
    assert!(
        !host.has_wintun("Wintun"),
        "the core must be stopped when the capture does not converge"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn verify_rejects_missing_required_address_family() {
    let dir = temp_dir("verify-missing-v6");
    let host = FakeHost::default();
    let applied = apply_ok(&dir, &host, FakeCoreCoordinator::new(host.clone()));
    let bk = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()));

    assert!(bk.verify(&applied).expect("verify before").all_ok());

    // The interface silently lost its IPv6 address: the exact-address lock
    // must reject the capture.
    let mut state = host.state.lock().unwrap();
    state.interfaces[0]
        .1
        .addresses
        .retain(|addr| !addr.contains(':'));
    drop(state);
    let health = bk.verify(&applied).expect("verify after v6 loss");
    assert!(!health.addresses_present);
    assert!(!health.all_ok());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn verify_rejects_partial_route_loss() {
    let dir = temp_dir("verify-route-loss");
    let host = FakeHost::default();
    let applied = apply_ok(&dir, &host, FakeCoreCoordinator::new(host.clone()));
    let bk = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()));

    assert!(bk.verify(&applied).expect("verify before").all_ok());

    // One owned route no longer resolves to the adapter: the full-route lock
    // must reject the capture.
    host.state
        .lock()
        .unwrap()
        .routes
        .retain(|(dest, _)| dest != "10.0.0.0/8");
    let health = bk.verify(&applied).expect("verify after route loss");
    assert!(!health.routes_owned);
    assert!(!health.all_ok());
    let _ = fs::remove_dir_all(&dir);
}

// --- restore / recover ---

#[test]
fn restore_cleans_and_journals_removal_steps() {
    let dir = temp_dir("restore-ok");
    let host = FakeHost::default();
    let coordinator = FakeCoreCoordinator::new(host.clone());
    let applied = apply_ok(&dir, &host, coordinator);

    let mut bk = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()));
    bk.restore(&applied).expect("restore");

    let journal = TunJournal::load(&journal_path(&dir))
        .unwrap()
        .expect("journal");
    assert_eq!(journal.last_completed_step, steps::INTERFACE_REMOVED);
    assert!(journal.interface_name.is_none());
    assert!(journal.routes.is_empty());
    assert!(!host.has_wintun("Wintun"));

    let health = bk.verify(&applied).expect("verify after restore");
    assert!(health.nothing_owned);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn restore_fails_closed_when_interface_survives_stop() {
    let dir = temp_dir("restore-stuck");
    let host = FakeHost::default();
    let applied = apply_ok(&dir, &host, FakeCoreCoordinator::new(host.clone()));

    let mut coordinator = FakeCoreCoordinator::new(host.clone());
    coordinator.remove_on_stop = false; // the core/helper refuses to release
    let mut bk = backend(&dir, host.clone(), coordinator);
    let err = bk.restore(&applied).expect_err("interface survives stop");
    assert_eq!(
        err.code,
        TunErrorCode::RecoveryRequired,
        "uncertain cleanup must fail closed, not claim success"
    );
    assert!(
        host.has_wintun("Wintun"),
        "unverified resource is never deleted"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn recover_kill_cleaned_by_kernel() {
    let dir = temp_dir("recover-kill");
    let host = FakeHost::default();
    let coordinator = FakeCoreCoordinator::new(host.clone());
    let _applied = apply_ok(&dir, &host, coordinator);
    host.simulate_kill_clean("Wintun");

    let mut bk = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()));
    let journal = TunJournal::load(&journal_path(&dir))
        .unwrap()
        .expect("journal");
    let outcome = bk.recover(&journal).expect("recover");
    assert_eq!(
        outcome,
        RecoveryOutcome::Cleaned,
        "kernel already flushed the routes with the interface; verification alone proves cleanup"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn recover_with_orphaned_routes_converges_to_clean() {
    let dir = temp_dir("recover-orphaned");
    let host = FakeHost::default();
    let coordinator = FakeCoreCoordinator::new(host.clone());
    let _applied = apply_ok(&dir, &host, coordinator);
    host.simulate_adapter_gone_routes_remain("Wintun");

    let mut bk = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()));
    let journal = TunJournal::load(&journal_path(&dir))
        .unwrap()
        .expect("journal");
    let outcome = bk.recover(&journal).expect("recover");
    assert_eq!(
        outcome,
        RecoveryOutcome::Cleaned,
        "recover runs the idempotent release (stop core) and the fake flushes the orphaned routes"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn recover_stops_core_and_converges_to_clean() {
    let dir = temp_dir("recover-stop");
    let host = FakeHost::default();
    let coordinator = FakeCoreCoordinator::new(host.clone());
    let _applied = apply_ok(&dir, &host, coordinator);
    assert!(
        host.has_wintun("Wintun"),
        "precondition: capture still applied"
    );

    let mut bk = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()));
    let journal = TunJournal::load(&journal_path(&dir))
        .unwrap()
        .expect("journal");
    let outcome = bk.recover(&journal).expect("recover");
    assert_eq!(
        outcome,
        RecoveryOutcome::Cleaned,
        "recover must release the owned capture (stop core) then verify clean"
    );
    assert!(!host.has_wintun("Wintun"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn recover_never_enables_capture() {
    let dir = temp_dir("recover-noop");
    let host = FakeHost::default();
    let coordinator = FakeCoreCoordinator::new(host.clone());
    let _ = apply_ok(&dir, &host, coordinator);

    let mut bk = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()));
    let journal = TunJournal::load(&journal_path(&dir))
        .unwrap()
        .expect("journal");
    bk.recover(&journal).expect("recover");

    let journal = TunJournal::load(&journal_path(&dir))
        .unwrap()
        .expect("journal");
    assert_ne!(
        journal.state,
        JournalState::Applied,
        "recovery never re-enables capture"
    );
    let _ = fs::remove_dir_all(&dir);
}

// --- factory / dev-runner gating ---

#[test]
fn create_backend_capability_matches_the_platform_gate() {
    let backend = create_backend(
        OWNER,
        PathBuf::from("/tmp/ice-box-test-config.json"),
        None,
        PathBuf::from("/tmp/ice-box-test.log"),
    );
    let capability = backend.capability();
    #[cfg(target_os = "macos")]
    {
        assert!(capability.supported, "macos_tun_ready: gate green");
    }
    #[cfg(target_os = "windows")]
    {
        // The dev opt-in is not set in this test; production stays
        // fail-closed until `windows_tun_ready`.
        assert!(
            !capability.supported,
            "windows_tun_ready pending: fail closed without the dev opt-in"
        );
        assert!(capability.reason.is_some());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        assert!(!capability.supported, "unsupported host: fail closed");
        assert!(capability.reason.is_some());
    }
}

#[test]
fn windows_backend_capability_reports_planned_shape() {
    let host = FakeHost::default();
    let bk = WindowsTunBackend::new(
        OWNER,
        Box::new(host.clone()),
        Box::new(FakeCoreCoordinator::new(host)),
        PathBuf::from("/tmp/config.json"),
    );
    let capability = bk.capability();
    assert!(capability.supported);
    assert!(capability.ipv4 && capability.ipv6);
    assert!(
        !capability.dns_hijack,
        "Windows DNS ownership is an open spike item"
    );
}

#[test]
fn netsh_interfaces_parser_is_shared_and_stable() {
    let output = "\
Idx     Met         MTU          State          Name
---------------------------------------------------------------------------
  1          75        4294967295  connected     Loopback Pseudo-Interface 1
 17          25          9000  connected     Wintun
";
    let parsed = parse_netsh_interfaces(output);
    assert_eq!(parsed[1], (17, "Wintun".to_string()));
}

#[cfg(target_os = "windows")]
#[test]
#[ignore = "live host reads + elevation; run on a real Windows host via scripts/run-acceptance-windows-tun.sh"]
fn live_process_host_reads_match_a_real_host() {
    let host = ice_tun_sys::ProcessWindowsHost;
    let names = host.list_interface_names().expect("netsh interfaces");
    assert!(
        names.len() >= 2,
        "a real Windows host has several interfaces"
    );
    let loopback_route = host.route_interface("127.0.0.1").expect("route print");
    assert_ne!(
        loopback_route.as_deref(),
        None,
        "loopback always has a route"
    );
}
