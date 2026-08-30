//! macOS backend tests (plan §5 T2 shared exit gate).
//!
//! Host-free on every CI platform: the backend logic runs against a fake
//! `MacOsHost` (simulated `ifconfig` / `route` state) and a fake
//! `CoreCoordinator` that starts/stops the "elevated core" by mutating the
//! same fake host. Proves: prepare validation + utun collision fallback,
//! journaled apply with rollback on journal-write failure, verify semantics,
//! fail-closed restore, kill-9 recovery, and the unsupported/factory paths.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use ice_tun_sys::backend::RecoveryOutcome;
use ice_tun_sys::coordinator::CoreCoordinator;
use ice_tun_sys::error::{TunError, TunErrorCode};
use ice_tun_sys::journal::{steps, JournalState, TunJournal};
use ice_tun_sys::macos::{MacInterfaceState, MacOsHost};
use ice_tun_sys::routes;
#[cfg(target_os = "macos")]
use ice_tun_sys::ProcessMacOsHost;
use ice_tun_sys::{
    create_backend, AppliedTun, MacosTunBackend, PreparedTun, TunBackend, TunConfig, TunStack,
    UnsupportedTunBackend,
};

const OWNER: &str = "ice-box:test-install-1";

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ice-tun-macos-{label}-{}",
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

fn mac_config() -> TunConfig {
    TunConfig {
        interface_name: Some("utun420".into()),
        addresses: vec!["10.0.0.1/30".into(), "fdfe:dcba:9876::1/126".into()],
        mtu: 9000,
        stack: TunStack::Gvisor,
        auto_route: true,
        strict_route: true,
        dns_hijack: false,
    }
}

/// Simulated OS state: interfaces + route table (destination → interface).
#[derive(Default)]
struct HostState {
    interfaces: Vec<(String, MacInterfaceState)>,
    routes: HashMap<String, String>,
}

/// Fake `MacOsHost` sharing one `HostState` with the fake coordinator.
#[derive(Clone, Default)]
struct FakeHost {
    state: Arc<Mutex<HostState>>,
}

impl FakeHost {
    fn add_utun(&self, name: &str, addresses: &[String]) {
        let mut state = self.state.lock().unwrap();
        state.interfaces.push((
            name.to_string(),
            MacInterfaceState {
                up: true,
                addresses: addresses.to_vec(),
            },
        ));
        let config = TunConfig {
            interface_name: Some(name.to_string()),
            addresses: addresses.to_vec(),
            mtu: 9000,
            stack: TunStack::Gvisor,
            auto_route: true,
            strict_route: true,
            dns_hijack: false,
        };
        for destination in routes::auto_route_destinations(&config) {
            state.routes.insert(destination, name.to_string());
        }
        // Loopback always resolves via lo0, never via the tun.
        state.routes.insert("127.0.0.1".into(), "lo0".into());
    }

    fn remove_utun(&self, name: &str) {
        let mut state = self.state.lock().unwrap();
        state.interfaces.retain(|(existing, _)| existing != name);
        state.routes.retain(|_, iface| iface != name);
    }

    /// `kill -9` residue model: the kernel removes the interface and flushes
    /// its routes with the fd close (T0 spike, live-confirmed on macOS).
    fn simulate_kill9(&self) {
        self.remove_utun("utun420");
    }

    fn has_utun(&self, name: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .interfaces
            .iter()
            .any(|(existing, _)| existing == name)
    }
}

impl MacOsHost for FakeHost {
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

    fn interface_state(&self, name: &str) -> Result<Option<MacInterfaceState>, TunError> {
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
        Ok(self.state.lock().unwrap().routes.get(destination).cloned())
    }
}

/// Fake elevated-core coordinator: "starts" sing-box by creating the utun on
/// the shared host (addresses read from the config file, mirroring the real
/// flow) and "stops" it by removing it — unless `remove_on_stop` is false
/// (a core that refuses to die / a stuck helper).
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
            self.host.add_utun("utun420", &addresses);
        } else {
            // Adapter without its route table: the backend must not claim it.
            let mut state = self.host.state.lock().unwrap();
            state.interfaces.push((
                "utun420".to_string(),
                MacInterfaceState {
                    up: true,
                    addresses: addresses.clone(),
                },
            ));
            state.routes.insert("127.0.0.1".into(), "lo0".into());
        }
        self.started = true;
        Ok(4242)
    }

    fn stop(&mut self) -> Result<(), TunError> {
        if let Some(code) = self.stop_failure {
            return Err(TunError::new(code, "injected stop failure"));
        }
        // The real runner always terminates the core; whether the adapter
        // disappears is host behavior, so the fake removes it unconditionally
        // (unless `remove_on_stop` simulates a stuck core/helper).
        if self.remove_on_stop {
            self.host.remove_utun("utun420");
        }
        self.started = false;
        Ok(())
    }
}

fn backend(
    dir: &std::path::Path,
    host: FakeHost,
    coordinator: FakeCoreCoordinator,
) -> MacosTunBackend {
    MacosTunBackend::new(
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
        .prepare(&mac_config())
        .expect("prepare");
    backend(dir, host.clone(), FakeCoreCoordinator::new(host.clone()))
        .apply(&prepared)
        .expect("apply")
}

// --- prepare ---

#[test]
fn prepare_resolves_free_utun_when_unspecified() {
    let dir = temp_dir("prepare-none");
    let host = FakeHost::default();
    host.add_utun("utun0", &["1.2.3.4/32".into()]);
    host.add_utun("utun5", &["1.2.3.5/32".into()]);
    let prepared = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()))
        .prepare(&TunConfig {
            interface_name: None,
            ..mac_config()
        })
        .expect("probe free utun");
    assert_eq!(prepared.config.interface_name.as_deref(), Some("utun200"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn prepare_uses_requested_name_when_free() {
    let dir = temp_dir("prepare-name");
    let host = FakeHost::default();
    let prepared = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()))
        .prepare(&mac_config())
        .expect("prepare");
    assert_eq!(prepared.config.interface_name.as_deref(), Some("utun420"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn prepare_collision_probes_higher_index() {
    let dir = temp_dir("prepare-collision");
    let host = FakeHost::default();
    host.add_utun("utun420", &["1.2.3.4/32".into()]);
    let prepared = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()))
        .prepare(&mac_config())
        .expect("collision fallback");
    assert_eq!(
        prepared.config.interface_name.as_deref(),
        Some("utun421"),
        "occupied name must fall back to a higher free index"
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
        ..mac_config()
    };
    let err = bk.prepare(&ipv4_only).expect_err("ipv4-only leaks IPv6");
    assert_eq!(err.code, TunErrorCode::ApplyFailed);
    assert!(err.message.contains("IPv6"));

    let ipv6_only = TunConfig {
        addresses: vec!["fdfe:dcba:9876::1/126".into()],
        ..mac_config()
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
            ..mac_config()
        };
        let err = bk.prepare(&bad).expect_err("bad cidr");
        assert_eq!(err.code, TunErrorCode::ApplyFailed, "case: {cidr}");
    }
    for cidr in ["fdfe:dcba:9876::1", "fdfe:dcba:9876::1/129", "10.0.0.1/24"] {
        let bad = TunConfig {
            addresses: vec!["10.0.0.1/30".into(), cidr.into()],
            ..mac_config()
        };
        let err = bk.prepare(&bad).expect_err("bad v6 cidr");
        assert_eq!(err.code, TunErrorCode::ApplyFailed, "case: {cidr}");
    }

    let low_mtu = TunConfig {
        mtu: 576,
        ..mac_config()
    };
    assert_eq!(
        bk.prepare(&low_mtu).expect_err("low mtu").code,
        TunErrorCode::ApplyFailed
    );

    for name in ["tun0", "utun", "utunx", "utun-1"] {
        let bad_name = TunConfig {
            interface_name: Some(name.into()),
            ..mac_config()
        };
        let err = bk.prepare(&bad_name).expect_err("bad utun name");
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
    let prepared = bk.prepare(&mac_config()).expect("prepare");
    let applied = bk.apply(&prepared).expect("apply");

    assert_eq!(applied.interface_name.as_deref(), Some("utun420"));
    assert_eq!(applied.interface_id.as_deref(), Some("420"));
    assert_eq!(
        applied.addresses,
        mac_config()
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
        "macOS never mutates DNS"
    );

    let journal = TunJournal::load(&journal_path(&dir))
        .unwrap()
        .expect("journal");
    assert_eq!(journal.last_completed_step, steps::ROUTES_ADDED);
    assert_eq!(journal.interface_name.as_deref(), Some("utun420"));
    assert_eq!(journal.interface_id.as_deref(), Some("420"));
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
    let prepared = bk.prepare(&mac_config()).expect("prepare");
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
    assert!(!host.has_utun("utun420"));
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
    let mut bk = MacosTunBackend::new(
        OWNER,
        Box::new(host.clone()),
        Box::new(coordinator),
        config_path(&dir),
    )
    .with_journal(broken_journal);
    write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let prepared = bk.prepare(&mac_config()).expect("prepare");
    let err = bk.apply(&prepared).expect_err("journal write failure");
    assert_eq!(err.code, TunErrorCode::ApplyFailed);

    assert!(
        !host.has_utun("utun420"),
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
    let mut bk = MacosTunBackend::new(
        OWNER,
        Box::new(host.clone()),
        Box::new(coordinator),
        config_path(&dir),
    )
    .with_journal(broken_journal);
    write_tun_config(&dir, &["10.0.0.1/30", "fdfe:dcba:9876::1/126"]);
    let prepared = bk.prepare(&mac_config()).expect("prepare");
    let err = bk.apply(&prepared).expect_err("uncertain cleanup");
    assert_eq!(err.code, TunErrorCode::RecoveryRequired);
    assert!(
        host.has_utun("utun420"),
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

    host.simulate_kill9();
    let health = bk.verify(&applied).expect("verify after kill-9");
    assert!(!health.interface_up);
    assert!(!health.routes_owned);
    assert!(
        health.nothing_owned,
        "kernel cleaned the fd close; nothing owned"
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
    let prepared = bk.prepare(&mac_config()).expect("prepare");
    let err = bk.apply(&prepared).expect_err("routes never converge");
    assert_eq!(err.code, TunErrorCode::HealthcheckFailed);
    assert!(
        !host.has_utun("utun420"),
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
    // must reject the capture even though the owned record for IPv6 was
    // written (the old self-referential check would pass).
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

    // One required sub-range no longer resolves to the tun: the full-route
    // lock must reject the capture (the old "any route remains" check would
    // pass).
    host.state.lock().unwrap().routes.remove("128.0.0.0/1");
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
    assert!(!host.has_utun("utun420"));

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
        host.has_utun("utun420"),
        "unverified resource is never deleted"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn recover_kill9_residue_is_cleaned() {
    let dir = temp_dir("recover-kill9");
    let host = FakeHost::default();
    let coordinator = FakeCoreCoordinator::new(host.clone());
    let _applied = apply_ok(&dir, &host, coordinator);
    host.simulate_kill9();

    let mut bk = backend(&dir, host.clone(), FakeCoreCoordinator::new(host.clone()));
    let journal = TunJournal::load(&journal_path(&dir))
        .unwrap()
        .expect("journal");
    let outcome = bk.recover(&journal).expect("recover");
    assert_eq!(
        outcome,
        RecoveryOutcome::Cleaned,
        "kernel already removed everything; verification alone proves cleanup"
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
        host.has_utun("utun420"),
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
    assert!(!host.has_utun("utun420"));
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

// --- unsupported / factory ---

#[test]
fn unsupported_backend_refuses_every_operation_with_stable_code() {
    let mut backend = UnsupportedTunBackend::new("gate pending");
    let capability = backend.capability();
    assert!(!capability.supported);
    assert_eq!(capability.reason.as_deref(), Some("gate pending"));
    assert!(!capability.ipv4 && !capability.ipv6 && !capability.dns_hijack);

    let err = backend.prepare(&mac_config()).expect_err("prepare");
    assert_eq!(err.code, TunErrorCode::NotSupported);
    let err = backend
        .apply(&PreparedTun {
            config: mac_config(),
        })
        .expect_err("apply");
    assert_eq!(err.code, TunErrorCode::NotSupported);
    let err = backend
        .verify(&AppliedTun {
            interface_name: None,
            interface_id: None,
            addresses: vec![],
            routes: vec![],
            expected_addresses: vec![],
            expected_routes: vec![],
            dns_before: None,
            dns_after: None,
            core_pid: None,
        })
        .expect_err("verify");
    assert_eq!(err.code, TunErrorCode::NotSupported);
    let err = backend
        .restore(&AppliedTun {
            interface_name: None,
            interface_id: None,
            addresses: vec![],
            routes: vec![],
            expected_addresses: vec![],
            expected_routes: vec![],
            dns_before: None,
            dns_after: None,
            core_pid: None,
        })
        .expect_err("restore");
    assert_eq!(err.code, TunErrorCode::NotSupported);
}

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
        assert!(
            !capability.dns_hijack,
            "macOS native path never mutates OS DNS"
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        assert!(
            !capability.supported,
            "platform gate pending/failed: fail closed"
        );
        assert!(capability.reason.is_some());
    }
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "live host reads; run manually on a real macOS host"]
fn live_process_host_reads_match_a_real_host() {
    let host = ProcessMacOsHost;
    let names = host.list_interface_names().expect("ifconfig -l");
    assert!(names.iter().any(|name| name.starts_with("lo0")));
    let lo0 = host.interface_state("lo0").expect("ifconfig lo0");
    assert!(lo0.is_some(), "lo0 always exists");
    let loopback_route = host.route_interface("127.0.0.1").expect("route get");
    assert_eq!(loopback_route.as_deref(), Some("lo0"));
}
