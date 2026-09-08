// SPDX-License-Identifier: GPL-3.0-or-later

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
    save_settings_for, write_json_atomic, AppError, AppPaths, AppSettings, CaptureIntent,
    ErrorCode, TunSettings,
};
use ice_core::{CoreHandle, CoreStatus};
use ice_engine::host_platform;
use ice_proxy_sys::{proxy_backup_indicates_ownership, SystemProxy};
use ice_tun_sys::{
    create_backend, steps, AppliedTun, JournalState, RecoveryDriver, RecoveryOutcome, TunBackend,
    TunCapability, TunConfig, TunError, TunErrorCode, TunJournal, TunStack,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::lock_poisoned;
use crate::orchestrate::{
    build_core_paths, generate_config, orchestrate_disable_system_proxy,
    orchestrate_enable_system_proxy,
};

fn map_tun(err: TunError) -> AppError {
    AppError::new(tun_code(err.code), err.message)
}

fn tun_code(code: ice_tun_sys::TunErrorCode) -> ErrorCode {
    use ice_tun_sys::TunErrorCode;
    match code {
        TunErrorCode::NotSupported => ErrorCode::TunNotSupported,
        TunErrorCode::PermissionRequired => ErrorCode::TunPermissionRequired,
        TunErrorCode::ApplyFailed => ErrorCode::TunApplyFailed,
        TunErrorCode::RestoreFailed => ErrorCode::TunRestoreFailed,
        TunErrorCode::HealthcheckFailed => ErrorCode::TunHealthcheckFailed,
        TunErrorCode::RecoveryRequired => ErrorCode::TunRecoveryRequired,
        TunErrorCode::InvalidArgument => ErrorCode::TunInvalidArgument,
        TunErrorCode::ConfigRejected => ErrorCode::TunConfigRejected,
    }
}

/// Bounded wait for the app's own Clash API connections to release the core
/// ports before the next core starts.
///
/// The traffic monitor holds a long-lived `/traffic` stream to the clash API
/// port. When the previous core dies (TUN disable / enable hand-off), the
/// server's graceful close leaves that socket in `CLOSE_WAIT` until the
/// monitor's read loop (1.5 s bound, `STREAM_READ_TIMEOUT` in
/// `ice_core::traffic`) consumes the EOF. On macOS a half-closed loopback
/// connection still occupies its peer port: a fresh listener bind on the
/// same port fails with `EADDRINUSE` in that window, which would surface as
/// the next core's `bind: address already in use` even though no real holder
/// exists. The probe mirrors the core's own bind options (SO_REUSEADDR,
/// like sing-box's Go listeners), so it reports the port blocked only while
/// a live holder exists, not during the `TIME_WAIT` window. The budget is
/// the monitor's 1.5 s read-loop bound plus a 0.6 s margin: a normal
/// hand-off settles within it, while a genuinely held port stalls at most
/// ~2 s before the start fails fast and stays fail-closed.
fn wait_for_core_ports_released(settings: &AppSettings) -> bool {
    // 7 * 300 ms = 2.1 s: the 1.5 s monitor read-loop bound + 0.6 s margin.
    const RELEASE_WAIT_TRIES: u32 = 7;
    const RELEASE_WAIT_DELAY_MS: u64 = 300;
    let probes: [(String, u16); 2] = [
        (settings.mixed_listen.clone(), settings.mixed_port),
        (settings.clash_api_listen.clone(), settings.clash_api_port),
    ];
    for _ in 0..RELEASE_WAIT_TRIES {
        if probes
            .iter()
            .all(|(host, port)| ice_core::tcp_bind_available(host, *port))
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(RELEASE_WAIT_DELAY_MS));
    }
    tracing::warn!(
        "core ports not bindable after {} ms; the next core start may fail with bind: address already in use",
        u64::from(RELEASE_WAIT_TRIES) * RELEASE_WAIT_DELAY_MS
    );
    false
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
    /// True when the platform must not surface TUN controls at all.
    /// Currently only Windows: the backend reports `supported=false` when
    /// no bundled sing-box binary is present (deferred coordinator), so the
    /// controls stay hidden. Derived from the backend capability rather than
    /// a bare `cfg!` so the controls reappear automatically when the
    /// capability turns supported.
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

/// `tun.enabled` is the desired backend for the *next* service start. A
/// settings save that only flips that flag must persist it without touching
/// the live capture (no start, no stop, no backend switch).
pub fn only_tun_enabled_changed(previous: &AppSettings, next: &AppSettings) -> bool {
    let mut left = previous.clone();
    let mut right = next.clone();
    left.tun.enabled = false;
    right.tun.enabled = false;
    left == right && previous.tun.enabled != next.tun.enabled
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
            // Windows hides TUN controls while the backend is unsupported
            // (e.g. no bundled binary → deferred coordinator →
            // tun.permission_required). Deriving from the backend capability
            // (not a bare `cfg!(windows)`) keeps the two in lockstep: the
            // controls reappear automatically when the capability turns
            // supported.
            tun_ui_hidden: cfg!(target_os = "windows") && !capability.supported,
        }
    }
}

mod journal;
mod recovery;
mod transition;

#[cfg(test)]
mod tests;
