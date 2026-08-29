//! TUN capture platform boundary (plan §4.5, T0 slice).
//!
//! `ice-tun-sys` owns the TUN mutation journal, the platform backend
//! contract, and the startup/watchdog recovery driver. It performs no
//! OS mutation itself: platform backends do, recording every journaled
//! mutation boundary. System-proxy backup data is never reused for TUN
//! state (see `ice-proxy-sys`).
//!
//! T0 ships the host-free core (journal + contract + fake backend +
//! recovery) and the fault-injection tests that prove recovery is
//! idempotent; the macOS / Windows backends land in T2.

pub mod backend;
pub mod error;
pub mod fake;
pub mod journal;
pub mod recovery;

pub use backend::{
    unsupported_capability, AppliedTun, PreparedTun, RecoveryOutcome, TunBackend, TunCapability,
    TunConfig, TunHealth, TunStack,
};
pub use error::{TunError, TunErrorCode};
pub use journal::{steps, CidrRecord, DnsSnapshot, JournalState, RouteRecord, TunJournal};
pub use recovery::RecoveryDriver;
