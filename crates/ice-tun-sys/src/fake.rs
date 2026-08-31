//! Host-free fake backend for orchestration and fault-injection tests
//! (plan T0 exit gate: "Inject failures after every journaled mutation in
//! a host-free fake controller and prove that startup recovery is
//! idempotent").
//!
//! The fake simulates the OS resource state (interface, addresses, routes,
//! DNS), is idempotent, and writes the same journal steps a real backend
//! writes. It models IPv4 *and* IPv6 routes (dual-stack lock, architecture
//! §24.5), so a dual-stack or IPv6-only config can never pass health checks
//! while IPv6 leaks. `FaultPlan` scripted failures can fire after any
//! journaled mutation — including the crash window between an OS mutation
//! and its journal record, where the fake rolls the mutation back so an
//! unjournaled resource can never leak — and tests can then prove recovery.

use std::path::PathBuf;

use crate::backend::{
    AppliedTun, PreparedTun, RecoveryOutcome, TunBackend, TunCapability, TunConfig, TunHealth,
};
use crate::error::{TunError, TunErrorCode};
use crate::journal::{steps, CidrRecord, DnsSnapshot, JournalState, RouteRecord, TunJournal};
use crate::routes;
pub use crate::routes::{AUTO_ROUTE_RANGES, AUTO_ROUTE_RANGES_V6};

/// Simulated adapter interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeInterface {
    pub name: String,
    pub id: String,
}

/// The fake platform's OS resource state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FakeOsState {
    pub interface: Option<FakeInterface>,
    pub addresses: Vec<String>,
    pub routes: Vec<RouteRecord>,
    /// Current platform DNS resolver (simulated); `None` = platform default.
    pub dns_current: Option<String>,
}

/// Scripted failures. Each `Option<usize>` counts *completed* journaled
/// mutations, so `Some(k)` fails right after the k-th mutation.
#[derive(Debug, Clone, Default)]
pub struct FaultPlan {
    /// Capability reports unsupported with this reason.
    pub capability_reason: Option<String>,
    pub fail_prepare: bool,
    pub fail_apply_after_mutations: Option<usize>,
    /// Fail the journal write for the k-th apply mutation, i.e. crash in the
    /// window between the OS mutation and its ownership record. The fake
    /// must roll the mutation back so no unjournaled resource can leak.
    pub fail_journal_write_after_mutations: Option<usize>,
    pub fail_verify_applied: bool,
    /// Control path reports unreachable even though resources are applied.
    pub control_path_broken: bool,
    pub fail_restore_after_mutations: Option<usize>,
    /// A route that cannot be removed (unverifiable cleanup).
    pub stuck_route: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceEvent {
    Capability,
    Prepare,
    InterfaceCreated(String),
    AddressesAssigned,
    RoutesAdded,
    DnsApplied,
    Verify,
    RoutesRemoved,
    InterfaceRemoved,
    DnsRestored,
}

pub struct FakeTunBackend {
    pub state: FakeOsState,
    pub faults: FaultPlan,
    trace: std::cell::RefCell<Vec<TraceEvent>>,
    pub owner_token: String,
    journal_path: Option<PathBuf>,
}

impl FakeTunBackend {
    pub fn new(owner_token: impl Into<String>) -> Self {
        Self {
            state: FakeOsState::default(),
            faults: FaultPlan::default(),
            trace: std::cell::RefCell::new(Vec::new()),
            owner_token: owner_token.into(),
            journal_path: None,
        }
    }

    /// Snapshot of every backend operation so tests can assert ordering.
    pub fn trace(&self) -> Vec<TraceEvent> {
        self.trace.borrow().clone()
    }

    fn push(&self, event: TraceEvent) {
        self.trace.borrow_mut().push(event);
    }

    /// Attach the journal file the driver uses; the fake updates it after
    /// every granular mutation, exactly like a real backend.
    pub fn with_journal(mut self, path: PathBuf) -> Self {
        self.journal_path = Some(path);
        self
    }

    fn journal_record(
        &self,
        step: &str,
        mutate: impl FnOnce(&mut TunJournal),
    ) -> Result<(), TunError> {
        let Some(path) = &self.journal_path else {
            return Ok(());
        };
        let mut journal = TunJournal::load(path)?
            .unwrap_or_else(|| TunJournal::new("unknown".into(), self.owner_token.clone()));
        journal.last_completed_step = step.to_string();
        journal.updated_at = chrono::Utc::now().to_rfc3339();
        mutate(&mut journal);
        journal.save(path)
    }

    /// Apply one granular OS mutation and immediately persist its journal
    /// record. If the journal write fails — an injected crash in the
    /// mutation→journal window, or a real I/O error — the mutation is rolled
    /// back: a resource whose ownership was never recorded must not survive,
    /// because recovery is only authorized to delete journaled resources.
    /// `before` is captured before `mutate`, and `event` is traced only
    /// after both the mutation and its journal record succeeded.
    fn mutate_and_journal(
        &mut self,
        step: &str,
        mutate: impl FnOnce(&mut FakeOsState),
        journal: impl FnOnce(&mut TunJournal),
        event: TraceEvent,
        mutations: usize,
        fail_journal_write: Option<usize>,
    ) -> Result<(), TunError> {
        let before = self.state.clone();
        mutate(&mut self.state);
        let failure = if fail_journal_write == Some(mutations) {
            Some(TunError::new(
                TunErrorCode::ApplyFailed,
                format!("injected journal write failure after {mutations} mutations"),
            ))
        } else {
            self.journal_record(step, journal).err()
        };
        if let Some(err) = failure {
            self.state = before;
            return Err(err);
        }
        self.push(event);
        Ok(())
    }

    fn fail_after(
        &self,
        done: Option<usize>,
        mutations: usize,
        code: TunErrorCode,
    ) -> Result<(), TunError> {
        if done == Some(mutations) {
            return Err(TunError::new(
                code,
                format!("injected failure after {mutations} mutations"),
            ));
        }
        Ok(())
    }

    /// `Some(0)` means "fail before the first mutation".
    fn fail_before(&self, done: Option<usize>) -> Result<(), TunError> {
        if done == Some(0) {
            return Err(TunError::new(
                TunErrorCode::ApplyFailed,
                "injected failure before any mutation",
            ));
        }
        Ok(())
    }

    /// The route set for `auto_route`: the IPv4 and IPv6 sub-range sets. TUN
    /// address destinations are excluded by the locked policy and are not
    /// claimed as routes by the native macOS backend.
    fn build_auto_routes(config: &TunConfig) -> Vec<RouteRecord> {
        let mut routes = Vec::new();
        if config.auto_route {
            if routes::has_v4(&config.addresses) {
                routes.extend(AUTO_ROUTE_RANGES.iter().map(|d| RouteRecord {
                    destination: (*d).to_string(),
                    gateway: Some("10.0.0.2".into()),
                    metric: 0,
                    owned: true,
                }));
            }
            if routes::has_v6(&config.addresses) {
                routes.extend(AUTO_ROUTE_RANGES_V6.iter().map(|d| RouteRecord {
                    destination: (*d).to_string(),
                    gateway: Some("fdfe:dcba:9876::2".into()),
                    metric: 0,
                    owned: true,
                }));
            }
        }
        routes
    }
}

impl TunBackend for FakeTunBackend {
    fn capability(&self) -> TunCapability {
        self.push(TraceEvent::Capability);
        if let Some(reason) = &self.faults.capability_reason {
            return TunCapability {
                supported: false,
                reason: Some(reason.clone()),
                ipv4: false,
                ipv6: false,
                dns_hijack: false,
            };
        }
        TunCapability {
            supported: true,
            reason: None,
            ipv4: true,
            ipv6: true,
            dns_hijack: true,
        }
    }

    fn prepare(&self, config: &TunConfig) -> Result<PreparedTun, TunError> {
        self.push(TraceEvent::Prepare);
        if self.faults.fail_prepare {
            return Err(TunError::new(
                TunErrorCode::ApplyFailed,
                "injected prepare failure",
            ));
        }
        if config.addresses.is_empty() {
            return Err(TunError::new(
                TunErrorCode::ApplyFailed,
                "tun config requires at least one address",
            ));
        }
        // A config that claims IPv6 support must be able to install IPv6
        // routes; an unparseable IPv6 address would silently skip them and
        // leak IPv6 traffic (dual-stack lock).
        for cidr in &config.addresses {
            if cidr.contains(':') && routes::ipv6_groups(cidr).is_none() {
                return Err(TunError::new(
                    TunErrorCode::ApplyFailed,
                    format!("invalid IPv6 address in tun config: {cidr}"),
                ));
            }
        }
        Ok(PreparedTun {
            config: config.clone(),
        })
    }

    fn apply(&mut self, prepared: &PreparedTun) -> Result<AppliedTun, TunError> {
        let config = &prepared.config;
        let mut mutations = 0usize;
        let fail_journal_write = self.faults.fail_journal_write_after_mutations;
        self.fail_before(self.faults.fail_apply_after_mutations)?;

        if self.state.interface.is_none() {
            let name = config
                .interface_name
                .clone()
                .unwrap_or_else(|| "fake-tun".into());
            let id = format!("id-{name}");
            mutations += 1;
            self.mutate_and_journal(
                steps::INTERFACE_CREATED,
                |state| {
                    state.interface = Some(FakeInterface {
                        name: name.clone(),
                        id: id.clone(),
                    });
                },
                |j| {
                    j.interface_name = Some(name.clone());
                    j.interface_id = Some(id.clone());
                    j.expected_addresses = config.addresses.clone();
                    j.expected_routes = routes::auto_route_destinations(config);
                },
                TraceEvent::InterfaceCreated(name.clone()),
                mutations,
                fail_journal_write,
            )?;
            self.fail_after(
                self.faults.fail_apply_after_mutations,
                mutations,
                TunErrorCode::ApplyFailed,
            )?;
        }

        if self.state.addresses.is_empty() {
            let addresses = config.addresses.clone();
            mutations += 1;
            self.mutate_and_journal(
                steps::ADDRESSES_ASSIGNED,
                |state| {
                    state.addresses = addresses.clone();
                },
                |j| {
                    j.addresses = addresses
                        .iter()
                        .map(|cidr| CidrRecord {
                            cidr: cidr.clone(),
                            owned: true,
                        })
                        .collect();
                },
                TraceEvent::AddressesAssigned,
                mutations,
                fail_journal_write,
            )?;
            self.fail_after(
                self.faults.fail_apply_after_mutations,
                mutations,
                TunErrorCode::ApplyFailed,
            )?;
        }

        if self.state.routes.is_empty() {
            let routes = Self::build_auto_routes(config);
            mutations += 1;
            self.mutate_and_journal(
                steps::ROUTES_ADDED,
                |state| {
                    state.routes = routes.clone();
                },
                |j| {
                    j.routes = routes.clone();
                },
                TraceEvent::RoutesAdded,
                mutations,
                fail_journal_write,
            )?;
            self.fail_after(
                self.faults.fail_apply_after_mutations,
                mutations,
                TunErrorCode::ApplyFailed,
            )?;
        }

        let mut dns_before = None;
        let mut dns_after = None;
        if config.dns_hijack {
            // The journal's first-saved snapshot is the restore target; a
            // replayed apply must reuse it instead of snapshotting the
            // already-hijacked resolver (apply idempotence, backend.rs
            // contract).
            let recorded = self
                .journal_path
                .as_ref()
                .and_then(|path| TunJournal::load(path).ok().flatten())
                .and_then(|journal| journal.dns_before);
            dns_before = Some(recorded.unwrap_or_else(|| {
                DnsSnapshot {
                    platform_snapshot: self
                        .state
                        .dns_current
                        .clone()
                        .unwrap_or_else(|| "platform-default".into()),
                }
            }));
            dns_after = Some(DnsSnapshot {
                platform_snapshot: "fake-tun-resolver".into(),
            });

            if self.state.dns_current.as_deref() != Some("fake-tun-resolver") {
                mutations += 1;
                let before_snapshot = dns_before.clone();
                let after_snapshot = dns_after.clone();
                self.mutate_and_journal(
                    steps::DNS_APPLIED,
                    |state| {
                        state.dns_current = Some("fake-tun-resolver".into());
                    },
                    |j| {
                        j.dns_before = before_snapshot;
                        j.dns_after = after_snapshot;
                    },
                    TraceEvent::DnsApplied,
                    mutations,
                    fail_journal_write,
                )?;
                self.fail_after(
                    self.faults.fail_apply_after_mutations,
                    mutations,
                    TunErrorCode::ApplyFailed,
                )?;
            }
        }

        Ok(AppliedTun {
            interface_name: self.state.interface.as_ref().map(|i| i.name.clone()),
            interface_id: self.state.interface.as_ref().map(|i| i.id.clone()),
            addresses: self
                .state
                .addresses
                .iter()
                .map(|cidr| CidrRecord {
                    cidr: cidr.clone(),
                    owned: true,
                })
                .collect(),
            routes: self.state.routes.clone(),
            // Required sets: the fake installs exactly what the config
            // required, so verification against the required sets doubles as
            // the dual-stack / full-route lock in host-free tests too.
            expected_addresses: config.addresses.clone(),
            expected_routes: routes::auto_route_destinations(config),
            dns_before,
            dns_after,
            // The fake does not start an external core; the shell restarts
            // its own core on the generated config instead.
            core_pid: None,
        })
    }

    fn verify(&self, applied: &AppliedTun) -> Result<TunHealth, TunError> {
        self.push(TraceEvent::Verify);
        if self.faults.fail_verify_applied {
            return Err(TunError::new(
                TunErrorCode::HealthcheckFailed,
                "injected verify failure",
            ));
        }
        let interface_up = self.state.interface.as_ref().is_some_and(|i| {
            applied.interface_name.as_deref() == Some(i.name.as_str())
                && applied.interface_id.as_deref() == Some(i.id.as_str())
        });
        // Exact-address / full-route locks against the *required* sets: a
        // resource missing before it was recorded must never pass.
        let addresses_present = applied
            .expected_addresses
            .iter()
            .all(|addr| self.state.addresses.contains(addr));
        let routes_owned = applied.expected_routes.iter().all(|destination| {
            self.state
                .routes
                .iter()
                .any(|r| &r.destination == destination)
        });
        let dns_consistent = match &applied.dns_after {
            Some(after) => {
                self.state.dns_current.as_deref() == Some(after.platform_snapshot.as_str())
            }
            None => true,
        };
        let nothing_owned = self.state.interface.is_none()
            && applied
                .addresses
                .iter()
                .all(|a| !self.state.addresses.contains(&a.cidr))
            && applied
                .routes
                .iter()
                .filter(|r| r.owned)
                .all(|r| !self.state.routes.contains(r))
            && match &applied.dns_after {
                Some(after) => {
                    self.state.dns_current.as_deref() != Some(after.platform_snapshot.as_str())
                }
                None => true,
            };
        Ok(TunHealth {
            interface_up,
            addresses_present,
            routes_owned,
            dns_consistent,
            control_path_reachable: !self.faults.control_path_broken,
            nothing_owned,
        })
    }

    fn restore(&mut self, applied: &AppliedTun) -> Result<(), TunError> {
        let mut mutations = 0usize;
        self.fail_before(self.faults.fail_restore_after_mutations)?;

        let owned_routes: Vec<RouteRecord> =
            applied.routes.iter().filter(|r| r.owned).cloned().collect();
        // Keep non-owned routes (they belong to someone else) and owned
        // routes that could not be removed (unverifiable cleanup). Only
        // journaled owned routes that are actually removable are deleted.
        let remaining: Vec<RouteRecord> = self
            .state
            .routes
            .iter()
            .filter(|r| {
                let is_owned = owned_routes.iter().any(|o| o == *r);
                let is_stuck = self.faults.stuck_route.as_deref() == Some(r.destination.as_str());
                !is_owned || is_stuck
            })
            .cloned()
            .collect();
        if remaining.len() != self.state.routes.len() {
            self.state.routes = remaining;
            mutations += 1;
            self.push(TraceEvent::RoutesRemoved);
            self.journal_record(steps::ROUTES_REMOVED, |j| {
                j.routes = self.state.routes.clone();
            })?;
            self.fail_after(
                self.faults.fail_restore_after_mutations,
                mutations,
                TunErrorCode::RestoreFailed,
            )?;
        }

        // Interface + owned addresses are removed only when the live adapter
        // matches the journaled identity (name AND id). A replaced adapter
        // (e.g. same name, different id) or an adapter that exists before
        // ownership was journaled is an unverified resource and is never
        // deleted. Owned addresses are removed item by item; addresses not
        // recorded as owned (external) survive the cleanup.
        let interface_matches = match (
            &self.state.interface,
            applied.interface_name.as_deref(),
            applied.interface_id.as_deref(),
        ) {
            (Some(current), Some(name), Some(id)) => current.name == name && current.id == id,
            _ => false,
        };
        if interface_matches {
            let owned_cidrs: Vec<&String> = applied
                .addresses
                .iter()
                .filter(|a| a.owned)
                .map(|a| &a.cidr)
                .collect();
            let remaining_addresses: Vec<String> = self
                .state
                .addresses
                .iter()
                .filter(|a| !owned_cidrs.contains(a))
                .cloned()
                .collect();
            self.state.addresses = remaining_addresses;
            self.state.interface = None;
            mutations += 1;
            self.push(TraceEvent::InterfaceRemoved);
            self.journal_record(steps::INTERFACE_REMOVED, |j| {
                j.interface_name = None;
                j.interface_id = None;
                j.addresses.clear();
            })?;
            self.fail_after(
                self.faults.fail_restore_after_mutations,
                mutations,
                TunErrorCode::RestoreFailed,
            )?;
        }

        if let Some(after) = &applied.dns_after {
            match &self.state.dns_current {
                Some(current) if current == &after.platform_snapshot => {
                    self.state.dns_current = applied
                        .dns_before
                        .as_ref()
                        .map(|b| b.platform_snapshot.clone());
                    mutations += 1;
                    self.push(TraceEvent::DnsRestored);
                    self.journal_record(steps::DNS_RESTORED, |j| {
                        j.dns_before = None;
                        j.dns_after = None;
                    })?;
                    self.fail_after(
                        self.faults.fail_restore_after_mutations,
                        mutations,
                        TunErrorCode::RestoreFailed,
                    )?;
                }
                _ => {
                    // Compare-before-restore: an external DNS change is
                    // preserved, never overwritten with stale data. This is
                    // a *defined* fail-closed state (recovery_required),
                    // not an unexpected failure.
                    return Err(TunError::new(
                        TunErrorCode::RecoveryRequired,
                        "platform DNS no longer matches the journal's dns_after snapshot; external change preserved",
                    ));
                }
            }
        }

        Ok(())
    }

    fn recover(&mut self, journal: &TunJournal) -> Result<RecoveryOutcome, TunError> {
        if journal.state == JournalState::Clean {
            return Ok(RecoveryOutcome::NothingToDo);
        }
        let applied = AppliedTun::from_journal(journal);
        match self.restore(&applied) {
            Ok(()) => {}
            Err(err) if err.code == TunErrorCode::RecoveryRequired => {
                // Defined fail-closed state (e.g. external DNS change):
                // report it as the outcome; the driver persists the journal.
                return Ok(RecoveryOutcome::RecoveryRequired);
            }
            Err(err) => return Err(err),
        }
        let health = self.verify(&applied)?;
        if health.nothing_owned {
            Ok(RecoveryOutcome::Cleaned)
        } else {
            Ok(RecoveryOutcome::RecoveryRequired)
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn attach_journal(&mut self, path: PathBuf) {
        self.journal_path = Some(path);
    }
}
