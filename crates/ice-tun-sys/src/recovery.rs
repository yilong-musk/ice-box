//! Startup / watchdog recovery driver (plan §4.4).
//!
//! The driver is the *only* path that resolves an outstanding journal. It
//! verifies the owner token, never enables capture, resumes an idempotent
//! restore from the last completed step, and persists the terminal state
//! (`clean` only after verification; `recovery_required` when cleanup
//! cannot be confirmed).
//!
//! T3 orchestrates this driver inside the orchestration lock: reclamation
//! of orphan sing-box processes happens before `recover()` is called.

use std::path::Path;

use crate::backend::{RecoveryOutcome, TunBackend};
use crate::error::TunError;
use crate::journal::{steps, JournalState, TunJournal};

/// Drives one recovery attempt for the active installation.
pub struct RecoveryDriver<'a> {
    journal_path: &'a Path,
    backend: &'a mut dyn TunBackend,
    /// `ice-box:<installation-id>`; journals with a different token are
    /// treated as foreign and never touched.
    owner_token: &'a str,
}

impl<'a> RecoveryDriver<'a> {
    pub fn new(
        journal_path: &'a Path,
        backend: &'a mut dyn TunBackend,
        owner_token: &'a str,
    ) -> Self {
        Self {
            journal_path,
            backend,
            owner_token,
        }
    }

    /// Attempt startup recovery:
    ///
    /// 1. No journal → nothing to do.
    /// 2. Foreign owner token → nothing is touched (`ForeignJournal`).
    /// 3. `clean` journal → nothing to do.
    /// 4. Otherwise run the backend's idempotent release + verification and
    ///    persist `clean` / `recovery_required`.
    ///
    /// Never enables capture, even when `settings.json` says TUN is enabled.
    pub fn recover(&mut self) -> Result<RecoveryOutcome, TunError> {
        let Some(journal) = TunJournal::load(self.journal_path)? else {
            return Ok(RecoveryOutcome::NothingToDo);
        };
        if journal.owner_token != self.owner_token {
            tracing::warn!(
                token = %journal.owner_token,
                "tun journal belongs to another installation; refusing to touch TUN state"
            );
            return Ok(RecoveryOutcome::ForeignJournal);
        }
        if journal.state == JournalState::Clean {
            return Ok(RecoveryOutcome::NothingToDo);
        }

        match self.backend.recover(&journal) {
            Ok(RecoveryOutcome::Cleaned) => {
                // The backend may have advanced granular steps; re-read the
                // journal so the persisted `clean` record is authoritative.
                let mut journal = TunJournal::load(self.journal_path)?.unwrap_or(journal);
                journal.record(
                    self.journal_path,
                    JournalState::Clean,
                    steps::VERIFY_CLEAN,
                    |j| {
                        j.interface_name = None;
                        j.interface_id = None;
                        j.addresses.clear();
                        j.routes.clear();
                        j.dns_before = None;
                        j.dns_after = None;
                    },
                )?;
                tracing::info!("tun recovery: all owned resources verified removed");
                Ok(RecoveryOutcome::Cleaned)
            }
            Ok(RecoveryOutcome::RecoveryRequired) => {
                let mut journal = TunJournal::load(self.journal_path)?.unwrap_or(journal);
                let step = journal.last_completed_step.clone();
                journal.record(
                    self.journal_path,
                    JournalState::RecoveryRequired,
                    &step,
                    |_| {},
                )?;
                tracing::warn!("tun recovery: cleanup not verified; capture fail-closed");
                Ok(RecoveryOutcome::RecoveryRequired)
            }
            Ok(other) => Ok(other),
            Err(err) => {
                // Uncertain cleanup: persist RecoveryRequired (fail closed),
                // keep the journal for the next watchdog tick / startup.
                let mut journal = TunJournal::load(self.journal_path)?.unwrap_or(journal);
                let step = journal.last_completed_step.clone();
                let _ = journal.record(
                    self.journal_path,
                    JournalState::RecoveryRequired,
                    &step,
                    |_| {},
                );
                tracing::error!(error = %err, "tun recovery failed; state persisted as recovery_required");
                Err(err)
            }
        }
    }
}
