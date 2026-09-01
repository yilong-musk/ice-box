//! TUN capture runtime controller (plan §4.3, slice T3).
//!
//! `CaptureController` is the single owner of the active capture backend and
//! the TUN capture state machine. Every start / stop / apply / reload / quit /
//! crash-recovery path reads it; no path infers the active backend from
//! `tun.enabled`, `settings.json`, or `proxy-backup.json`.
//!
//! ```text
//! Capture: Disabled -> Preparing -> Enabled -> Stopping -> Disabled
//!                          \-> PermissionRequired / Error / RecoveryRequired
//! ```
//!
//! `RecoveryRequired` is fail-closed: both capture backends stay disabled and
//! new TUN activation is rejected until an explicit recovery attempt succeeds.
//!
//! Transition ownership: methods here run under the orchestration lock held by
//! the command layer. The controller keeps its own small locks for state and
//! the platform backend so status reads never block transitions.

use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ice_config::{
    save_settings, write_json_atomic, AppError, AppPaths, AppSettings, CaptureIntent, ErrorCode,
    TunSettings,
};
use ice_core::{CoreHandle, CoreStatus};
use ice_proxy_sys::{is_proxy_applied_on_disk, SystemProxy};
use ice_tun_sys::{
    create_backend, steps, AppliedTun, JournalState, RecoveryDriver, RecoveryOutcome, TunBackend,
    TunCapability, TunConfig, TunError, TunErrorCode, TunJournal, TunStack,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::orchestrate::{
    build_core_paths, generate_config, orchestrate_disable_system_proxy,
    orchestrate_enable_system_proxy,
};

fn lock_poisoned(context: &str) -> AppError {
    AppError::new(
        ErrorCode::ConfigInvalid,
        format!("internal lock poisoned: {context}"),
    )
}

fn map_tun(err: TunError) -> AppError {
    AppError::with_code(err.code.as_str(), err.message)
}

/// The backend that currently captures traffic (plan §4.3 status payload).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrafficCapture {
    /// No capture backend is claimed (core may still run for diagnostics).
    Inactive,
    /// The OS HTTP/HTTPS/SOCKS proxy is applied.
    SystemProxy,
    /// TUN capture is active.
    Tun,
}

/// TUN capture lifecycle (plan §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TunStatus {
    Disabled,
    Preparing,
    Enabled,
    Stopping,
    PermissionRequired,
    Error,
    RecoveryRequired,
}

/// Status payload fragment (plan §4.3). `traffic_capture` is derived only
/// from the controller; `configured_tun` is the committed settings desire.
#[derive(Debug, Clone, Serialize)]
pub struct CaptureStatus {
    pub traffic_capture: TrafficCapture,
    pub configured_tun: bool,
    pub tun_status: TunStatus,
    pub tun_interface: Option<String>,
    pub tun_error: Option<AppError>,
    pub capture_transition_id: Option<String>,
    pub tun_available: bool,
    pub tun_unavailable_reason: Option<String>,
    /// True when the platform must not surface TUN controls at all. Currently
    /// Windows: the T0 gate is blocked upstream (`docs/design-notes/
    /// tun-windows-t0.md`), so the production backend reports
    /// `supported=false` and the controls stay hidden. Derived from the
    /// backend capability rather than a bare `cfg!` so the controls
    /// reappear automatically when `windows_tun_ready` flips green, and so
    /// the `ICE_BOX_TUN_WINDOWS_DEV` opt-in (capability supported) exposes
    /// the controls to exercise the backend from the UI.
    pub tun_ui_hidden: bool,
}

/// Interrupted settings-transaction record (plan §4.3). `settings.json` is
/// never touched until the transition succeeds, so a leftover record on
/// startup simply means "the committed settings are still the old state".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSettingsRecord {
    pub candidate: AppSettings,
    pub created_at: String,
}

struct CaptureInner {
    active: TrafficCapture,
    tun_status: TunStatus,
    transition_id: Option<String>,
    tun_interface: Option<String>,
    tun_error: Option<AppError>,
    /// Whether the helper has run the elevated core at least once this app
    /// session; latched, never cleared, so the log view keeps merging the
    /// helper core log after TUN capture stops.
    helper_core_used: bool,
}

/// The runtime capture controller (plan §4.3).
pub struct CaptureController {
    paths: AppPaths,
    owner: String,
    resource_dir: Option<PathBuf>,
    backend: Mutex<Box<dyn TunBackend + Send>>,
    /// Cached static capability report. Reads never take the backend lock, so
    /// status polls cannot queue behind a transition (the backend mutex is
    /// held for the whole enable/disable convergence window).
    capability: Mutex<TunCapability>,
    inner: Mutex<CaptureInner>,
}

/// File name of the per-installation owner token inside the app data dir.
/// Persisted so the token survives data-dir relocations (a path-derived
/// token would invalidate a non-clean journal after a move, stranding it as
/// `ForeignJournal` with no in-app escape).
pub const OWNER_TOKEN_FILE: &str = "installation-id";

fn is_valid_owner_token(token: &str) -> bool {
    token
        .strip_prefix("ice-box:")
        .is_some_and(|hex| hex.len() == 16 && hex.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Stable per-installation owner token: `ice-box:<64-bit hex>`, persisted in
/// the app data dir. Recovery refuses journals from other installations.
///
/// The token is a persisted random id, not a hash of the data-dir path, so
/// moving/renaming the data dir keeps the token stable and a non-clean
/// journal stays recoverable instead of becoming `ForeignJournal`.
pub fn tun_owner_token(paths: &AppPaths) -> String {
    let _ = fs::create_dir_all(paths.root());
    let token_path = paths.root().join(OWNER_TOKEN_FILE);
    if let Ok(existing) = fs::read_to_string(&token_path) {
        if is_valid_owner_token(existing.trim()) {
            return existing.trim().to_string();
        }
    }
    // Migration: a journal written before the token file existed carries the
    // installation's owner token. Adopt it so an outstanding journal is not
    // stranded as foreign by the move to a persisted token.
    if let Ok(Some(journal)) = TunJournal::load(&paths.tun_state()) {
        if is_valid_owner_token(&journal.owner_token) {
            let _ = fs::write(&token_path, format!("{}\n", journal.owner_token));
            return journal.owner_token;
        }
    }
    // Fresh installation: generate and persist a random token. Best effort —
    // a read-only data dir falls back to the path hash so the app still
    // starts (TUN transitions would fail on the journal write anyway).
    let token = format!("ice-box:{:016x}", (Uuid::new_v4().as_u128() >> 64) as u64);
    if fs::write(&token_path, format!("{token}\n")).is_ok() {
        token
    } else {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        paths.root().to_string_lossy().hash(&mut hasher);
        format!("ice-box:{:016x}", hasher.finish())
    }
}

fn tun_config_from_settings(settings: &AppSettings) -> TunConfig {
    let stack = match settings.tun.stack.as_str() {
        "system" => TunStack::System,
        "mixed" => TunStack::Mixed,
        _ => TunStack::Gvisor,
    };
    TunConfig {
        interface_name: settings.tun.interface_name.clone(),
        addresses: vec![
            settings.tun.ipv4_address.clone(),
            settings.tun.ipv6_address.clone(),
        ],
        mtu: settings.tun.mtu,
        stack,
        auto_route: settings.tun.auto_route,
        strict_route: settings.tun.strict_route,
        dns_hijack: settings.tun.dns_hijack,
    }
}

/// Whether two TUN settings change the capture topology (plan §4.3). The
/// interface name is excluded: it is resolved per transition by the backend.
pub fn tun_topology_changed(a: &TunSettings, b: &TunSettings) -> bool {
    a.ipv4_address != b.ipv4_address
        || a.ipv6_address != b.ipv6_address
        || a.mtu != b.mtu
        || a.stack != b.stack
        || a.dns_hijack != b.dns_hijack
        || a.auto_route != b.auto_route
        || a.strict_route != b.strict_route
}

impl CaptureController {
    pub fn new(paths: AppPaths, resource_dir: Option<PathBuf>) -> Self {
        let owner = tun_owner_token(&paths);
        // Resolve the bundled binary at construction so the dev `sudo`
        // runner (`ICE_BOX_TUN_DEV_SUDO`) can spawn the elevated core; the
        // enable path resolves the binary again and surfaces a clean error
        // when it is missing.
        let binary = crate::orchestrate::resolve_binary(resource_dir.as_deref()).ok();
        let mut backend = create_backend(&owner, paths.config(), binary, paths.core_log());
        backend.attach_journal(paths.tun_state());
        let capability = backend.capability();
        Self {
            paths,
            owner,
            resource_dir,
            backend: Mutex::new(backend),
            capability: Mutex::new(capability),
            inner: Mutex::new(CaptureInner {
                active: TrafficCapture::Inactive,
                tun_status: TunStatus::Disabled,
                transition_id: None,
                tun_interface: None,
                tun_error: None,
                helper_core_used: false,
            }),
        }
    }

    #[cfg(test)]
    pub fn with_backend_for_tests(
        paths: AppPaths,
        mut backend: Box<dyn TunBackend + Send>,
    ) -> Self {
        // Host-free controller tests run on every CI host with an injected
        // (fake) backend; the compile-time platform TUN gate in ice-config
        // would otherwise reject Tun config generation on non-macOS runners
        // before the fake is ever exercised.
        ice_config::force_tun_gate_ready();
        let owner = tun_owner_token(&paths);
        backend.attach_journal(paths.tun_state());
        let capability = backend.capability();
        Self {
            paths,
            owner,
            resource_dir: None,
            backend: Mutex::new(backend),
            capability: Mutex::new(capability),
            inner: Mutex::new(CaptureInner {
                active: TrafficCapture::Inactive,
                tun_status: TunStatus::Disabled,
                transition_id: None,
                tun_interface: None,
                tun_error: None,
                helper_core_used: false,
            }),
        }
    }

    pub fn active_backend(&self) -> TrafficCapture {
        self.inner
            .lock()
            .map(|inner| inner.active)
            .unwrap_or(TrafficCapture::Inactive)
    }

    /// Whether the helper has run the elevated core this app session (latched
    /// on the first helper-managed TUN transition). The log view merges the
    /// helper core log while this holds, so a finished TUN session's core
    /// lines stay visible after capture stops.
    pub fn helper_core_used(&self) -> bool {
        self.inner
            .lock()
            .map(|inner| inner.helper_core_used)
            .unwrap_or(false)
    }

    /// Resource dir handed to backend construction (bundle resources in
    /// production, `None` in tests); used by helper-core drift checks.
    pub fn resource_dir(&self) -> Option<&Path> {
        self.resource_dir.as_deref()
    }

    /// Rebuild the platform backend while no capture backend is active and no
    /// transition is in flight. The helper coordinator is probed at
    /// construction, so this refresh allows a helper installed or repaired
    /// while the app is open to become usable on the next start/recovery
    /// attempt without replacing an active backend.
    ///
    /// A transition in flight (`Preparing` / `Stopping`) is not yet reflected
    /// in `active_backend()` (it is set only at `finish_transition`), but the
    /// in-flight instance still owns transition state — e.g. the dev `sudo`
    /// coordinator's pid/child handle — so swapping would make the next stop
    /// a no-op that fails closed into `RecoveryRequired`. The helper path is
    /// safe (the daemon holds the core state), but the guard applies to both.
    pub fn refresh_backend(&self) -> Result<(), AppError> {
        if self.active_backend() != TrafficCapture::Inactive {
            return Ok(());
        }
        if !matches!(self.tun_status(), TunStatus::Disabled) {
            return Ok(());
        }
        let binary = crate::orchestrate::resolve_binary(self.resource_dir.as_deref()).ok();
        let mut backend = create_backend(
            &self.owner,
            self.paths.config(),
            binary,
            self.paths.core_log(),
        );
        backend.attach_journal(self.paths.tun_state());
        let capability = backend.capability();
        let mut slot = self
            .backend
            .lock()
            .map_err(|_| lock_poisoned("capture backend"))?;
        *slot = backend;
        let mut cached = self
            .capability
            .lock()
            .map_err(|_| lock_poisoned("capture capability"))?;
        *cached = capability;
        Ok(())
    }

    pub fn tun_status(&self) -> TunStatus {
        self.inner
            .lock()
            .map(|inner| inner.tun_status)
            .unwrap_or(TunStatus::Disabled)
    }

    /// The capture intent to use when regenerating config for a normal
    /// (policy-only) apply: TUN while TUN is owned, Diagnostic otherwise.
    pub fn apply_intent(&self) -> CaptureIntent {
        match self.active_backend() {
            TrafficCapture::Tun => CaptureIntent::Tun,
            _ => CaptureIntent::Diagnostic,
        }
    }

    /// Report capture state; never fails (status polls must not error). The
    /// capability is cached at backend (re)build so this never blocks behind
    /// a transition holding the backend mutex.
    pub fn status(&self, settings: &AppSettings) -> CaptureStatus {
        let capability = self
            .capability
            .lock()
            .ok()
            .map(|capability| capability.clone())
            .unwrap_or_else(|| ice_tun_sys::unsupported_capability("controller unavailable"));
        let inner = self.inner.lock().ok().map(|i| {
            (
                i.active,
                i.tun_status,
                i.tun_interface.clone(),
                i.tun_error.clone(),
                i.transition_id.clone(),
            )
        });
        let (traffic_capture, tun_status, tun_interface, tun_error, transition_id) = match inner {
            Some(value) => value,
            None => (
                TrafficCapture::Inactive,
                TunStatus::Disabled,
                None,
                None,
                None,
            ),
        };
        CaptureStatus {
            traffic_capture,
            configured_tun: settings.tun.enabled,
            tun_status,
            tun_interface,
            tun_error,
            capture_transition_id: transition_id,
            tun_available: capability.supported,
            tun_unavailable_reason: capability.reason,
            // Windows hides TUN controls while the gate is pending. Deriving
            // from the backend capability (not a bare `cfg!(windows)`)
            // removes the coupling to `windows_tun_ready`: when
            // `create_backend` turns green the controls reappear here without
            // a second edit, and the `ICE_BOX_TUN_WINDOWS_DEV` dev runner
            // (capability supported) keeps the controls visible so the dev
            // backend stays exercisable and disableable from the UI.
            tun_ui_hidden: cfg!(target_os = "windows") && !capability.supported,
        }
    }

    /// Mark the system proxy as the active backend after a successful apply
    /// (used by the Home start path).
    pub fn set_system_proxy_active(&self) -> Result<(), AppError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| lock_poisoned("capture state"))?;
        inner.active = TrafficCapture::SystemProxy;
        Ok(())
    }

    /// Enable the system-proxy backend while the core is Running (Home start
    /// path). Exclusivity is enforced here, never inferred from `tun.enabled`
    /// or memory alone: TUN must be fully released and no transition may be
    /// in flight; `RecoveryRequired` is fail-closed for both backends.
    pub fn enable_system_proxy(
        &self,
        settings: &AppSettings,
        core: &dyn CoreHandle,
        proxy: &dyn SystemProxy,
    ) -> Result<(), AppError> {
        {
            let inner = self
                .inner
                .lock()
                .map_err(|_| lock_poisoned("capture state"))?;
            match inner.active {
                TrafficCapture::SystemProxy => return Ok(()),
                TrafficCapture::Tun => {
                    return Err(AppError::with_code(
                        "proxy.apply_failed",
                        "TUN capture is still active; stop TUN before enabling the system proxy",
                    ));
                }
                TrafficCapture::Inactive => {}
            }
            match inner.tun_status {
                TunStatus::Preparing | TunStatus::Stopping | TunStatus::RecoveryRequired => {
                    return Err(AppError::with_code(
                        "proxy.apply_failed",
                        format!(
                            "system proxy unavailable (TUN status {:?})",
                            inner.tun_status
                        ),
                    ));
                }
                _ => {}
            }
        }
        self.require_clean_journal_for_proxy()?;
        orchestrate_enable_system_proxy(&self.paths, settings, core, proxy)?;
        self.set_system_proxy_active()
    }

    /// System proxy is also a capture backend. It must not be enabled while a
    /// TUN journal is unreadable or non-terminal: the OS may still contain
    /// resources that the controller cannot account for.
    fn require_clean_journal_for_proxy(&self) -> Result<(), AppError> {
        let journal = match TunJournal::load(&self.paths.tun_state()) {
            Ok(journal) => journal,
            Err(err) => {
                let app_err = AppError::with_code(
                    "tun.recovery_required",
                    format!(
                        "TUN journal cannot be read; recovery is required ({})",
                        err.message
                    ),
                );
                let _ = self.fail_transition(TunStatus::RecoveryRequired, &app_err);
                return Err(app_err);
            }
        };
        if journal.is_some_and(|journal| journal.state != JournalState::Clean) {
            let app_err = AppError::with_code(
                "tun.recovery_required",
                "TUN journal is not clean; run recovery before enabling system proxy",
            );
            let _ = self.fail_transition(TunStatus::RecoveryRequired, &app_err);
            return Err(app_err);
        }
        Ok(())
    }

    fn begin_transition(&self, status: TunStatus, transition_id: String) -> Result<(), AppError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| lock_poisoned("capture state"))?;
        inner.tun_status = status;
        inner.transition_id = Some(transition_id);
        inner.tun_error = None;
        Ok(())
    }

    fn finish_transition(
        &self,
        active: TrafficCapture,
        status: TunStatus,
        tun_interface: Option<String>,
    ) -> Result<(), AppError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| lock_poisoned("capture state"))?;
        inner.active = active;
        inner.tun_status = status;
        inner.tun_error = None;
        inner.transition_id = None;
        inner.tun_interface = tun_interface;
        // A successful TUN transition outside the dev `sudo` runner means the
        // elevated core was spawned by the helper and wrote to the helper's
        // fixed core log; latch it so the log view keeps merging that file
        // after TUN stops (the app-data core log is frozen during the helper
        // session).
        if active == TrafficCapture::Tun && !ice_tun_sys::dev_sudo_runner_enabled() {
            inner.helper_core_used = true;
        }
        Ok(())
    }

    /// Fail a transition: no backend claimed, status + error recorded.
    fn fail_transition(&self, status: TunStatus, err: &AppError) -> Result<(), AppError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| lock_poisoned("capture state"))?;
        inner.active = TrafficCapture::Inactive;
        inner.tun_status = status;
        inner.tun_error = Some(err.clone());
        inner.transition_id = None;
        inner.tun_interface = None;
        Ok(())
    }

    /// Enable TUN capture (plan §4.3). Preconditions (checked here): platform
    /// gate green, no active system proxy, no in-flight transition, not in
    /// `RecoveryRequired`. The core must already be Running on the Diagnostic
    /// config (the Home start path ensures it); this stops the app-managed
    /// core and the backend's coordinator starts the elevated one, which the
    /// shell then adopts. Returns the resolved interface name (the caller
    /// commits it to `settings.json` only after the transition is healthy).
    pub fn enable_tun(
        &self,
        settings: &AppSettings,
        core: &mut dyn CoreHandle,
        binary: PathBuf,
    ) -> Result<Option<String>, AppError> {
        let mut backend = self
            .backend
            .lock()
            .map_err(|_| lock_poisoned("capture backend"))?;
        let capability = backend.capability();
        if !capability.supported {
            return Err(AppError::with_code(
                "tun.not_supported",
                capability
                    .reason
                    .unwrap_or_else(|| "TUN unavailable on this platform".to_string())
                    .to_string(),
            ));
        }
        {
            let inner = self
                .inner
                .lock()
                .map_err(|_| lock_poisoned("capture state"))?;
            if inner.tun_status == TunStatus::RecoveryRequired {
                return Err(AppError::with_code(
                    "tun.recovery_required",
                    "TUN cleanup is unverified; run recovery before enabling capture",
                ));
            }
            match inner.active {
                TrafficCapture::SystemProxy => {
                    return Err(AppError::with_code(
                        "tun.apply_failed",
                        "system proxy is active; disable it before enabling TUN",
                    ));
                }
                TrafficCapture::Tun if inner.tun_status == TunStatus::Enabled => {
                    // Already enabled: the resolved name is unchanged.
                    return Ok(None);
                }
                TrafficCapture::Tun => {
                    return Err(AppError::with_code(
                        "tun.apply_failed",
                        "TUN capture transition already in flight",
                    ));
                }
                TrafficCapture::Inactive => {}
            }
        }
        // Fail-closed journal guard: every non-clean journal is an outstanding
        // transition, even when granular ownership fields are empty. A
        // post-mutation journal write may have failed before the first record
        // became durable, so field-based inference could overwrite the only
        // evidence of an owned adapter or route.
        match self.journal_has_outstanding_records() {
            Ok(true) => {
                let app_err = AppError::with_code(
                    "tun.recovery_required",
                    "TUN journal is not clean; run recovery before enabling capture",
                );
                let _ = self.fail_transition(TunStatus::RecoveryRequired, &app_err);
                return Err(app_err);
            }
            Ok(false) => {}
            Err(err) => {
                let app_err = AppError::with_code(
                    "tun.recovery_required",
                    format!(
                        "TUN journal cannot be read; recovery is required ({})",
                        err.message
                    ),
                );
                let _ = self.fail_transition(TunStatus::RecoveryRequired, &app_err);
                return Err(app_err);
            }
        }

        // One installed core version at a time: when the app ships a new
        // sing-box, the helper's root-owned copy must be refreshed by the
        // elevated installer before TUN may use it (the old copy must never
        // linger as an active core).
        if crate::helper_install::helper_core_stale(self.resource_dir.as_deref())
            && !ice_tun_sys::dev_sudo_runner_enabled()
        {
            let app_err = AppError::with_code(
                "tun.helper_stale",
                "the helper still runs the old core: update the helper in Settings first",
            );
            let _ = self.fail_transition(TunStatus::Error, &app_err);
            return Err(app_err);
        }

        let transition_id = Uuid::new_v4().to_string();
        self.begin_transition(TunStatus::Preparing, transition_id)?;

        let result = self.enable_tun_inner(settings, core, binary.clone(), &mut **backend);
        match result {
            Ok(interface) => {
                self.finish_transition(TrafficCapture::Tun, TunStatus::Enabled, interface.clone())?;
                Ok(interface)
            }
            Err(err) => {
                // Fail closed: no backend claimed. If the app-managed core was
                // released, bring it back on the Diagnostic config (best
                // effort) so the previous service state is restored.
                if core.state().status == CoreStatus::Stopped {
                    let _ = generate_config(
                        &self.paths,
                        settings,
                        self.resource_dir.as_deref(),
                        CaptureIntent::Diagnostic,
                    );
                    let core_paths = build_core_paths(&self.paths, settings, binary);
                    let _ = core.start(&core_paths);
                }
                // A journal that still claims (or may claim) owned OS resources: state is
                // not clean and at least one ownership record exists. An error-state
                // journal with no records means nothing was ever mutated (the
                // failure happened before any OS mutation) and is safe to retry.
                // A journal that cannot even be read is fail-closed: recovery
                // is required before any new transition.
                let outstanding = self.journal_has_outstanding_records().unwrap_or(true);
                let status = if err.code == "tun.permission_required" {
                    TunStatus::PermissionRequired
                } else if outstanding {
                    TunStatus::RecoveryRequired
                } else {
                    TunStatus::Error
                };
                let _ = self.fail_transition(status, &err);
                Err(err)
            }
        }
    }

    /// The actual transition; `self` holds the backend lock. Returns the
    /// applied interface name on success. Leaves the journal coherent on
    /// every failure (clean when nothing was mutated, error otherwise — the
    /// startup recovery driver converges the rest).
    fn enable_tun_inner(
        &self,
        settings: &AppSettings,
        core: &mut dyn CoreHandle,
        binary: PathBuf,
        backend: &mut dyn TunBackend,
    ) -> Result<Option<String>, AppError> {
        let journal = TunJournal::new(
            self.inner
                .lock()
                .map_err(|_| lock_poisoned("capture state"))?
                .transition_id
                .clone()
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            self.owner.clone(),
        );
        journal.save(&self.paths.tun_state()).map_err(map_tun)?;

        // prepare — side-effect free; resolves the interface name.
        let prepared = match backend.prepare(&tun_config_from_settings(settings)) {
            Ok(prepared) => prepared,
            Err(err) => {
                self.journal_clean("prepare rejected the tun config")?;
                return Err(map_tun(err));
            }
        };

        if core.state().status != CoreStatus::Running {
            self.journal_clean("core not running; nothing was mutated")?;
            return Err(AppError::new(
                ErrorCode::CoreInvalidState,
                "core is not running; cannot enable TUN capture",
            ));
        }

        // Build the Tun config with the resolved interface name.
        let resolved_name = prepared.config.interface_name.clone();
        let mut tun_settings = settings.clone();
        if let Some(name) = &resolved_name {
            tun_settings.tun.interface_name = Some(name.clone());
        }
        if let Err(err) = generate_config(
            &self.paths,
            &tun_settings,
            self.resource_dir.as_deref(),
            CaptureIntent::Tun,
        ) {
            self.journal_clean("tun config generation failed; nothing was mutated")?;
            return Err(err);
        }

        // Release the app-managed (non-elevated) core; the elevated core
        // takes over. On any later failure the app core is restarted on the
        // Diagnostic config by the caller's error path.
        if let Err(err) = core.stop(&self.paths.pid()) {
            self.journal_clean("core stop failed; nothing was mutated")?;
            return Err(AppError::from(err));
        }

        // The backend starts the elevated core with config.json (coordinator)
        // and journals the observed ownership.
        let applied = match backend.apply(&prepared) {
            Ok(applied) => applied,
            Err(err) => {
                if err.code == TunErrorCode::PermissionRequired {
                    // Refused before any OS mutation (helper not authorized):
                    // nothing was owned, the journal can be closed clean.
                    self.journal_clean("permission required before any mutation")?;
                } else {
                    self.journal_error("apply failed; startup recovery verifies cleanup")?;
                }
                return Err(map_tun(err));
            }
        };

        // Adopt the elevated core (native path) or restart the app-managed
        // core on the Tun config (mock/fallback backends).
        let core_paths = build_core_paths(&self.paths, &tun_settings, binary.clone());
        if let Some(pid) = applied.core_pid {
            if let Err(err) = core
                .adopt_external(pid, &core_paths)
                .map_err(AppError::from)
            {
                // The elevated core is already running and owns the TUN
                // resources; adoption failed. Release it fail-closed: stop
                // the core via the coordinator and verify cleanup before
                // returning. The journal is closed clean only when cleanup
                // is verified; otherwise recovery is required.
                return self
                    .fail_after_adopt_failure(&err, &applied, core, settings, binary, backend);
            }
        } else {
            core.start(&core_paths).map_err(AppError::from)?;
        }

        // Readiness: backend health (Clash API liveness was probed by the
        // adopt/start above). On disagreement, release fail-closed.
        let health = backend.verify(&applied).map_err(map_tun)?;
        if !health.all_ok() {
            let _ = core.stop(&self.paths.pid());
            let _ = backend.restore(&applied);
            self.journal_error("tun readiness checks disagreed")?;
            return Err(AppError::with_code(
                "tun.healthcheck_failed",
                "TUN readiness check failed; capture released",
            ));
        }

        // Journal applied + verified.
        let mut journal = TunJournal::load(&self.paths.tun_state())
            .map_err(map_tun)?
            .unwrap_or(journal)
            .clone();
        journal
            .record(
                &self.paths.tun_state(),
                JournalState::Applied,
                steps::VERIFY_APPLIED,
                |_| {},
            )
            .map_err(map_tun)?;

        // The resolved interface name is NOT persisted here: the caller
        // commits `settings.json` only after the transition is healthy
        // (plan §4.3 commit-after-health), so a later commit failure cannot
        // leave a half-committed settings file.
        Ok(resolved_name)
    }

    /// Adoption of the elevated core failed after `backend.apply` started it
    /// and journaled its ownership. Release fail-closed: stop the core via
    /// the coordinator and verify cleanup. A verified release restores the
    /// app-managed core on the Diagnostic config and keeps the failure
    /// retryable (`Error`); an unverified release enters `RecoveryRequired`.
    fn fail_after_adopt_failure(
        &self,
        err: &AppError,
        applied: &AppliedTun,
        core: &mut dyn CoreHandle,
        settings: &AppSettings,
        binary: PathBuf,
        backend: &mut dyn TunBackend,
    ) -> Result<Option<String>, AppError> {
        match backend.restore(applied) {
            Ok(()) => {
                let _ = self.journal_clean("adopt failed; elevated core released and verified");
                let _ = generate_config(
                    &self.paths,
                    settings,
                    self.resource_dir.as_deref(),
                    CaptureIntent::Diagnostic,
                );
                let core_paths = build_core_paths(&self.paths, settings, binary);
                let _ = core.start(&core_paths);
                Err(err.clone())
            }
            Err(release_err) => {
                self.journal_error("adopt failed and release unverified")
                    .ok();
                let release_app_err = map_tun(release_err);
                let _ = self.fail_transition(
                    TunStatus::RecoveryRequired,
                    &AppError::with_code(
                        "tun.recovery_required",
                        format!(
                            "failed to take over the elevated core ({err}) and cleanup is unconfirmed ({})",
                            release_app_err.message
                        ),
                    ),
                );
                Err(AppError::with_code(
                    "tun.recovery_required",
                    format!(
                        "failed to take over the elevated core and TUN cleanup is unconfirmed ({}); fail-closed",
                        release_app_err.message
                    ),
                ))
            }
        }
    }

    fn journal_clean(&self, context: &str) -> Result<(), AppError> {
        let mut journal = TunJournal::load(&self.paths.tun_state())
            .map_err(map_tun)?
            .ok_or_else(|| AppError::new(ErrorCode::ConfigInvalid, context))?;
        journal
            .record(
                &self.paths.tun_state(),
                JournalState::Clean,
                steps::VERIFY_CLEAN,
                |j| {
                    j.interface_name = None;
                    j.interface_id = None;
                    j.addresses.clear();
                    j.routes.clear();
                    j.expected_addresses.clear();
                    j.expected_routes.clear();
                    j.dns_before = None;
                    j.dns_after = None;
                },
            )
            .map_err(map_tun)?;
        Ok(())
    }

    fn journal_error(&self, context: &str) -> Result<(), AppError> {
        let mut journal = TunJournal::load(&self.paths.tun_state())
            .map_err(map_tun)?
            .ok_or_else(|| AppError::new(ErrorCode::ConfigInvalid, context))?;
        journal
            .record(
                &self.paths.tun_state(),
                JournalState::Error,
                steps::VERIFY_APPLIED,
                |_| {},
            )
            .map_err(map_tun)?;
        Ok(())
    }

    /// Whether the on-disk journal can be replaced by a new transition. Every
    /// non-clean state is treated as outstanding, because a journal write can
    /// fail immediately after an OS mutation and before ownership fields are
    /// durable. An unreadable journal is an error, never a "no records"
    /// answer.
    fn journal_has_outstanding_records(&self) -> Result<bool, AppError> {
        let journal = TunJournal::load(&self.paths.tun_state()).map_err(map_tun)?;
        Ok(journal.is_some_and(|journal| journal.state != JournalState::Clean))
    }

    /// Disable whichever backend is active (plan §4.3): restores the OS proxy
    /// for the system-proxy backend, or releases TUN capture and — when
    /// `restart_diagnostic` — brings the app-managed core back on the
    /// Mixed-only config. Idempotent when nothing is active.
    pub fn disable_active_backend(
        &self,
        settings: &AppSettings,
        core: &mut dyn CoreHandle,
        proxy: &dyn SystemProxy,
        binary: PathBuf,
        restart_diagnostic: bool,
    ) -> Result<(), AppError> {
        let mut backend = self
            .backend
            .lock()
            .map_err(|_| lock_poisoned("capture backend"))?;
        match self.active_backend() {
            TrafficCapture::Inactive => Ok(()),
            TrafficCapture::SystemProxy => {
                orchestrate_disable_system_proxy(&self.paths, proxy)?;
                self.finish_transition(TrafficCapture::Inactive, TunStatus::Disabled, None)
            }
            TrafficCapture::Tun => {
                self.disable_tun(settings, core, binary, restart_diagnostic, &mut **backend)
            }
        }
    }

    fn disable_tun(
        &self,
        settings: &AppSettings,
        core: &mut dyn CoreHandle,
        binary: PathBuf,
        restart_diagnostic: bool,
        backend: &mut dyn TunBackend,
    ) -> Result<(), AppError> {
        self.begin_transition(TunStatus::Stopping, Uuid::new_v4().to_string())?;
        let mut journal = TunJournal::load(&self.paths.tun_state())
            .map_err(map_tun)?
            .ok_or_else(|| {
                AppError::with_code(
                    "tun.restore_failed",
                    "tun journal missing while TUN capture is active",
                )
            })?;
        let applied = AppliedTun::from_journal(&journal);
        journal
            .record(
                &self.paths.tun_state(),
                JournalState::Restoring,
                steps::RESTORE_STARTED,
                |_| {},
            )
            .map_err(map_tun)?;

        // Release capture: the backend restore first — on the native path it
        // stops the elevated core through the coordinator (the only component
        // that may signal a root-owned process) and verifies teardown — then
        // stop the app-managed core. Both must succeed before the journal is
        // marked clean; an unverified release is fail-closed.
        let restore_result = backend.restore(&applied);
        let stop_result = core.stop(&self.paths.pid());
        match (restore_result, stop_result) {
            (Ok(()), Ok(())) => {}
            (Err(err), _) => {
                let mut journal = TunJournal::load(&self.paths.tun_state())
                    .map_err(map_tun)?
                    .unwrap_or(journal);
                journal
                    .record(
                        &self.paths.tun_state(),
                        JournalState::RecoveryRequired,
                        steps::RESTORE_STARTED,
                        |_| {},
                    )
                    .ok();
                let _ = core.stop(&self.paths.pid());
                let app_err = map_tun(err);
                let _ = self.fail_transition(TunStatus::RecoveryRequired, &app_err);
                return Err(AppError::with_code(
                    "tun.recovery_required",
                    format!("TUN cleanup unconfirmed; fail-closed ({})", app_err.message),
                ));
            }
            (Ok(()), Err(stop_err)) => {
                let mut journal = TunJournal::load(&self.paths.tun_state())
                    .map_err(map_tun)?
                    .unwrap_or(journal);
                journal
                    .record(
                        &self.paths.tun_state(),
                        JournalState::RecoveryRequired,
                        steps::RESTORE_STARTED,
                        |_| {},
                    )
                    .ok();
                let app_err = AppError::from(stop_err);
                let _ = self.fail_transition(TunStatus::RecoveryRequired, &app_err);
                return Err(AppError::with_code(
                    "tun.recovery_required",
                    format!(
                        "TUN resources released but core stop unconfirmed; fail-closed ({})",
                        app_err.message
                    ),
                ));
            }
        }

        // Journal clean; capture is disabled. `settings.json` is untouched.
        let mut journal = TunJournal::load(&self.paths.tun_state())
            .map_err(map_tun)?
            .unwrap_or(journal)
            .clone();
        journal
            .record(
                &self.paths.tun_state(),
                JournalState::Clean,
                steps::VERIFY_CLEAN,
                |j| {
                    j.interface_name = None;
                    j.interface_id = None;
                    j.addresses.clear();
                    j.routes.clear();
                    j.expected_addresses.clear();
                    j.expected_routes.clear();
                    j.dns_before = None;
                    j.dns_after = None;
                },
            )
            .map_err(map_tun)?;
        self.finish_transition(TrafficCapture::Inactive, TunStatus::Disabled, None)?;

        if restart_diagnostic {
            generate_config(
                &self.paths,
                settings,
                self.resource_dir.as_deref(),
                CaptureIntent::Diagnostic,
            )?;
            let core_paths = build_core_paths(&self.paths, settings, binary);
            core.start(&core_paths).map_err(AppError::from)?;
        }
        Ok(())
    }

    /// Watchdog path: sing-box exited unexpectedly while TUN was claimed.
    /// Runs the idempotent release + verification and writes the Diagnostic
    /// config so a later automatic core start cannot recreate TUN. Returns a
    /// UI warning when cleanup cannot be confirmed (fail-closed).
    pub fn handle_unexpected_core_exit(
        &self,
        core: &mut dyn CoreHandle,
        settings: &AppSettings,
    ) -> Option<String> {
        if self.active_backend() != TrafficCapture::Tun {
            return None;
        }
        let mut backend = match self.backend.lock() {
            Ok(backend) => backend,
            Err(_) => return Some("capture controller unavailable; TUN state not restored".into()),
        };
        let _ = self.begin_transition(TunStatus::Stopping, Uuid::new_v4().to_string());
        let mut journal = match TunJournal::load(&self.paths.tun_state()) {
            Ok(Some(journal)) => journal,
            _ => {
                // TUN was claimed but the journal is missing or unreadable:
                // ownership cannot be verified, so fail closed to
                // RecoveryRequired — a plain Error would allow a new
                // activation to overwrite unknown state.
                let _ = self.fail_transition(
                    TunStatus::RecoveryRequired,
                    &AppError::with_code(
                        "tun.recovery_required",
                        "journal missing while TUN capture was active",
                    ),
                );
                return Some(
                    "TUN journal missing after sing-box exited unexpectedly; fail-closed".into(),
                );
            }
        };
        let applied = AppliedTun::from_journal(&journal);
        journal
            .record(
                &self.paths.tun_state(),
                JournalState::Restoring,
                steps::RESTORE_STARTED,
                |_| {},
            )
            .ok();
        match backend.restore(&applied) {
            Ok(()) => {
                let mut journal = TunJournal::load(&self.paths.tun_state())
                    .ok()
                    .flatten()
                    .unwrap_or(journal);
                journal
                    .record(
                        &self.paths.tun_state(),
                        JournalState::Clean,
                        steps::VERIFY_CLEAN,
                        |j| {
                            j.interface_name = None;
                            j.interface_id = None;
                            j.addresses.clear();
                            j.routes.clear();
                            j.expected_addresses.clear();
                            j.expected_routes.clear();
                            j.dns_before = None;
                            j.dns_after = None;
                        },
                    )
                    .ok();
                // The Diagnostic config prevents auto-start from recreating TUN.
                let _ = generate_config(
                    &self.paths,
                    settings,
                    self.resource_dir.as_deref(),
                    CaptureIntent::Diagnostic,
                );
                let _ = self.finish_transition(TrafficCapture::Inactive, TunStatus::Disabled, None);
                tracing::info!("tun capture released after unexpected sing-box exit");
                None
            }
            Err(err) => {
                tracing::error!(error = %err, "tun cleanup after unexpected core exit failed");
                let mut journal = TunJournal::load(&self.paths.tun_state())
                    .ok()
                    .flatten()
                    .unwrap_or(journal);
                journal
                    .record(
                        &self.paths.tun_state(),
                        JournalState::RecoveryRequired,
                        steps::RESTORE_STARTED,
                        |_| {},
                    )
                    .ok();
                let _ = core.stop(&self.paths.pid());
                let _ = self.fail_transition(
                    TunStatus::RecoveryRequired,
                    &AppError::with_code(
                        "tun.recovery_required",
                        format!("TUN cleanup unconfirmed ({err})"),
                    ),
                );
                Some(format!(
                    "TUN cleanup unconfirmed after sing-box exited unexpectedly: {err}"
                ))
            }
        }
    }

    /// Recovery (inside the orchestration lock, plan §4.4): discard an
    /// interrupted settings transaction, then run the journal recovery
    /// driver. Never enables capture. Returns a UI warning when anything
    /// needs attention. Used by startup (after orphan-core reclamation)
    /// and by the on-demand「重试恢复」action from the UI.
    pub fn recover(&self, core: &mut dyn CoreHandle) -> Result<Option<String>, AppError> {
        let mut warnings: Vec<String> = Vec::new();

        if self.paths.pending_settings().is_file() {
            tracing::warn!(
                "interrupted settings transaction; committed settings are the old state"
            );
            let _ = fs::remove_file(self.paths.pending_settings());
            warnings.push(
                "an incomplete backend switch was detected; restored the previous settings".into(),
            );
        }

        let mut backend = self
            .backend
            .lock()
            .map_err(|_| lock_poisoned("capture backend"))?;
        let outcome = {
            let journal_path = &self.paths.tun_state();
            let mut driver = RecoveryDriver::new(journal_path, &mut **backend, &self.owner);
            match driver.recover() {
                Ok(outcome) => outcome,
                Err(err) => {
                    let app_err = map_tun(err);
                    let recovery_err = AppError::with_code(
                        "tun.recovery_required",
                        format!("TUN recovery could not be verified ({})", app_err.message),
                    );
                    let _ = self.fail_transition(TunStatus::RecoveryRequired, &recovery_err);
                    return Err(recovery_err);
                }
            }
        };
        match outcome {
            RecoveryOutcome::NothingToDo
            | RecoveryOutcome::ForeignJournal
            | RecoveryOutcome::Cleaned => {
                // Recovery never enables capture; after a verified clean (or
                // with nothing outstanding) the controller starts from the
                // disabled baseline. When a journal was cleaned, rewrite the
                // Diagnostic config so a later automatic core start can never
                // recreate TUN from a stale runtime file.
                if outcome == RecoveryOutcome::Cleaned {
                    let settings =
                        crate::orchestrate::current_settings(&self.paths).unwrap_or_default();
                    let _ = generate_config(
                        &self.paths,
                        &settings,
                        self.resource_dir.as_deref(),
                        CaptureIntent::Diagnostic,
                    );
                }
                // The OS system proxy may still be applied (startup recovery
                // runs after the proxy backup was restored into the live OS,
                // or recovery is retried mid-session). The controller must
                // not report Inactive while the system proxy is still owned —
                // a later TUN enable would double-capture. The disk record is
                // authoritative: the memory flag alone could have been lost.
                if self.active_backend() == TrafficCapture::SystemProxy
                    || is_proxy_applied_on_disk(&self.paths.proxy_backup())
                {
                    tracing::info!(
                        "recovery finished while the system proxy is still applied; keeping it as the active capture backend"
                    );
                } else {
                    let _ =
                        self.finish_transition(TrafficCapture::Inactive, TunStatus::Disabled, None);
                }
            }
            RecoveryOutcome::RecoveryRequired => {
                let _ = self.fail_transition(
                    TunStatus::RecoveryRequired,
                    &AppError::with_code(
                        "tun.recovery_required",
                        "TUN cleanup unconfirmed; new TUN activation is blocked",
                    ),
                );
                warnings.push("TUN cleanup unconfirmed; fail-closed. Retry recovery".into());
            }
        }
        let _ = core;
        Ok((!warnings.is_empty()).then(|| warnings.join("；")))
    }

    /// Serialized capture-backend transition when `tun.enabled` (or the TUN
    /// topology) changed while the service is active (plan §4.3). Writes the
    /// pending record first; commits `settings.json` only after the requested
    /// backend is healthy; on failure rolls back to the old backend; clears
    /// the pending record in both cases. An uncertain rollback leaves both
    /// backends disabled and enters `RecoveryRequired`. When the active
    /// backend and the committed settings already agree with the candidate,
    /// no transition runs: the candidate is persisted as-is.
    pub fn transition_tun_settings(
        &self,
        previous: &AppSettings,
        candidate: &AppSettings,
        core: &mut dyn CoreHandle,
        proxy: &dyn SystemProxy,
        binary: PathBuf,
    ) -> Result<(), AppError> {
        // Pre-reconcile the candidate against the active profile so
        // `generate_config` never persists settings mid-transition.
        let candidate = self.reconciled_candidate(candidate);
        let pending = PendingSettingsRecord {
            candidate: candidate.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        write_json_atomic(&self.paths.pending_settings(), &pending).map_err(AppError::from)?;

        let active = self.active_backend();
        // Decide whether a backend transition is actually required before
        // doing anything. When the active backend and the committed settings
        // already agree with the candidate (e.g. a rollback left the active
        // backend matching the new desire), fall through to a plain persist —
        // the mismatch is precisely the condition the pending record exists
        // to resolve, and erroring would surface a bogus save failure.
        let transition_needed = match (active, previous.tun.enabled, candidate.tun.enabled) {
            (TrafficCapture::SystemProxy, false, true) | (TrafficCapture::Tun, true, false) => true,
            (TrafficCapture::Tun, true, true) => {
                tun_topology_changed(&previous.tun, &candidate.tun)
            }
            _ => false,
        };
        if !transition_needed {
            save_settings(&self.paths.settings(), &candidate)?;
            let _ = fs::remove_file(self.paths.pending_settings());
            return Ok(());
        }

        // Run the transition inside a closure so every intermediate `?`
        // feeds the rollback + pending cleanup below instead of returning
        // early and leaving a stale pending record or an uncommitted switch.
        // The closure returns the resolved TUN interface name (when TUN ended
        // up enabled) so the final commit can fold it into `settings.json`.
        let transition: Result<Option<String>, AppError> = (|| {
            match (active, previous.tun.enabled, candidate.tun.enabled) {
                // system proxy -> tun
                (TrafficCapture::SystemProxy, false, true) => {
                    orchestrate_disable_system_proxy(&self.paths, proxy)?;
                    let _ =
                        self.finish_transition(TrafficCapture::Inactive, TunStatus::Disabled, None);
                    self.enable_tun(&candidate, core, binary.clone())
                }
                // tun -> system proxy
                (TrafficCapture::Tun, true, false) => {
                    self.disable_active_backend(previous, core, proxy, binary.clone(), true)?;
                    Ok(None)
                }
                // TUN topology change while TUN stays enabled: explicit
                // stop/reconfigure/start (plan §4.3, no in-place mutation).
                (TrafficCapture::Tun, true, true) => {
                    self.disable_active_backend(previous, core, proxy, binary.clone(), true)?;
                    self.enable_tun(&candidate, core, binary.clone())
                }
                _ => unreachable!("transition_needed was checked above"),
            }
        })();

        match transition {
            Ok(resolved_name) => {
                // The requested backend is healthy and active. Commit the
                // candidate — folding in the resolved TUN interface name —
                // and only then clear the pending record.
                let mut committed = candidate.clone();
                if let Some(name) = &resolved_name {
                    committed.tun.interface_name = Some(name.clone());
                }
                match save_settings(&self.paths.settings(), &committed) {
                    Ok(()) => {
                        let _ = fs::remove_file(self.paths.pending_settings());
                        Ok(())
                    }
                    Err(commit_err) => {
                        // settings.json could not be committed: roll the
                        // backend back to the previous state and KEEP the
                        // pending record so startup recovery does not treat
                        // the committed (old) settings as the running truth.
                        let rollback = self
                            .rollback_after_commit_failure(previous, active, core, proxy, binary);
                        match rollback {
                            Ok(()) => Err(commit_err),
                            Err(rollback_err) => {
                                let _ = self.fail_transition(
                                    TunStatus::RecoveryRequired,
                                    &AppError::with_code(
                                        "tun.recovery_required",
                                        format!(
                                            "settings commit failed ({commit_err}) and backend rollback is unconfirmed ({rollback_err})"
                                        ),
                                    ),
                                );
                                Err(AppError::with_code(
                                    "tun.recovery_required",
                                    "settings commit failed and backend rollback is unconfirmed; both backends are disabled. Retry recovery",
                                ))
                            }
                        }
                    }
                }
            }
            Err(err) => {
                let rollback = self.rollback_backend(previous, active, core, proxy, binary);
                let _ = fs::remove_file(self.paths.pending_settings());
                match rollback {
                    Ok(()) => Err(err),
                    Err(rollback_err) => {
                        let _ = self.fail_transition(
                            TunStatus::RecoveryRequired,
                            &AppError::with_code(
                                "tun.recovery_required",
                                format!("switch failed ({err}) and rollback is unconfirmed ({rollback_err})"),
                            ),
                        );
                        Err(AppError::with_code(
                            "tun.recovery_required",
                            "backend switch failed and rollback is unconfirmed; both backends are disabled. Retry recovery",
                        ))
                    }
                }
            }
        }
    }

    /// Roll back a transition whose *commit* failed: the new backend is
    /// healthy and active, so it is disabled first (release TUN / restore
    /// the OS proxy), then the backend that was active before the transition
    /// is restored.
    fn rollback_after_commit_failure(
        &self,
        previous: &AppSettings,
        previous_active: TrafficCapture,
        core: &mut dyn CoreHandle,
        proxy: &dyn SystemProxy,
        binary: PathBuf,
    ) -> Result<(), AppError> {
        match self.active_backend() {
            TrafficCapture::Tun => {
                self.disable_active_backend(previous, core, proxy, binary.clone(), true)?;
            }
            TrafficCapture::SystemProxy => {
                orchestrate_disable_system_proxy(&self.paths, proxy)?;
                let _ = self.finish_transition(TrafficCapture::Inactive, TunStatus::Disabled, None);
            }
            TrafficCapture::Inactive => {}
        }
        if previous.tun.enabled {
            self.enable_tun(previous, core, binary).map(|_| ())
        } else if previous_active == TrafficCapture::SystemProxy {
            self.enable_system_proxy(previous, core, proxy)
        } else {
            Ok(())
        }
    }

    /// Reconcile a transition candidate's selected tag against the active
    /// profile, without writing disk (plan §4.3 commit-after-health).
    fn reconciled_candidate(&self, settings: &AppSettings) -> AppSettings {
        use ice_subscription::{load_active_profile, load_index, SubscriptionPaths};
        let sub_paths = SubscriptionPaths::from_app(&self.paths);
        match load_index(&sub_paths)
            .ok()
            .and_then(|index| load_active_profile(&sub_paths, &index).ok())
        {
            Some(profile) => crate::orchestrate::reconcile_selected_tag(settings, &profile),
            None => settings.clone(),
        }
    }

    /// Policy-only apply while TUN capture is active (plan §4.3). The
    /// elevated core cannot be signalled by the app (SIGHUP from a non-root
    /// process fails with EPERM), so every config change runs the
    /// stop/reconfigure/start sequence through the backend: the controller
    /// moves through `stopping`/`preparing`, regenerates the Tun config from
    /// the new settings, restarts the core, and re-verifies TUN health
    /// before returning to `enabled`. A restart that removes resources is
    /// treated as a disable/re-apply, never as a successful transparent
    /// reload. On failure the previous TUN settings are re-applied; an
    /// unconfirmed rollback fail-closes to `RecoveryRequired`.
    pub fn apply_while_tun_active(
        &self,
        settings: &AppSettings,
        previous: &AppSettings,
        core: &mut dyn CoreHandle,
        proxy: &dyn SystemProxy,
        binary: PathBuf,
    ) -> Result<(), AppError> {
        if self.active_backend() != TrafficCapture::Tun {
            return Err(AppError::new(
                ErrorCode::CoreInvalidState,
                "apply requested while TUN capture is not active",
            ));
        }
        self.disable_active_backend(previous, core, proxy, binary.clone(), true)?;
        match self.enable_tun(settings, core, binary.clone()) {
            Ok(_resolved_name) => Ok(()),
            Err(err) => {
                // The failed candidate may have left a non-clean journal even
                // when no ownership fields became durable. Reconcile that
                // journal before attempting the old configuration; otherwise
                // the fail-closed activation guard would block the rollback.
                let rollback = match self.recover(core) {
                    Ok(_) if self.tun_status() != TunStatus::RecoveryRequired => {
                        self.enable_tun(previous, core, binary)
                    }
                    Ok(_) => Err(AppError::with_code(
                        "tun.recovery_required",
                        "TUN recovery remains unverified after apply failure",
                    )),
                    Err(recovery_err) => Err(recovery_err),
                };
                match rollback {
                    Ok(_) => Err(err),
                    Err(rollback_err) => {
                        let _ = self.fail_transition(
                            TunStatus::RecoveryRequired,
                            &AppError::with_code(
                                "tun.recovery_required",
                                format!("apply failed ({err}) and TUN rollback is unconfirmed ({rollback_err})"),
                            ),
                        );
                        Err(AppError::with_code(
                            "tun.recovery_required",
                            "TUN config apply failed and rollback is unconfirmed; fail-closed. Retry recovery",
                        ))
                    }
                }
            }
        }
    }

    /// Best-effort restore of the backend that was active before a failed
    /// transition. Uncertain outcomes surface as errors (the caller then
    /// enters `RecoveryRequired`).
    fn rollback_backend(
        &self,
        previous: &AppSettings,
        previous_active: TrafficCapture,
        core: &mut dyn CoreHandle,
        proxy: &dyn SystemProxy,
        binary: PathBuf,
    ) -> Result<(), AppError> {
        // A failed transition can leave a non-clean journal even when the
        // backend reports an error before ownership fields were persisted.
        // Recovery is the only safe way to establish a clean baseline before
        // restoring the previous backend.
        if self.journal_has_outstanding_records().unwrap_or(true) {
            self.recover(core)?;
            if self.tun_status() == TunStatus::RecoveryRequired {
                return Err(AppError::with_code(
                    "tun.recovery_required",
                    "TUN recovery remains unverified; previous backend is not restored",
                ));
            }
        }
        if previous.tun.enabled {
            self.enable_tun(previous, core, binary).map(|_| ())
        } else if previous_active == TrafficCapture::SystemProxy {
            self.enable_system_proxy(previous, core, proxy)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(err.code, "tun.apply_failed");
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
        assert_eq!(err.code, "tun.not_supported");
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
        assert_eq!(err.code, "tun.recovery_required");
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
        assert_eq!(err.code, "tun.apply_failed");
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
        assert_eq!(err.code, "tun.recovery_required");

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
        assert_eq!(err.code, "tun.apply_failed");
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
        assert_eq!(err.code, "tun.apply_failed");
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
        assert_eq!(err.code, "tun.apply_failed");
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
        assert_eq!(err.code, "tun.recovery_required");
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
        assert_eq!(err.code, "tun.recovery_required");
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
}
