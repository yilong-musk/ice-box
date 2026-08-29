//! The platform backend contract for TUN capture (plan §4.5).
//!
//! `ice-tun-sys` backends expose only intent-level operations; they never
//! leak platform command strings into `ice-config` or the UI. `prepare` is
//! side-effect free. `apply`, `verify`, `restore` and `recover` are
//! idempotent and update the journal at each mutation boundary.
//!
//! Ownership model (T0 lock): the platform backend owns whatever it records
//! in the journal (`owned: true`); an unverified resource is never deleted.
//! On macOS the native sing-box path records resources owned by sing-box and
//! coordinates the core; on a helper path the helper performs the explicit
//! OS mutations. The two are never mixed for the same resource.

use crate::error::{TunError, TunErrorCode};
use crate::journal::{CidrRecord, DnsSnapshot, RouteRecord, TunJournal};

/// Validated TUN capture parameters (locked by the feasibility spike).
/// Field placement is provisional until T1 wires the persisted settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunConfig {
    /// Adapter interface name. macOS requires a `utun<N>` numeric suffix.
    pub interface_name: Option<String>,
    /// CIDR addresses assigned to the adapter (e.g. `10.0.0.1/30`).
    pub addresses: Vec<String>,
    pub mtu: u16,
    pub stack: TunStack,
    pub auto_route: bool,
    pub strict_route: bool,
    /// Whether this backend performs OS-level DNS interception (T0 decision;
    /// macOS native sing-box does not touch system DNS).
    pub dns_hijack: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunStack {
    Gvisor,
    System,
    Mixed,
}

impl TunStack {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gvisor => "gvisor",
            Self::System => "system",
            Self::Mixed => "mixed",
        }
    }
}

/// Static capability report. `supported == false` means the host can never
/// start a TUN transition; the UI shows `tun_available=false` with `reason`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunCapability {
    pub supported: bool,
    /// Stable, human-readable reason when unsupported (e.g. missing driver,
    /// unsupported platform). Never exposes privileged internals.
    pub reason: Option<String>,
    pub ipv4: bool,
    /// IPv6 is best effort; never presented as "all traffic".
    pub ipv6: bool,
    pub dns_hijack: bool,
}

/// Result of the side-effect-free `prepare` step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedTun {
    pub config: TunConfig,
}

/// Capture applied: the adapter identity and every owned resource, as the
/// journal must record them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedTun {
    pub interface_name: Option<String>,
    pub interface_id: Option<String>,
    pub addresses: Vec<CidrRecord>,
    pub routes: Vec<RouteRecord>,
    pub dns_before: Option<DnsSnapshot>,
    pub dns_after: Option<DnsSnapshot>,
}

impl AppliedTun {
    /// Rebuild the applied-resource view from a journal (recovery path).
    pub fn from_journal(journal: &TunJournal) -> Self {
        Self {
            interface_name: journal.interface_name.clone(),
            interface_id: journal.interface_id.clone(),
            addresses: journal.addresses.clone(),
            routes: journal.routes.clone(),
            dns_before: journal.dns_before.clone(),
            dns_after: journal.dns_after.clone(),
        }
    }
}

/// Health of an applied capture. A Clash API TCP success alone is never
/// sufficient: adapter identity, owned CIDRs, routes, DNS state and the
/// control path must all agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunHealth {
    /// The recorded interface exists, is up, and matches `interface_id`.
    pub interface_up: bool,
    /// Every journaled owned address is present.
    pub addresses_present: bool,
    /// Every journaled owned route is present.
    pub routes_owned: bool,
    /// DNS interception / bypass state matches the journal.
    pub dns_consistent: bool,
    /// Loopback / control path is reachable (no capture of the control path).
    pub control_path_reachable: bool,
    /// True when NO journaled owned resource remains (adapter, addresses,
    /// owned routes, applied DNS state all absent). Reported by the backend
    /// itself — an empty resource list must not be misread as "clean".
    pub nothing_owned: bool,
}

impl TunHealth {
    /// All healthy — only then may a transition claim capture as enabled.
    pub fn all_ok(&self) -> bool {
        self.interface_up
            && self.addresses_present
            && self.routes_owned
            && self.dns_consistent
            && self.control_path_reachable
    }
}

/// Outcome of an explicit recovery attempt (startup or watchdog retry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// No journal, or the journal was already verified clean.
    NothingToDo,
    /// All owned resources are confirmed removed; capture is disabled.
    Cleaned,
    /// Cleanup could not be verified. Fail-closed: capture stays disabled
    /// and new TUN activation is rejected until an explicit retry succeeds.
    RecoveryRequired,
    /// The journal belongs to another installation; nothing was touched.
    ForeignJournal,
}

/// The platform backend contract (plan §4.5).
///
/// Implementations must be idempotent: replaying `apply` / `restore` /
/// `recover` from any journaled step converges to the same terminal state.
pub trait TunBackend {
    /// Static capability probe; never performs OS mutations.
    fn capability(&self) -> TunCapability;

    /// Side-effect free validation of the desired capture parameters.
    fn prepare(&self, config: &TunConfig) -> Result<PreparedTun, TunError>;

    /// Perform the capture transition: create/own adapter, addresses,
    /// routes, DNS per the selected ownership model. Updates the journal
    /// after each mutation boundary. Idempotent.
    fn apply(&mut self, prepared: &PreparedTun) -> Result<AppliedTun, TunError>;

    /// Verify the applied capture against the journaled ownership records.
    /// Never fails solely because resources are gone; it reports state.
    fn verify(&self, applied: &AppliedTun) -> Result<TunHealth, TunError>;

    /// Release the capture: remove owned resources (compare-before-restore
    /// for DNS) and verify. Idempotent; unverified resources are kept.
    fn restore(&mut self, applied: &AppliedTun) -> Result<(), TunError>;

    /// Explicit recovery retry (startup / watchdog). Never enables capture;
    /// returns whether all owned resources are confirmed clean.
    fn recover(&mut self, journal: &TunJournal) -> Result<RecoveryOutcome, TunError>;
}

/// Reject a capture request whose platform cannot run TUN at all.
pub fn unsupported_capability(reason: impl Into<String>) -> TunCapability {
    TunCapability {
        supported: false,
        reason: Some(reason.into()),
        ipv4: false,
        ipv6: false,
        dns_hijack: false,
    }
}

impl From<TunErrorCode> for TunError {
    fn from(code: TunErrorCode) -> Self {
        Self::new(code, code.as_str())
    }
}
