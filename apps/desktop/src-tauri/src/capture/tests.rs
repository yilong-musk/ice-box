// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use ice_core::{CoreError, CorePaths, CoreState, ReloadOutcome};
use ice_proxy_sys::{ProxyBackup, ProxyEndpoints, ProxySysError};
use ice_tun_sys::fake::{FakeOsState, FakeTunBackend};
use ice_tun_sys::{
    AppliedTun, PreparedTun, RecoveryOutcome, TunCapability, TunConfig, TunError, TunErrorCode,
    TunHealth, TunJournal,
};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const OWNER: &str = "ice-box:test";

fn temp_paths(label: &str) -> AppPaths {
    let dir = std::env::temp_dir().join(format!(
        "ice-box-capture-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let paths = AppPaths::new(&dir);
    paths.ensure_dirs().unwrap();
    paths
}

fn tun_settings(enabled: bool) -> AppSettings {
    AppSettings {
        tun: TunSettings {
            enabled,
            interface_name: Some("utun420".into()),
            // Crash-recovery tests exercise interface/address/route
            // topology; keep DNS mutations out so the simulated OS reset
            // converges to Clean instead of RecoveryRequired.
            dns_hijack: false,
            ..TunSettings::default()
        },
        ..AppSettings::default()
    }
}

/// Core mock recording lifecycle calls for transition assertions.
struct TrackCore {
    status: Cell<CoreStatus>,
    start_calls: Cell<usize>,
    stop_calls: Cell<usize>,
    adopt_pids: std::cell::RefCell<Vec<u32>>,
    last_start_config: std::cell::RefCell<Option<String>>,
    fail_adopt: Cell<bool>,
}

impl Default for TrackCore {
    fn default() -> Self {
        Self {
            status: Cell::new(CoreStatus::Stopped),
            start_calls: Cell::new(0),
            stop_calls: Cell::new(0),
            adopt_pids: std::cell::RefCell::new(Vec::new()),
            last_start_config: std::cell::RefCell::new(None),
            fail_adopt: Cell::new(false),
        }
    }
}

impl TrackCore {
    fn running() -> Self {
        Self {
            status: Cell::new(CoreStatus::Running),
            ..Self::default()
        }
    }
}

impl CoreHandle for TrackCore {
    fn state(&self) -> CoreState {
        CoreState {
            status: self.status.get(),
            message: None,
            inbound_host: Some("127.0.0.1".into()),
            inbound_port: Some(17890),
        }
    }

    fn start(&mut self, paths: &CorePaths) -> Result<(), CoreError> {
        self.start_calls.set(self.start_calls.get() + 1);
        self.last_start_config
            .replace(Some(paths.config.to_string_lossy().into_owned()));
        self.status.set(CoreStatus::Running);
        Ok(())
    }

    fn stop(&mut self, _pid_file: &Path) -> Result<(), CoreError> {
        self.stop_calls.set(self.stop_calls.get() + 1);
        self.status.set(CoreStatus::Stopped);
        Ok(())
    }

    fn reload(&mut self, _paths: &CorePaths) -> Result<ReloadOutcome, CoreError> {
        Err(CoreError::invalid_state("mock"))
    }

    fn needs_proxy_restore(&self) -> bool {
        false
    }

    fn clear_needs_proxy_restore(&mut self) {}

    fn reap_exited_child(&mut self, _pid_file: &Path) -> bool {
        false
    }

    fn adopt_external(&mut self, pid: u32, _paths: &CorePaths) -> Result<(), CoreError> {
        if self.fail_adopt.get() {
            self.status.set(CoreStatus::Error);
            return Err(CoreError::SpawnFailed("mock adopt failed".into()));
        }
        self.adopt_pids.borrow_mut().push(pid);
        self.status.set(CoreStatus::Running);
        Ok(())
    }

    fn reclaim_orphan_pid(&mut self, _: &Path) -> Result<(), CoreError> {
        self.status.set(CoreStatus::Stopped);
        Ok(())
    }
}

/// Fake backend wrapper for native-path simulations: can report an
/// elevated-core pid (so the controller adopts it) and inject one-shot
/// apply failures.
struct ScriptedBackend {
    inner: FakeTunBackend,
    fail_next_apply: Cell<bool>,
    core_pid: Option<u32>,
}

impl ScriptedBackend {
    fn new() -> Self {
        Self {
            inner: FakeTunBackend::new(OWNER),
            fail_next_apply: Cell::new(false),
            core_pid: None,
        }
    }
}

impl TunBackend for ScriptedBackend {
    fn capability(&self) -> TunCapability {
        self.inner.capability()
    }

    fn prepare(&self, config: &TunConfig) -> Result<PreparedTun, TunError> {
        self.inner.prepare(config)
    }

    fn apply(&mut self, prepared: &PreparedTun) -> Result<AppliedTun, TunError> {
        if self.fail_next_apply.replace(false) {
            return Err(TunError::new(
                TunErrorCode::ApplyFailed,
                "injected one-shot apply failure",
            ));
        }
        let mut applied = self.inner.apply(prepared)?;
        applied.core_pid = self.core_pid;
        Ok(applied)
    }

    fn verify(&self, applied: &AppliedTun) -> Result<TunHealth, TunError> {
        self.inner.verify(applied)
    }

    fn restore(&mut self, applied: &AppliedTun) -> Result<(), TunError> {
        self.inner.restore(applied)
    }

    fn recover(&mut self, journal: &TunJournal) -> Result<RecoveryOutcome, TunError> {
        self.inner.recover(journal)
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn attach_journal(&mut self, path: PathBuf) {
        self.inner.attach_journal(path);
    }
}

#[derive(Default)]
struct TrackProxy {
    apply_calls: Cell<usize>,
    restore_calls: Cell<usize>,
    fail_apply: Cell<bool>,
    fail_restore: Cell<bool>,
}

impl SystemProxy for TrackProxy {
    fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
        Ok(ProxyBackup::default())
    }

    fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
        self.apply_calls.set(self.apply_calls.get() + 1);
        if self.fail_apply.get() {
            return Err(ProxySysError::ApplyFailed("mock".into()));
        }
        Ok(())
    }

    fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
        self.restore_calls.set(self.restore_calls.get() + 1);
        if self.fail_restore.get() {
            return Err(ProxySysError::RestoreFailed("mock".into()));
        }
        Ok(())
    }
}

fn fake_backend() -> Box<dyn TunBackend + Send> {
    Box::new(FakeTunBackend::new(OWNER))
}

fn seed_applied_proxy(paths: &AppPaths) {
    use ice_proxy_sys::ProxyBackupFile;
    let backup = ProxyBackupFile {
        applied: true,
        pending_apply: false,
        applied_at: None,
        endpoints: ProxyEndpoints {
            http_host: "127.0.0.1".into(),
            http_port: 17890,
            socks_host: None,
            socks_port: None,
        },
        backup: ProxyBackup::default(),
    };
    backup.save(&paths.proxy_backup()).expect("seed backup");
}

fn seed_subscription(paths: &AppPaths) {
    use ice_subscription::{
        write_subscription_success, SubscriptionFormat, SubscriptionMeta, SubscriptionPaths,
    };
    let sub = SubscriptionPaths::from_app(paths);
    let meta = SubscriptionMeta {
        id: uuid::Uuid::new_v4(),
        name: "t".into(),
        url: "https://example.com/s".into(),
        active: true,
        format: SubscriptionFormat::SingBox,
        node_count: 1,
        group_count: 0,
        rule_count: 0,
        has_dns: false,
        parse_warnings: vec![],
        last_updated: None,
        last_error: None,
        etag: None,
        last_modified: None,
        auto_update: false,
        auto_update_interval: None,
    };
    let nodes = vec![ice_config::NormalizedOutbound {
        tag: "n1".into(),
        outbound: serde_json::json!({
            "type": "socks",
            "tag": "n1",
            "server": "127.0.0.1",
            "server_port": 1080
        }),
    }];
    write_subscription_success(
        &sub,
        &meta,
        "{}",
        &ice_config::NormalizedProfile::from_nodes_only(nodes),
    )
    .unwrap();
}

fn controller(paths: &AppPaths) -> CaptureController {
    CaptureController::with_backend_for_tests(paths.clone(), fake_backend())
}

#[test]
fn only_tun_enabled_changed_ignores_live_capture_desire() {
    let off = tun_settings(false);
    let on = tun_settings(true);
    assert!(
        only_tun_enabled_changed(&off, &on),
        "flipping tun.enabled alone is a next-start desire"
    );
    assert!(only_tun_enabled_changed(&on, &off));
    assert!(!only_tun_enabled_changed(&off, &off));
    assert!(!only_tun_enabled_changed(&on, &on));

    let mut on_and_port = on.clone();
    on_and_port.mixed_port = 17900;
    assert!(
        !only_tun_enabled_changed(&off, &on_and_port),
        "any other field change is not desire-only"
    );
}

#[test]
fn owner_token_is_stable_and_prefixed() {
    let paths = temp_paths("token");
    let a = tun_owner_token(&paths);
    let b = tun_owner_token(&paths);
    assert_eq!(a, b);
    assert!(a.starts_with("ice-box:"));
    assert_eq!(a.len(), "ice-box:".len() + 16);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn owner_token_survives_data_dir_relocation() {
    let dir = temp_paths("token-move");
    let token = tun_owner_token(&dir);
    assert!(is_valid_owner_token(&token));
    // Relocate the data dir: the persisted token moves with it, so the
    // same installation keeps its identity and an outstanding journal
    // stays recoverable (a path-derived token would change and strand it
    // as ForeignJournal with no in-app escape).
    let new_root = dir.root().with_file_name(format!(
        "{}-relocated",
        dir.root().file_name().unwrap().to_string_lossy()
    ));
    let relocated = AppPaths::new(&new_root);
    relocated.ensure_dirs().unwrap();
    fs::rename(
        dir.root().join(OWNER_TOKEN_FILE),
        relocated.root().join(OWNER_TOKEN_FILE),
    )
    .unwrap();
    assert_eq!(tun_owner_token(&relocated), token);
    let _ = fs::remove_dir_all(dir.root());
    let _ = fs::remove_dir_all(relocated.root());
}

#[test]
fn owner_token_adopts_an_existing_journal_token() {
    let dir = temp_paths("token-adopt");
    // A journal written before the token file existed (e.g. an upgraded
    // build) carries the installation's owner token; the controller must
    // adopt it instead of generating a fresh one, which would strand the
    // outstanding journal as foreign.
    let journal = TunJournal::new("t-old".into(), "ice-box:0123456789abcdef".into());
    journal.save(&dir.tun_state()).unwrap();
    assert_eq!(tun_owner_token(&dir), "ice-box:0123456789abcdef");
    assert_eq!(
        fs::read_to_string(dir.root().join(OWNER_TOKEN_FILE))
            .unwrap()
            .trim(),
        "ice-box:0123456789abcdef",
        "the adopted token is persisted for future launches"
    );
    let _ = fs::remove_dir_all(dir.root());
}

#[test]
fn status_reports_available_and_configured() {
    let paths = temp_paths("status");
    let c = controller(&paths);
    let settings = tun_settings(true);
    let status = c.status(&settings);
    assert_eq!(status.traffic_capture, TrafficCapture::Inactive);
    assert!(status.configured_tun);
    assert_eq!(status.tun_status, TunStatus::Disabled);
    assert!(status.tun_available);
    assert_eq!(status.tun_unavailable_reason, None);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn enable_tun_success_journals_applied_and_activates() {
    let paths = temp_paths("enable");
    seed_subscription(&paths);
    let c = controller(&paths);
    let mut core = TrackCore::running();
    let settings = tun_settings(true);

    c.enable_tun(&settings, &mut core, PathBuf::from("/bin/true"))
        .expect("enable");

    assert_eq!(c.active_backend(), TrafficCapture::Tun);
    assert_eq!(c.tun_status(), TunStatus::Enabled);
    assert!(
        c.helper_core_used(),
        "helper core log latched after a helper-managed TUN enable"
    );
    let journal = TunJournal::load(&paths.tun_state()).unwrap().unwrap();
    assert_eq!(journal.state, JournalState::Applied);
    assert_eq!(journal.last_completed_step, steps::VERIFY_APPLIED);
    assert_eq!(journal.interface_name.as_deref(), Some("utun420"));
    assert_eq!(core.stop_calls.get(), 1, "app-managed core released");
    assert_eq!(
        core.start_calls.get(),
        1,
        "fake backend does not start an external core; shell core restarts on the Tun config"
    );
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(paths.config()).unwrap()).unwrap();
    assert_eq!(
        config["inbounds"].as_array().unwrap().len(),
        2,
        "mixed + tun"
    );
    let on_disk = ice_config::load_settings(&paths.settings()).unwrap();
    assert_eq!(on_disk.tun.interface_name.as_deref(), Some("utun420"));
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn enable_tun_rejects_when_system_proxy_active() {
    let paths = temp_paths("enable-reject");
    let c = controller(&paths);
    c.set_system_proxy_active().unwrap();
    let mut core = TrackCore::running();
    let err = c
        .enable_tun(&tun_settings(true), &mut core, PathBuf::from("/bin/true"))
        .expect_err("exclusivity");
    assert_eq!(err.code, ErrorCode::TunApplyFailed.as_str());
    assert_eq!(c.active_backend(), TrafficCapture::SystemProxy);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn enable_tun_requires_running_core_and_cleans_journal() {
    let paths = temp_paths("enable-nocore");
    seed_subscription(&paths);
    let c = controller(&paths);
    let mut core = TrackCore::default(); // stopped
    let err = c
        .enable_tun(&tun_settings(true), &mut core, PathBuf::from("/bin/true"))
        .expect_err("core not running");
    assert_eq!(err.code, "core.invalid_state");
    let journal = TunJournal::load(&paths.tun_state()).unwrap().unwrap();
    assert_eq!(journal.state, JournalState::Clean, "nothing was mutated");
    assert_eq!(c.tun_status(), TunStatus::Error);
    assert_eq!(c.active_backend(), TrafficCapture::Inactive);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn enable_tun_permission_required_is_clean_and_reported() {
    let paths = temp_paths("enable-permission");
    seed_subscription(&paths);
    // An unsupported gate (Windows/Linux hosts) rejects before any mutation.
    let c = CaptureController::with_backend_for_tests(
        paths.clone(),
        Box::new(ice_tun_sys::UnsupportedTunBackend::new("gate pending")),
    );
    let mut core = TrackCore::running();
    let err = c
        .enable_tun(&tun_settings(true), &mut core, PathBuf::from("/bin/true"))
        .expect_err("unsupported");
    assert_eq!(err.code, ErrorCode::TunNotSupported.as_str());
    assert!(!paths.tun_state().exists());
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn disable_tun_restores_diagnostic_and_cleans_journal() {
    let paths = temp_paths("disable");
    seed_subscription(&paths);
    let c = controller(&paths);
    let mut core = TrackCore::running();
    let settings = tun_settings(true);
    c.enable_tun(&settings, &mut core, PathBuf::from("/bin/true"))
        .expect("enable");
    core.start_calls.set(0);
    core.stop_calls.set(0);

    let proxy = TrackProxy::default();
    c.disable_active_backend(
        &settings,
        &mut core,
        &proxy,
        PathBuf::from("/bin/true"),
        true,
    )
    .expect("disable");

    assert_eq!(c.active_backend(), TrafficCapture::Inactive);
    assert_eq!(c.tun_status(), TunStatus::Disabled);
    let journal = TunJournal::load(&paths.tun_state()).unwrap().unwrap();
    assert_eq!(journal.state, JournalState::Clean);
    assert_eq!(journal.interface_name, None);
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(paths.config()).unwrap()).unwrap();
    assert_eq!(
        config["inbounds"].as_array().unwrap().len(),
        1,
        "diagnostic config has no tun inbound"
    );
    assert_eq!(core.stop_calls.get(), 1);
    assert_eq!(
        core.start_calls.get(),
        1,
        "app core restarted on Diagnostic"
    );
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn disable_system_proxy_backend_restores_os_proxy() {
    let paths = temp_paths("disable-proxy");
    let c = controller(&paths);
    seed_applied_proxy(&paths);
    c.set_system_proxy_active().unwrap();
    let proxy = TrackProxy::default();
    let mut core = TrackCore::running();
    c.disable_active_backend(
        &AppSettings::default(),
        &mut core,
        &proxy,
        PathBuf::from("/bin/true"),
        true,
    )
    .expect("disable proxy");
    assert_eq!(proxy.restore_calls.get(), 1);
    assert_eq!(c.active_backend(), TrafficCapture::Inactive);
    assert_eq!(core.stop_calls.get(), 0, "core keeps running");
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn disable_tun_fail_closed_when_cleanup_uncertain() {
    let paths = temp_paths("disable-stuck");
    seed_subscription(&paths);
    let c = controller(&paths);
    let mut core = TrackCore::running();
    let settings = tun_settings(true);
    c.enable_tun(&settings, &mut core, PathBuf::from("/bin/true"))
        .expect("enable");

    // Swap in a backend whose restore fails before any mutation (uncertain
    // cleanup cannot claim success).
    let mut stuck = FakeTunBackend::new(OWNER);
    stuck.faults.fail_restore_after_mutations = Some(0);
    *c.backend.lock().unwrap() = Box::new(stuck);

    let proxy = TrackProxy::default();
    let err = c
        .disable_active_backend(
            &settings,
            &mut core,
            &proxy,
            PathBuf::from("/bin/true"),
            true,
        )
        .expect_err("restore uncertain");
    assert_eq!(err.code, ErrorCode::TunRecoveryRequired.as_str());
    assert_eq!(c.tun_status(), TunStatus::RecoveryRequired);
    let journal = TunJournal::load(&paths.tun_state()).unwrap().unwrap();
    assert_eq!(journal.state, JournalState::RecoveryRequired);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn enable_tun_fail_closed_when_apply_mutated_but_failed() {
    let paths = temp_paths("enable-mutated");
    seed_subscription(&paths);
    // The fake mutates the OS (interface created) and then fails: cleanup
    // is unverified, so the controller must enter RecoveryRequired, not a
    // retryable Error, and a new enable must be rejected.
    let mut failing = FakeTunBackend::new(OWNER);
    failing.faults.fail_apply_after_mutations = Some(1);
    let c = CaptureController::with_backend_for_tests(paths.clone(), Box::new(failing));
    let mut core = TrackCore::running();
    let settings = tun_settings(true);

    let err = c
        .enable_tun(&settings, &mut core, PathBuf::from("/bin/true"))
        .expect_err("apply failed after a mutation");
    assert_eq!(err.code, ErrorCode::TunApplyFailed.as_str());
    assert_eq!(
        c.tun_status(),
        TunStatus::RecoveryRequired,
        "an unverified mutation must fail closed"
    );
    assert_eq!(c.active_backend(), TrafficCapture::Inactive);
    let journal = TunJournal::load(&paths.tun_state()).unwrap().unwrap();
    assert!(
        journal.interface_name.is_some(),
        "journal keeps the unverified ownership records"
    );

    // A retry without recovery must be rejected (the journal guard must
    // not let a new transition overwrite the outstanding records).
    let err = c
        .enable_tun(&settings, &mut core, PathBuf::from("/bin/true"))
        .expect_err("retry before recovery");
    assert_eq!(err.code, ErrorCode::TunRecoveryRequired.as_str());

    // Recovery converges the journal and re-enables capture.
    c.recover(&mut core).expect("recover");
    assert_eq!(c.tun_status(), TunStatus::Disabled);
    {
        let mut backend = c.backend.lock().unwrap();
        let fake = backend
            .as_any_mut()
            .downcast_mut::<FakeTunBackend>()
            .expect("fake");
        fake.faults.fail_apply_after_mutations = None;
    }
    c.enable_tun(&settings, &mut core, PathBuf::from("/bin/true"))
        .expect("enable after recovery");
    assert_eq!(c.tun_status(), TunStatus::Enabled);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn enable_tun_fail_with_ambiguous_journal_fails_closed() {
    let paths = temp_paths("enable-premain");
    seed_subscription(&paths);
    // A backend failure leaves a non-clean journal whose mutation boundary
    // cannot be proven from the controller. The conservative result is
    // RecoveryRequired; recovery verifies the empty ownership set before
    // allowing another activation.
    let mut failing = FakeTunBackend::new(OWNER);
    failing.faults.fail_apply_after_mutations = Some(0);
    let c = CaptureController::with_backend_for_tests(paths.clone(), Box::new(failing));
    let mut core = TrackCore::running();
    let settings = tun_settings(true);

    let err = c
        .enable_tun(&settings, &mut core, PathBuf::from("/bin/true"))
        .expect_err("apply failed before any mutation");
    assert_eq!(err.code, ErrorCode::TunApplyFailed.as_str());
    assert_eq!(
        c.tun_status(),
        TunStatus::RecoveryRequired,
        "an ambiguous journal must fail closed"
    );
    assert_eq!(c.active_backend(), TrafficCapture::Inactive);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn unexpected_exit_cleans_capture_and_writes_diagnostic_config() {
    let paths = temp_paths("exit");
    seed_subscription(&paths);
    let c = controller(&paths);
    let mut core = TrackCore::running();
    let settings = tun_settings(true);
    c.enable_tun(&settings, &mut core, PathBuf::from("/bin/true"))
        .expect("enable");

    // sing-box died; the kernel (macOS) removed interface + routes.
    {
        let mut backend = c.backend.lock().unwrap();
        let fake = backend
            .as_any_mut()
            .downcast_mut::<FakeTunBackend>()
            .expect("fake");
        fake.state = FakeOsState::default();
    }
    let warning = c.handle_unexpected_core_exit(&mut core, &settings);
    assert!(warning.is_none(), "cleanup confirmed");
    assert_eq!(c.active_backend(), TrafficCapture::Inactive);
    let journal = TunJournal::load(&paths.tun_state()).unwrap().unwrap();
    assert_eq!(journal.state, JournalState::Clean);
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(paths.config()).unwrap()).unwrap();
    assert_eq!(
        config["inbounds"].as_array().unwrap().len(),
        1,
        "diagnostic"
    );
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn transition_from_system_proxy_to_tun_commits_settings() {
    let paths = temp_paths("transition-on");
    seed_subscription(&paths);
    let c = controller(&paths);
    let previous = AppSettings::default();
    let candidate = tun_settings(true);
    let mut core = TrackCore::running();
    let proxy = TrackProxy::default();

    c.set_system_proxy_active().unwrap();
    seed_applied_proxy(&paths);
    c.transition_tun_settings(
        &previous,
        &candidate,
        &mut core,
        &proxy,
        PathBuf::from("/bin/true"),
    )
    .expect("transition");
    assert_eq!(proxy.restore_calls.get(), 1, "old backend disabled first");
    assert_eq!(c.active_backend(), TrafficCapture::Tun);
    let on_disk = ice_config::load_settings(&paths.settings()).unwrap();
    assert!(on_disk.tun.enabled, "committed only after healthy");
    assert!(!paths.pending_settings().exists());
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn transition_no_op_persists_candidate_without_backend_churn() {
    let paths = temp_paths("transition-noop");
    seed_subscription(&paths);
    let c = controller(&paths);
    let mut core = TrackCore::running();
    let settings = tun_settings(true);
    c.enable_tun(&settings, &mut core, PathBuf::from("/bin/true"))
        .expect("enable");
    core.stop_calls.set(0);
    core.start_calls.set(0);

    // TUN is active but the committed settings still say disabled (e.g. a
    // rollback left them behind): the transition must fall through to a
    // plain persist instead of surfacing a bogus save failure.
    let previous = AppSettings::default();
    let candidate = tun_settings(true);
    let proxy = TrackProxy::default();
    c.transition_tun_settings(
        &previous,
        &candidate,
        &mut core,
        &proxy,
        PathBuf::from("/bin/true"),
    )
    .expect("no-op transition persists");

    assert_eq!(c.active_backend(), TrafficCapture::Tun, "capture untouched");
    assert_eq!(c.tun_status(), TunStatus::Enabled);
    assert_eq!(core.stop_calls.get(), 0, "no backend transition");
    assert_eq!(core.start_calls.get(), 0, "no backend transition");
    assert!(!paths.pending_settings().exists());
    let on_disk = ice_config::load_settings(&paths.settings()).unwrap();
    assert!(on_disk.tun.enabled, "candidate committed");
    let journal = TunJournal::load(&paths.tun_state()).unwrap().unwrap();
    assert_eq!(journal.state, JournalState::Applied);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn transition_failure_rolls_back_old_backend_and_clears_pending() {
    let paths = temp_paths("transition-rollback");
    seed_subscription(&paths);
    // The enable fails at the backend apply step (before any OS mutation
    // in the fake), so the rollback can restore the system proxy cleanly.
    let mut failing = FakeTunBackend::new(OWNER);
    failing.faults.fail_apply_after_mutations = Some(0);
    let c = CaptureController::with_backend_for_tests(paths.clone(), Box::new(failing));
    let previous = AppSettings::default();
    let candidate = tun_settings(true);
    let mut core = TrackCore::running();
    let proxy = TrackProxy::default();

    c.set_system_proxy_active().unwrap();
    let err = c
        .transition_tun_settings(
            &previous,
            &candidate,
            &mut core,
            &proxy,
            PathBuf::from("/bin/true"),
        )
        .expect_err("transition failed");
    assert_eq!(err.code, ErrorCode::TunApplyFailed.as_str());
    assert!(!paths.pending_settings().exists(), "pending cleared");
    assert_eq!(
        proxy.apply_calls.get(),
        1,
        "old backend restored after the failed transition"
    );
    assert_eq!(
        c.active_backend(),
        TrafficCapture::SystemProxy,
        "rollback confirmed"
    );
    let on_disk = ice_config::load_settings(&paths.settings()).unwrap();
    assert!(
        !on_disk.tun.enabled,
        "settings.json must never commit the failed candidate"
    );
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn startup_recovery_discards_pending_and_converges_journal() {
    let paths = temp_paths("startup");
    seed_subscription(&paths);
    let c = controller(&paths);
    let mut core = TrackCore::running();
    let settings = tun_settings(true);
    c.enable_tun(&settings, &mut core, PathBuf::from("/bin/true"))
        .expect("enable");

    // Simulate a crash: journal says applied; the kernel cleaned resources.
    {
        let mut backend = c.backend.lock().unwrap();
        let fake = backend
            .as_any_mut()
            .downcast_mut::<FakeTunBackend>()
            .expect("fake");
        fake.state = FakeOsState::default();
    }
    // Interrupted settings transaction record.
    write_json_atomic(
        &paths.pending_settings(),
        &PendingSettingsRecord {
            candidate: settings.clone(),
            created_at: "now".into(),
        },
    )
    .unwrap();

    let warning = c.recover(&mut core).expect("recover");
    assert!(warning.is_some(), "pending transaction surfaced");
    assert!(!paths.pending_settings().exists());
    let journal = TunJournal::load(&paths.tun_state()).unwrap().unwrap();
    assert_eq!(journal.state, JournalState::Clean);
    assert_eq!(c.tun_status(), TunStatus::Disabled);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn startup_recovery_fail_closed_when_cleanup_uncertain() {
    let paths = temp_paths("startup-stuck");
    seed_subscription(&paths);
    let c = controller(&paths);
    let mut core = TrackCore::running();
    let settings = tun_settings(true);
    c.enable_tun(&settings, &mut core, PathBuf::from("/bin/true"))
        .expect("enable");
    {
        let mut backend = c.backend.lock().unwrap();
        let fake = backend
            .as_any_mut()
            .downcast_mut::<FakeTunBackend>()
            .expect("fake");
        fake.faults.stuck_route = Some("128.0.0.0/1".into());
    }

    let warning = c.recover(&mut core).expect("recover runs");
    assert!(warning.is_some());
    assert_eq!(c.tun_status(), TunStatus::RecoveryRequired);
    let journal = TunJournal::load(&paths.tun_state()).unwrap().unwrap();
    assert_eq!(journal.state, JournalState::RecoveryRequired);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn topology_change_goes_through_stop_reconfigure_start() {
    let paths = temp_paths("topology");
    seed_subscription(&paths);
    let c = controller(&paths);
    let mut core = TrackCore::running();
    let settings = tun_settings(true);
    c.enable_tun(&settings, &mut core, PathBuf::from("/bin/true"))
        .expect("enable");
    let mut candidate = settings.clone();
    candidate.tun.mtu = 1400;

    let proxy = TrackProxy::default();
    c.transition_tun_settings(
        &settings,
        &candidate,
        &mut core,
        &proxy,
        PathBuf::from("/bin/true"),
    )
    .expect("topology transition");
    assert_eq!(c.active_backend(), TrafficCapture::Tun, "capture restored");
    assert_eq!(
        ice_config::load_settings(&paths.settings())
            .unwrap()
            .tun
            .mtu,
        1400,
        "committed after re-enable"
    );
    assert_eq!(
        core.stop_calls.get(),
        3,
        "enable + disable + re-enable each release the app-managed core"
    );
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn apply_while_tun_active_reconfigures_and_reverifies() {
    let paths = temp_paths("apply-tun");
    seed_subscription(&paths);
    let c = controller(&paths);
    let mut core = TrackCore::running();
    let previous = tun_settings(true);
    c.enable_tun(&previous, &mut core, PathBuf::from("/bin/true"))
        .expect("enable");

    core.start_calls.set(0);
    core.stop_calls.set(0);
    let mut settings = previous.clone();
    settings.mixed_port = 17900;
    let proxy = TrackProxy::default();
    c.apply_while_tun_active(
        &settings,
        &previous,
        &mut core,
        &proxy,
        PathBuf::from("/bin/true"),
    )
    .expect("policy apply");

    assert_eq!(c.active_backend(), TrafficCapture::Tun);
    assert_eq!(c.tun_status(), TunStatus::Enabled);
    assert_eq!(core.stop_calls.get(), 2, "disable + re-enable");
    assert_eq!(core.start_calls.get(), 2, "diagnostic + tun configs");
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(paths.config()).unwrap()).unwrap();
    assert_eq!(
        config["inbounds"].as_array().unwrap().len(),
        2,
        "the Tun config must be regenerated, never the Mixed-only one"
    );
    let journal = TunJournal::load(&paths.tun_state()).unwrap().unwrap();
    assert_eq!(journal.state, JournalState::Applied);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn apply_while_tun_active_failure_rolls_back_previous_tun() {
    let paths = temp_paths("apply-tun-rollback");
    seed_subscription(&paths);
    let mut backend = ScriptedBackend::new();
    backend.core_pid = Some(4242);
    let c = CaptureController::with_backend_for_tests(paths.clone(), Box::new(backend));
    let mut core = TrackCore::running();
    let previous = tun_settings(true);
    c.enable_tun(&previous, &mut core, PathBuf::from("/bin/true"))
        .expect("enable");
    assert_eq!(core.adopt_pids.borrow().as_slice(), &[4242]);

    // The re-apply fails (one-shot); the rollback must re-apply the
    // previous TUN settings so capture stays enabled.
    {
        let mut backend = c.backend.lock().unwrap();
        let scripted = backend
            .as_any_mut()
            .downcast_mut::<ScriptedBackend>()
            .expect("scripted");
        scripted.fail_next_apply.set(true);
    }
    let mut settings = previous.clone();
    settings.mixed_port = 17900;
    let proxy = TrackProxy::default();
    let err = c
        .apply_while_tun_active(
            &settings,
            &previous,
            &mut core,
            &proxy,
            PathBuf::from("/bin/true"),
        )
        .expect_err("re-apply fails");
    assert_eq!(err.code, ErrorCode::TunApplyFailed.as_str());
    assert_eq!(
        c.active_backend(),
        TrafficCapture::Tun,
        "rollback re-enabled the previous TUN settings"
    );
    assert_eq!(c.tun_status(), TunStatus::Enabled);
    let journal = TunJournal::load(&paths.tun_state()).unwrap().unwrap();
    assert_eq!(journal.state, JournalState::Applied);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn enable_tun_adopt_failure_releases_backend_and_restores_app_core() {
    let paths = temp_paths("adopt-fail");
    seed_subscription(&paths);
    let mut backend = ScriptedBackend::new();
    backend.core_pid = Some(4242);
    let c = CaptureController::with_backend_for_tests(paths.clone(), Box::new(backend));
    let mut core = TrackCore::running();
    core.fail_adopt.set(true);
    let settings = tun_settings(true);

    let err = c
        .enable_tun(&settings, &mut core, PathBuf::from("/bin/true"))
        .expect_err("adopt fails");
    assert_eq!(err.code, "core.spawn_failed");
    assert_eq!(c.active_backend(), TrafficCapture::Inactive);
    // The elevated core was started and owned the resources: the release
    // must be verified before the failure is surfaced.
    let journal = TunJournal::load(&paths.tun_state()).unwrap().unwrap();
    assert_eq!(
        journal.state,
        JournalState::Clean,
        "verified release closes the journal so a retry is possible"
    );
    assert_eq!(
        core.start_calls.get(),
        1,
        "the app-managed core is restarted on the Diagnostic config"
    );
    assert_eq!(core.state().status, CoreStatus::Running);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn enable_tun_fails_closed_on_unreadable_journal() {
    let paths = temp_paths("journal-corrupt");
    seed_subscription(&paths);
    fs::write(paths.tun_state(), b"{not json").unwrap();
    let c = controller(&paths);
    let mut core = TrackCore::running();

    let err = c
        .enable_tun(&tun_settings(true), &mut core, PathBuf::from("/bin/true"))
        .expect_err("unreadable journal");
    assert_eq!(err.code, ErrorCode::TunRecoveryRequired.as_str());
    // The corrupt journal is untouched: no transition overwrote the only
    // record of possibly-owned resources.
    assert_eq!(
        fs::read_to_string(paths.tun_state()).unwrap(),
        "{not json",
        "journal must never be overwritten when it cannot be read"
    );
    assert_eq!(c.tun_status(), TunStatus::RecoveryRequired);
    assert_eq!(core.stop_calls.get(), 0);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn recover_keeps_system_proxy_active_when_backup_still_applied() {
    let paths = temp_paths("recover-proxy");
    let c = controller(&paths);
    seed_applied_proxy(&paths);
    c.set_system_proxy_active().unwrap();
    let mut core = TrackCore::running();

    c.recover(&mut core).expect("recover");
    assert_eq!(
        c.active_backend(),
        TrafficCapture::SystemProxy,
        "startup recovery must not clobber the still-applied system proxy"
    );
    assert_eq!(c.tun_status(), TunStatus::Disabled);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn system_proxy_enable_fails_closed_on_unreadable_tun_journal() {
    let paths = temp_paths("proxy-journal-corrupt");
    fs::write(paths.tun_state(), b"{not json").unwrap();
    let c = controller(&paths);
    let core = TrackCore::running();
    let proxy = TrackProxy::default();

    let err = c
        .enable_system_proxy(&AppSettings::default(), &core, &proxy)
        .expect_err("system proxy must not bypass an unreadable TUN journal");
    assert_eq!(err.code, ErrorCode::TunRecoveryRequired.as_str());
    assert_eq!(c.tun_status(), TunStatus::RecoveryRequired);
    assert_eq!(c.active_backend(), TrafficCapture::Inactive);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn unexpected_exit_with_missing_journal_fails_closed() {
    let paths = temp_paths("exit-nojournal");
    seed_subscription(&paths);
    let c = controller(&paths);
    let mut core = TrackCore::running();
    let settings = tun_settings(true);
    c.enable_tun(&settings, &mut core, PathBuf::from("/bin/true"))
        .expect("enable");
    fs::remove_file(paths.tun_state()).unwrap();

    let warning = c.handle_unexpected_core_exit(&mut core, &settings);
    assert!(warning.is_some(), "a warning must be surfaced");
    assert_eq!(
        c.tun_status(),
        TunStatus::RecoveryRequired,
        "a lost journal while TUN was claimed must fail closed"
    );
    assert_eq!(c.active_backend(), TrafficCapture::Inactive);
    let _ = fs::remove_dir_all(paths.root());
}

#[test]
fn transition_commit_failure_rolls_back_and_keeps_pending_record() {
    let paths = temp_paths("commit-fail");
    seed_subscription(&paths);
    let c = controller(&paths);
    // Pre-reconciled selected tag: generate_config never re-saves
    // settings mid-transition, so the directory squat below fails exactly
    // the final commit and nothing else.
    let previous = AppSettings {
        selected_tag: Some("n1".into()),
        ..AppSettings::default()
    };
    let candidate = AppSettings {
        selected_tag: Some("n1".into()),
        tun: TunSettings {
            enabled: true,
            interface_name: Some("utun420".into()),
            ..TunSettings::default()
        },
        ..previous.clone()
    };
    let mut core = TrackCore::running();
    let proxy = TrackProxy::default();
    c.set_system_proxy_active().unwrap();
    seed_applied_proxy(&paths);

    // Make the settings commit impossible: a directory squats on the
    // settings.json path, so save_settings fails after the transition.
    fs::create_dir_all(paths.settings()).unwrap();

    let err = c
        .transition_tun_settings(
            &previous,
            &candidate,
            &mut core,
            &proxy,
            PathBuf::from("/bin/true"),
        )
        .expect_err("commit fails");
    assert!(
        !err.code.is_empty() && !err.message.is_empty(),
        "the commit failure must be surfaced, not swallowed: {err}"
    );
    assert!(
        paths.pending_settings().exists(),
        "the pending record must survive a commit failure so startup recovery knows settings.json is stale"
    );
    assert_eq!(
        proxy.apply_calls.get(),
        1,
        "the old system-proxy backend is restored after the failed commit"
    );
    assert_eq!(c.active_backend(), TrafficCapture::SystemProxy);
    let journal = TunJournal::load(&paths.tun_state()).unwrap().unwrap();
    assert_eq!(journal.state, JournalState::Clean);
    let _ = fs::remove_dir_all(paths.root());
}
