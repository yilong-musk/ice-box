//! Fault-injection recovery tests (plan T0 exit gate).
//!
//! Every test drives the host-free `FakeTunBackend` against a real journal
//! file and the `RecoveryDriver`, injecting a failure after *each* journaled
//! mutation boundary, then proves that startup recovery converges: no owned
//! resource is left behind, terminal journal state is `clean` or
//! `recovery_required` (fail-closed), and re-running recovery is idempotent.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use ice_tun_sys::backend::RecoveryOutcome;
use ice_tun_sys::error::TunErrorCode;
use ice_tun_sys::fake::{FakeOsState, FakeTunBackend};
use ice_tun_sys::journal::{steps, JournalState, TunJournal};
use ice_tun_sys::recovery::RecoveryDriver;
use ice_tun_sys::{AppliedTun, RouteRecord, TunBackend, TunConfig, TunStack};

const OWNER: &str = "ice-box:test-install-1";
const FOREIGN: &str = "ice-box:someone-elses-install";

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ice-tun-recovery-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn config() -> TunConfig {
    TunConfig {
        interface_name: Some("utun420".into()),
        addresses: vec!["10.0.0.1/30".into()],
        mtu: 9000,
        stack: TunStack::Gvisor,
        auto_route: true,
        strict_route: true,
        dns_hijack: false,
    }
}

fn journal_path(dir: &std::path::Path) -> PathBuf {
    dir.join("tun-state.json")
}

fn backend(dir: &std::path::Path) -> FakeTunBackend {
    FakeTunBackend::new(OWNER).with_journal(journal_path(dir))
}

fn apply_ok(dir: &std::path::Path) -> (FakeTunBackend, AppliedTun) {
    let mut bk = backend(dir);
    let mut journal = TunJournal::new("t-enable".into(), OWNER.into());
    journal
        .record(
            &journal_path(dir),
            JournalState::Preparing,
            steps::JOURNAL_PREPARING,
            |_| {},
        )
        .unwrap();
    let prepared = bk.prepare(&config()).unwrap();
    let applied = bk.apply(&prepared).unwrap();
    let health = bk.verify(&applied).unwrap();
    assert!(health.all_ok(), "enable preconditions healthy");
    journal
        .record(
            &journal_path(dir),
            JournalState::Applied,
            steps::VERIFY_APPLIED,
            |j| {
                j.interface_name = applied.interface_name.clone();
                j.interface_id = applied.interface_id.clone();
                j.addresses = applied.addresses.clone();
                j.routes = applied.routes.clone();
            },
        )
        .unwrap();
    (bk, applied)
}

/// Simulate an enable that crashed after the k-th journaled mutation
/// (k = 0..=3: interface, addresses, routes, dns). Returns the journal
/// step the crash left behind.
fn crash_during_enable(dir: &std::path::Path, after_mutations: usize) -> String {
    let mut bk = backend(dir);
    let mut journal = TunJournal::new("t-crash".into(), OWNER.into());
    journal
        .record(
            &journal_path(dir),
            JournalState::Preparing,
            steps::JOURNAL_PREPARING,
            |_| {},
        )
        .unwrap();
    bk.faults.fail_apply_after_mutations = Some(after_mutations);
    let prepared = bk.prepare(&config()).unwrap();
    let err = bk.apply(&prepared).expect_err("injected apply failure");
    assert_eq!(err.code, TunErrorCode::ApplyFailed);
    let journal = TunJournal::load(&journal_path(dir))
        .unwrap()
        .expect("journal exists after crash");
    journal.last_completed_step.clone()
}

fn assert_no_owned_resources(state: &FakeOsState) {
    assert!(state.interface.is_none(), "interface must be removed");
    assert!(state.addresses.is_empty(), "addresses must be removed");
    assert!(state.routes.is_empty(), "routes must be removed");
}

#[test]
fn no_journal_is_nothing_to_do() {
    let dir = temp_dir("no-journal");
    let mut bk = backend(&dir);
    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(outcome, RecoveryOutcome::NothingToDo);
    assert!(bk.trace().is_empty(), "no backend ops when no journal");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn clean_journal_is_nothing_to_do_even_with_stale_resources() {
    let dir = temp_dir("clean");
    let mut bk = backend(&dir);
    // Resources linger but the journal says clean: recovery must not touch them.
    bk.state.interface = Some(ice_tun_sys::fake::FakeInterface {
        name: "utun420".into(),
        id: "id-utun420".into(),
    });
    let mut journal = TunJournal::new("t-clean".into(), OWNER.into());
    journal
        .record(
            &journal_path(&dir),
            JournalState::Clean,
            steps::VERIFY_CLEAN,
            |_| {},
        )
        .unwrap();

    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(outcome, RecoveryOutcome::NothingToDo);
    assert!(
        bk.state.interface.is_some(),
        "a clean journal must not drive resource removal"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn foreign_journal_is_never_touched() {
    let dir = temp_dir("foreign");
    let mut bk = backend(&dir);
    // Simulate an applied capture owned by another installation.
    let mut journal = TunJournal::new("t-foreign".into(), FOREIGN.into());
    journal
        .record(
            &journal_path(&dir),
            JournalState::Applied,
            steps::VERIFY_APPLIED,
            |j| {
                j.interface_name = Some("utun420".into());
                j.interface_id = Some("id-utun420".into());
                j.routes = vec![RouteRecord {
                    destination: "128.0.0.0/1".into(),
                    gateway: Some("10.0.0.2".into()),
                    metric: 0,
                    owned: true,
                }];
            },
        )
        .unwrap();
    bk.state.interface = Some(ice_tun_sys::fake::FakeInterface {
        name: "utun420".into(),
        id: "id-utun420".into(),
    });
    bk.state.routes = journal.routes.clone();

    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(outcome, RecoveryOutcome::ForeignJournal);
    assert!(
        bk.state.interface.is_some() && !bk.state.routes.is_empty(),
        "foreign resources must remain untouched"
    );
    let persisted = TunJournal::load(&journal_path(&dir)).unwrap().unwrap();
    assert_eq!(persisted.state, JournalState::Applied);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn crash_after_every_enable_mutation_converges_to_clean() {
    // after_mutations = 0..=3 covers the interface/addresses/routes/dns
    // boundaries (dns_hijack off here, so max mutations is 3).
    for after in 0..=3usize {
        let dir = temp_dir(&format!("crash-{after}"));
        let _step = crash_during_enable(&dir, after);

        let mut bk = backend(&dir);
        let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
            .recover()
            .unwrap();
        assert_eq!(outcome, RecoveryOutcome::Cleaned, "after {after} mutations");
        assert_no_owned_resources(&bk.state);

        let persisted = TunJournal::load(&journal_path(&dir)).unwrap().unwrap();
        assert_eq!(persisted.state, JournalState::Clean);
        assert_eq!(persisted.last_completed_step, steps::VERIFY_CLEAN);

        // Idempotence: a second recovery run sees nothing to do.
        let mut bk2 = backend(&dir);
        let again = RecoveryDriver::new(&journal_path(&dir), &mut bk2, OWNER)
            .recover()
            .unwrap();
        assert_eq!(
            again,
            RecoveryOutcome::NothingToDo,
            "after {after} mutations"
        );
        assert!(bk2.trace().is_empty(), "no backend ops on repeat recovery");
        let _ = fs::remove_dir_all(&dir);
    }
}

#[test]
fn crash_left_journal_step_matches_injected_boundary() {
    let dir = temp_dir("step-match");
    let step = crash_during_enable(&dir, 1);
    assert_eq!(step, steps::INTERFACE_CREATED);
    let _ = fs::remove_dir_all(&dir);

    let dir = temp_dir("step-match-2");
    let step = crash_during_enable(&dir, 2);
    assert_eq!(step, steps::ADDRESSES_ASSIGNED);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn prepare_failure_leaves_no_mutation_and_recovers_clean() {
    let dir = temp_dir("prepare-fail");
    let mut bk = backend(&dir);
    bk.faults.fail_prepare = true;
    let mut journal = TunJournal::new("t-prep".into(), OWNER.into());
    journal
        .record(
            &journal_path(&dir),
            JournalState::Preparing,
            steps::JOURNAL_PREPARING,
            |_| {},
        )
        .unwrap();
    let err = bk.prepare(&config()).expect_err("injected prepare failure");
    assert_eq!(err.code, TunErrorCode::ApplyFailed);
    assert_no_owned_resources(&bk.state);

    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(outcome, RecoveryOutcome::Cleaned);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn verify_failure_during_recovery_fails_closed_then_retry_converges() {
    let dir = temp_dir("verify-fail");
    let (mut bk, _applied) = apply_ok(&dir);
    bk.faults.fail_verify_applied = true;
    let applied =
        AppliedTun::from_journal(&TunJournal::load(&journal_path(&dir)).unwrap().unwrap());
    let err = bk.verify(&applied).expect_err("injected verify failure");
    assert_eq!(err.code, TunErrorCode::HealthcheckFailed);

    // Cleanup could not be *verified*: recovery fails closed even though the
    // resources were removed, and the journal persists recovery_required.
    let err = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .expect_err("verification failure must fail closed");
    assert_eq!(err.code, TunErrorCode::HealthcheckFailed);
    assert_no_owned_resources(&bk.state);
    let persisted = TunJournal::load(&journal_path(&dir)).unwrap().unwrap();
    assert_eq!(persisted.state, JournalState::RecoveryRequired);

    // Explicit retry with verification working converges to clean.
    bk.faults.fail_verify_applied = false;
    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(outcome, RecoveryOutcome::Cleaned);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn crash_while_applied_cleans_up_on_startup() {
    let dir = temp_dir("applied-crash");
    let (mut bk, _applied) = apply_ok(&dir);
    assert!(bk.state.interface.is_some());

    // kill -9 style crash: process gone, resources (routes) remain.
    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(outcome, RecoveryOutcome::Cleaned);
    assert_no_owned_resources(&bk.state);
    let persisted = TunJournal::load(&journal_path(&dir)).unwrap().unwrap();
    assert_eq!(persisted.state, JournalState::Clean);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn restore_resumes_from_every_restoring_step() {
    for step in [
        steps::RESTORE_STARTED,
        steps::ROUTES_REMOVED,
        steps::INTERFACE_REMOVED,
    ] {
        let dir = temp_dir(&format!("restoring-{step}"));
        let (mut bk, _applied) = apply_ok(&dir);
        let mut journal = TunJournal::load(&journal_path(&dir)).unwrap().unwrap();
        journal
            .record(&journal_path(&dir), JournalState::Restoring, step, |_| {})
            .unwrap();

        let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
            .recover()
            .unwrap();
        assert_eq!(outcome, RecoveryOutcome::Cleaned, "resume from {step}");
        assert_no_owned_resources(&bk.state);
        let _ = fs::remove_dir_all(&dir);
    }
}

#[test]
fn recovery_failure_partial_cleanup_then_retry_converges() {
    let dir = temp_dir("recovery-retry");
    let (mut bk, _applied) = apply_ok(&dir);
    bk.faults.fail_restore_after_mutations = Some(1);

    let err = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .expect_err("injected restore failure");
    assert_eq!(err.code, TunErrorCode::RestoreFailed);
    // Partial cleanup: routes removed, interface still present.
    assert!(
        bk.state.interface.is_some(),
        "interface survives the partial failure"
    );
    assert!(bk.state.routes.is_empty(), "routes were already removed");
    let persisted = TunJournal::load(&journal_path(&dir)).unwrap().unwrap();
    assert_eq!(
        persisted.state,
        JournalState::RecoveryRequired,
        "uncertain cleanup must persist recovery_required (fail closed)"
    );
    assert_eq!(persisted.last_completed_step, steps::ROUTES_REMOVED);

    // Explicit retry (no faults): converges.
    bk.faults.fail_restore_after_mutations = None;
    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(outcome, RecoveryOutcome::Cleaned);
    assert_no_owned_resources(&bk.state);

    // Idempotence: a third run has nothing to do.
    let mut bk3 = backend(&dir);
    let again = RecoveryDriver::new(&journal_path(&dir), &mut bk3, OWNER)
        .recover()
        .unwrap();
    assert_eq!(again, RecoveryOutcome::NothingToDo);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn stuck_resource_fails_closed_until_explicit_retry() {
    let dir = temp_dir("stuck");
    let (mut bk, _applied) = apply_ok(&dir);
    bk.faults.stuck_route = Some("128.0.0.0/1".into());

    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(
        outcome,
        RecoveryOutcome::RecoveryRequired,
        "unverifiable cleanup must fail closed, not claim success"
    );
    assert!(
        bk.state
            .routes
            .iter()
            .any(|r| r.destination == "128.0.0.0/1"),
        "unverified route must never be deleted"
    );
    let persisted = TunJournal::load(&journal_path(&dir)).unwrap().unwrap();
    assert_eq!(persisted.state, JournalState::RecoveryRequired);

    // Unstick and retry: converges.
    bk.faults.stuck_route = None;
    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(outcome, RecoveryOutcome::Cleaned);
    assert_no_owned_resources(&bk.state);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn dns_compare_before_restore_preserves_external_change() {
    let dir = temp_dir("dns-external");
    let mut bk = FakeTunBackend::new(OWNER).with_journal(journal_path(&dir));
    let mut journal = TunJournal::new("t-dns".into(), OWNER.into());
    journal
        .record(
            &journal_path(&dir),
            JournalState::Preparing,
            steps::JOURNAL_PREPARING,
            |_| {},
        )
        .unwrap();
    let mut cfg = config();
    cfg.dns_hijack = true;
    let prepared = bk.prepare(&cfg).unwrap();
    let applied = bk.apply(&prepared).unwrap();
    assert!(applied.dns_after.is_some());
    journal
        .record(
            &journal_path(&dir),
            JournalState::Applied,
            steps::VERIFY_APPLIED,
            |j| {
                j.interface_name = applied.interface_name.clone();
                j.interface_id = applied.interface_id.clone();
                j.addresses = applied.addresses.clone();
                j.routes = applied.routes.clone();
                j.dns_before = applied.dns_before.clone();
                j.dns_after = applied.dns_after.clone();
            },
        )
        .unwrap();
    assert_eq!(bk.state.dns_current.as_deref(), Some("fake-tun-resolver"));

    // Another VPN / user changed DNS while capture was active.
    bk.state.dns_current = Some("external-vpn-resolver".into());

    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(
        outcome,
        RecoveryOutcome::RecoveryRequired,
        "external DNS change must not be overwritten with stale data"
    );
    assert_eq!(
        bk.state.dns_current.as_deref(),
        Some("external-vpn-resolver"),
        "external DNS value preserved"
    );
    assert!(
        bk.state.interface.is_none(),
        "non-DNS resources still cleaned up"
    );
    let persisted = TunJournal::load(&journal_path(&dir)).unwrap().unwrap();
    assert_eq!(persisted.state, JournalState::RecoveryRequired);

    // Retry while the external change persists stays fail-closed.
    let again = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(again, RecoveryOutcome::RecoveryRequired);
    assert_eq!(
        bk.state.dns_current.as_deref(),
        Some("external-vpn-resolver"),
        "retry must keep preserving the external change"
    );

    // External change reverts to the applied snapshot → restore completes.
    bk.state.dns_current = Some("fake-tun-resolver".into());
    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(outcome, RecoveryOutcome::Cleaned);
    assert_eq!(
        bk.state.dns_current.as_deref(),
        Some("platform-default"),
        "stale before-snapshot restored only after the change reverted"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn recovery_required_journal_is_retried_on_next_startup() {
    let dir = temp_dir("rr-retry");
    let (mut bk, _applied) = apply_ok(&dir);
    let mut journal = TunJournal::load(&journal_path(&dir)).unwrap().unwrap();
    journal
        .record(
            &journal_path(&dir),
            JournalState::RecoveryRequired,
            steps::RESTORE_STARTED,
            |_| {},
        )
        .unwrap();

    // Next startup retries the idempotent recovery and converges.
    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(outcome, RecoveryOutcome::Cleaned);
    assert_no_owned_resources(&bk.state);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn control_path_broken_rejects_enable_health() {
    let dir = temp_dir("control-path");
    let mut bk = backend(&dir);
    bk.faults.control_path_broken = true;
    let mut journal = TunJournal::new("t-ctl".into(), OWNER.into());
    journal
        .record(
            &journal_path(&dir),
            JournalState::Preparing,
            steps::JOURNAL_PREPARING,
            |_| {},
        )
        .unwrap();
    let prepared = bk.prepare(&config()).unwrap();
    let applied = bk.apply(&prepared).unwrap();
    let health = bk.verify(&applied).unwrap();
    assert!(
        !health.all_ok(),
        "a broken control path must fail readiness"
    );
    assert!(health.interface_up);

    // Fail-closed on the enabled claim: recovery must clean up.
    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(outcome, RecoveryOutcome::Cleaned);
    assert_no_owned_resources(&bk.state);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn verify_reports_state_not_errors_when_resources_gone() {
    let dir = temp_dir("verify-gone");
    let (mut bk, applied) = apply_ok(&dir);
    // Simulate an external removal of the adapter while the app was alive.
    bk.state.interface = None;
    bk.state.addresses.clear();
    let health = bk.verify(&applied).unwrap();
    assert!(!health.interface_up);
    assert!(!health.addresses_present);
    assert!(!health.all_ok());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn apply_without_auto_route_records_no_routes() {
    let dir = temp_dir("no-route");
    let mut bk = backend(&dir);
    let mut cfg = config();
    cfg.auto_route = false;
    let mut journal = TunJournal::new("t-noroute".into(), OWNER.into());
    journal
        .record(
            &journal_path(&dir),
            JournalState::Preparing,
            steps::JOURNAL_PREPARING,
            |_| {},
        )
        .unwrap();
    let prepared = bk.prepare(&cfg).unwrap();
    let applied = bk.apply(&prepared).unwrap();
    assert!(applied.routes.is_empty());
    let health = bk.verify(&applied).unwrap();
    assert!(health.routes_owned);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unsupported_capability_reports_stable_reason() {
    let mut bk = backend(&std::path::PathBuf::new());
    bk.faults.capability_reason = Some("missing driver".into());
    let cap = bk.capability();
    assert!(!cap.supported);
    assert_eq!(cap.reason.as_deref(), Some("missing driver"));
    assert!(!cap.ipv4);
    let _ = fs::remove_dir_all(std::path::PathBuf::new());
}

/// Guard: the fake's `apply` is idempotent (replaying after a partial
/// apply converges without duplicates).
#[test]
fn fake_apply_is_idempotent() {
    let dir = temp_dir("idempotent-apply");
    let mut bk = backend(&dir);
    let mut journal = TunJournal::new("t-ida".into(), OWNER.into());
    journal
        .record(
            &journal_path(&dir),
            JournalState::Preparing,
            steps::JOURNAL_PREPARING,
            |_| {},
        )
        .unwrap();
    let prepared = bk.prepare(&config()).unwrap();
    let first = bk.apply(&prepared).unwrap();
    let second = bk.apply(&prepared).unwrap();
    assert_eq!(first, second, "replayed apply must be a no-op");
    assert_eq!(
        bk.state.routes.len(),
        ice_tun_sys::fake::AUTO_ROUTE_RANGES.len() + 1
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn health_nothing_owned_is_false_while_capture_active() {
    let dir = temp_dir("nothing-owned");
    let (bk, applied) = apply_ok(&dir);
    let health = bk.verify(&applied).unwrap();
    assert!(!health.nothing_owned);
    assert!(health.all_ok());
    let _ = fs::remove_dir_all(&dir);
}

/// The restore path must never delete an adapter it cannot verify: a live
/// interface whose identity does not match the journal (same name, foreign
/// id) is an external resource and survives recovery, which then fails
/// closed instead of claiming `Cleaned`.
#[test]
fn restore_keeps_external_interface_with_different_identity() {
    let dir = temp_dir("external-iface");
    let (mut bk, _applied) = apply_ok(&dir);
    // Another process replaced the adapter: same name, different id, with
    // its own addresses.
    bk.state.interface = Some(ice_tun_sys::fake::FakeInterface {
        name: "utun420".into(),
        id: "id-someone-elses".into(),
    });
    bk.state.addresses = vec!["192.168.50.1/24".into()];

    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(
        outcome,
        RecoveryOutcome::RecoveryRequired,
        "an unverified adapter must fail closed, not be deleted"
    );
    assert!(
        bk.state.interface.is_some(),
        "the external adapter must survive recovery"
    );
    assert_eq!(
        bk.state.addresses,
        vec!["192.168.50.1/24".to_string()],
        "the external adapter's addresses must survive recovery"
    );
    let persisted = TunJournal::load(&journal_path(&dir)).unwrap().unwrap();
    assert_eq!(persisted.state, JournalState::RecoveryRequired);
    let _ = fs::remove_dir_all(&dir);
}

/// During `preparing` the journal has not recorded the interface yet; an
/// adapter present in that window (e.g. pre-existing, or created by a
/// crashed process before ownership was journaled) must never be removed.
#[test]
fn unjournaled_interface_is_never_removed() {
    let dir = temp_dir("unjournaled-iface");
    let mut bk = backend(&dir);
    bk.state.interface = Some(ice_tun_sys::fake::FakeInterface {
        name: "utun420".into(),
        id: "id-utun420".into(),
    });
    let mut journal = TunJournal::new("t-unjournaled".into(), OWNER.into());
    journal
        .record(
            &journal_path(&dir),
            JournalState::Preparing,
            steps::JOURNAL_PREPARING,
            |_| {},
        )
        .unwrap();

    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(outcome, RecoveryOutcome::RecoveryRequired);
    assert!(
        bk.state.interface.is_some(),
        "an unjournaled interface must never be deleted"
    );
    let persisted = TunJournal::load(&journal_path(&dir)).unwrap().unwrap();
    assert_eq!(persisted.state, JournalState::RecoveryRequired);
    let _ = fs::remove_dir_all(&dir);
}

/// Replaying `apply` with `dns_hijack` must reuse the journal's first DNS
/// snapshot: the second apply must not re-record the already-hijacked
/// resolver as the restore target, and restore must converge back to the
/// original platform resolver.
#[test]
fn dns_hijack_apply_replay_is_idempotent() {
    let dir = temp_dir("dns-replay");
    let mut bk = FakeTunBackend::new(OWNER).with_journal(journal_path(&dir));
    let mut journal = TunJournal::new("t-dnsr".into(), OWNER.into());
    journal
        .record(
            &journal_path(&dir),
            JournalState::Preparing,
            steps::JOURNAL_PREPARING,
            |_| {},
        )
        .unwrap();
    let mut cfg = config();
    cfg.dns_hijack = true;
    let prepared = bk.prepare(&cfg).unwrap();
    let first = bk.apply(&prepared).unwrap();
    assert_eq!(
        first.dns_before.as_ref().unwrap().platform_snapshot,
        "platform-default"
    );
    let second = bk.apply(&prepared).unwrap();
    assert_eq!(first, second, "replayed apply must be a no-op");
    assert_eq!(
        second.dns_before.as_ref().unwrap().platform_snapshot,
        "platform-default",
        "replay must keep the first snapshot, not re-record the hijacked resolver"
    );
    assert_eq!(bk.state.dns_current.as_deref(), Some("fake-tun-resolver"));

    journal
        .record(
            &journal_path(&dir),
            JournalState::Applied,
            steps::VERIFY_APPLIED,
            |j| {
                j.interface_name = second.interface_name.clone();
                j.interface_id = second.interface_id.clone();
                j.addresses = second.addresses.clone();
                j.routes = second.routes.clone();
                j.dns_before = second.dns_before.clone();
                j.dns_after = second.dns_after.clone();
            },
        )
        .unwrap();

    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(outcome, RecoveryOutcome::Cleaned);
    assert_eq!(
        bk.state.dns_current.as_deref(),
        Some("platform-default"),
        "restore must return to the original resolver, not the hijacked one"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// When the platform DNS was reverted externally while the journal still
/// owns the hijack, a re-apply must reuse the journal's original snapshot
/// instead of overwriting it with the reverted value.
#[test]
fn dns_apply_reuses_journal_snapshot_when_state_reverted() {
    let dir = temp_dir("dns-reuse");
    let mut bk = FakeTunBackend::new(OWNER).with_journal(journal_path(&dir));
    let mut journal = TunJournal::new("t-dnsu".into(), OWNER.into());
    journal
        .record(
            &journal_path(&dir),
            JournalState::Applied,
            steps::DNS_APPLIED,
            |j| {
                j.dns_before = Some(ice_tun_sys::journal::DnsSnapshot {
                    platform_snapshot: "original-resolver".into(),
                });
                j.dns_after = Some(ice_tun_sys::journal::DnsSnapshot {
                    platform_snapshot: "fake-tun-resolver".into(),
                });
            },
        )
        .unwrap();
    // Platform DNS reverted externally while capture was active.
    let mut cfg = config();
    cfg.dns_hijack = true;
    let prepared = bk.prepare(&cfg).unwrap();
    let applied = bk.apply(&prepared).unwrap();
    assert_eq!(
        applied.dns_before.as_ref().unwrap().platform_snapshot,
        "original-resolver",
        "the journal's first snapshot must be reused, not recomputed"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Crash in the window between an OS mutation and its journal record: the
/// fake must roll the unjournaled mutation back (recovery is only
/// authorized to delete journaled resources), and previously journaled
/// mutations stay owned until recovery removes them.
#[test]
fn crash_between_mutation_and_journal_write_never_leaks_resources() {
    for after in 1..=3usize {
        let dir = temp_dir(&format!("journal-window-{after}"));
        let mut bk = backend(&dir);
        let mut journal = TunJournal::new("t-window".into(), OWNER.into());
        journal
            .record(
                &journal_path(&dir),
                JournalState::Preparing,
                steps::JOURNAL_PREPARING,
                |_| {},
            )
            .unwrap();
        bk.faults.fail_journal_write_after_mutations = Some(after);
        let prepared = bk.prepare(&config()).unwrap();
        let err = bk.apply(&prepared).expect_err("injected journal failure");
        assert_eq!(err.code, TunErrorCode::ApplyFailed);

        // The unjournaled mutation was rolled back; the journal ends at the
        // previous completed step and holds no ownership for it.
        let persisted = TunJournal::load(&journal_path(&dir)).unwrap().unwrap();
        let expected_step = match after {
            1 => steps::JOURNAL_PREPARING,
            2 => steps::INTERFACE_CREATED,
            _ => steps::ADDRESSES_ASSIGNED,
        };
        assert_eq!(
            persisted.last_completed_step, expected_step,
            "after {after}"
        );
        match after {
            1 => assert!(persisted.interface_name.is_none(), "after {after}"),
            2 => assert!(persisted.addresses.is_empty(), "after {after}"),
            _ => assert!(persisted.routes.is_empty(), "after {after}"),
        }

        // Recovery removes the journaled resources and converges clean.
        let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
            .recover()
            .unwrap();
        assert_eq!(outcome, RecoveryOutcome::Cleaned, "after {after}");
        assert_no_owned_resources(&bk.state);
        let _ = fs::remove_dir_all(&dir);
    }
}

/// The same crash window for the DNS mutation: the hijack must be rolled
/// back and no DNS ownership may be recorded, so recovery converges clean
/// instead of restoring to a stale snapshot.
#[test]
fn crash_between_dns_mutation_and_journal_write_rolls_back() {
    let dir = temp_dir("dns-window");
    let mut bk = backend(&dir);
    let mut journal = TunJournal::new("t-dnsw".into(), OWNER.into());
    journal
        .record(
            &journal_path(&dir),
            JournalState::Preparing,
            steps::JOURNAL_PREPARING,
            |_| {},
        )
        .unwrap();
    bk.faults.fail_journal_write_after_mutations = Some(4);
    let mut cfg = config();
    cfg.dns_hijack = true;
    let prepared = bk.prepare(&cfg).unwrap();
    let err = bk.apply(&prepared).expect_err("injected journal failure");
    assert_eq!(err.code, TunErrorCode::ApplyFailed);
    assert_eq!(
        bk.state.dns_current, None,
        "the unjournaled DNS mutation must be rolled back"
    );
    let persisted = TunJournal::load(&journal_path(&dir)).unwrap().unwrap();
    assert_eq!(persisted.last_completed_step, steps::ROUTES_ADDED);
    assert!(
        persisted.dns_before.is_none() && persisted.dns_after.is_none(),
        "no DNS ownership may be recorded"
    );

    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(outcome, RecoveryOutcome::Cleaned);
    assert_no_owned_resources(&bk.state);
    let _ = fs::remove_dir_all(&dir);
}

/// The fake models IPv4 *and* IPv6 routes (dual-stack lock); capability
/// must match the modeled route families.
#[test]
fn capability_matches_the_route_model() {
    let bk = backend(&std::path::PathBuf::new());
    let cap = bk.capability();
    assert!(cap.supported);
    assert!(cap.ipv4, "fake must model IPv4 routes");
    assert!(cap.ipv6, "fake must model IPv6 routes (dual-stack lock)");
    assert!(cap.dns_hijack);
    let _ = fs::remove_dir_all(std::path::PathBuf::new());
}

fn dual_stack_config() -> TunConfig {
    let mut cfg = config();
    cfg.addresses = vec!["10.0.0.1/30".into(), "fdfe:dcba:9876::1/126".into()];
    cfg
}

/// A dual-stack config must install and verify the IPv6 default-route split
/// and the IPv6 connected route, and recovery must remove them all.
#[test]
fn dual_stack_config_installs_and_verifies_ipv6_routes() {
    let dir = temp_dir("dual-stack");
    let mut bk = backend(&dir);
    let mut journal = TunJournal::new("t-ds".into(), OWNER.into());
    journal
        .record(
            &journal_path(&dir),
            JournalState::Preparing,
            steps::JOURNAL_PREPARING,
            |_| {},
        )
        .unwrap();
    let prepared = bk.prepare(&dual_stack_config()).unwrap();
    let applied = bk.apply(&prepared).unwrap();
    assert!(
        applied.routes.iter().any(|r| r.destination == "::/1"),
        "IPv6 default-route split must be installed"
    );
    assert!(applied.routes.iter().any(|r| r.destination == "8000::/1"));
    assert!(
        applied
            .routes
            .iter()
            .any(|r| r.destination == "fdfe:dcba:9876:0:0:0:0:2/126"),
        "IPv6 connected route must be installed"
    );
    assert_eq!(
        applied
            .routes
            .iter()
            .filter(|r| r.destination.contains(':'))
            .count(),
        ice_tun_sys::fake::AUTO_ROUTE_RANGES_V6.len() + 1,
        "v6 auto-ranges + v6 connected route"
    );
    let health = bk.verify(&applied).unwrap();
    assert!(health.all_ok(), "dual-stack health must be all-ok");

    journal
        .record(
            &journal_path(&dir),
            JournalState::Applied,
            steps::VERIFY_APPLIED,
            |j| {
                j.interface_name = applied.interface_name.clone();
                j.interface_id = applied.interface_id.clone();
                j.addresses = applied.addresses.clone();
                j.routes = applied.routes.clone();
            },
        )
        .unwrap();
    let outcome = RecoveryDriver::new(&journal_path(&dir), &mut bk, OWNER)
        .recover()
        .unwrap();
    assert_eq!(outcome, RecoveryOutcome::Cleaned);
    assert_no_owned_resources(&bk.state);
    let _ = fs::remove_dir_all(&dir);
}

/// An IPv6-only config is fully captured: no IPv4 routes are fabricated
/// and all health checks pass.
#[test]
fn ipv6_only_config_is_captured_and_verified() {
    let dir = temp_dir("v6-only");
    let mut bk = backend(&dir);
    let mut cfg = config();
    cfg.addresses = vec!["fdfe:dcba:9876::1/126".into()];
    let prepared = bk.prepare(&cfg).unwrap();
    let applied = bk.apply(&prepared).unwrap();
    assert!(
        applied.routes.iter().all(|r| r.destination.contains(':')),
        "an IPv6-only tun installs no IPv4 routes"
    );
    let health = bk.verify(&applied).unwrap();
    assert!(health.all_ok());
    let _ = fs::remove_dir_all(&dir);
}

/// A missing IPv6 route must fail health: an IPv6 leak can never pass
/// `all_ok()` on a dual-stack config.
#[test]
fn missing_ipv6_route_fails_dual_stack_health() {
    let dir = temp_dir("v6-leak");
    let mut bk = backend(&dir);
    let mut journal = TunJournal::new("t-v6l".into(), OWNER.into());
    journal
        .record(
            &journal_path(&dir),
            JournalState::Preparing,
            steps::JOURNAL_PREPARING,
            |_| {},
        )
        .unwrap();
    let prepared = bk.prepare(&dual_stack_config()).unwrap();
    let applied = bk.apply(&prepared).unwrap();
    // Simulate an IPv6 leak: the IPv6 default route never got installed.
    bk.state.routes.retain(|r| r.destination != "::/1");
    let health = bk.verify(&applied).unwrap();
    assert!(!health.routes_owned, "missing IPv6 route must fail health");
    assert!(!health.all_ok(), "an IPv6 leak must never pass all_ok");
    let _ = fs::remove_dir_all(&dir);
}

/// `prepare` must reject a config that claims an IPv6 address it cannot
/// turn into routes — otherwise the fake would silently skip IPv6 and leak.
#[test]
fn invalid_ipv6_address_is_rejected_by_prepare() {
    let dir = temp_dir("v6-invalid");
    let bk = backend(&dir);
    let mut cfg = config();
    cfg.addresses = vec!["zzzz::1/126".into()];
    let err = bk
        .prepare(&cfg)
        .expect_err("invalid IPv6 address must be rejected");
    assert_eq!(err.code, TunErrorCode::ApplyFailed);
    assert_no_owned_resources(&bk.state);
    let _ = fs::remove_dir_all(&dir);
}
