//! Fail-closed backend for platforms whose TUN gate is pending or failed
//! (plan §3.2 / §5 T2).
//!
//! `capability()` reports `supported=false` with the stable reason; every
//! operation returns `tun.not_supported` and never mutates the host. This is
//! what Windows and Linux hosts get until their own gate turns green.

use crate::backend::{
    unsupported_capability, AppliedTun, PreparedTun, RecoveryOutcome, TunBackend, TunCapability,
    TunConfig, TunHealth,
};
use crate::error::{TunError, TunErrorCode};
use crate::journal::TunJournal;

/// Backend that refuses every TUN operation with a stable reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedTunBackend {
    capability: TunCapability,
}

impl UnsupportedTunBackend {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            capability: unsupported_capability(reason),
        }
    }
}

impl TunBackend for UnsupportedTunBackend {
    fn capability(&self) -> TunCapability {
        self.capability.clone()
    }

    fn prepare(&self, _config: &TunConfig) -> Result<PreparedTun, TunError> {
        Err(TunError::new(
            TunErrorCode::NotSupported,
            "TUN is not available on this platform",
        ))
    }

    fn apply(&mut self, _prepared: &PreparedTun) -> Result<AppliedTun, TunError> {
        Err(TunError::new(
            TunErrorCode::NotSupported,
            "TUN is not available on this platform",
        ))
    }

    fn verify(&self, _applied: &AppliedTun) -> Result<TunHealth, TunError> {
        Err(TunError::new(
            TunErrorCode::NotSupported,
            "TUN is not available on this platform",
        ))
    }

    fn restore(&mut self, _applied: &AppliedTun) -> Result<(), TunError> {
        Err(TunError::new(
            TunErrorCode::NotSupported,
            "TUN is not available on this platform",
        ))
    }

    fn recover(&mut self, _journal: &TunJournal) -> Result<RecoveryOutcome, TunError> {
        // A platform that never runs TUN cannot own a journal; refuse rather
        // than guess. The startup driver surfaces the error and stays
        // fail-closed.
        Err(TunError::new(
            TunErrorCode::NotSupported,
            "TUN is not available on this platform",
        ))
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
