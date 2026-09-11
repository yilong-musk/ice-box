// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

impl CaptureController {
    pub(crate) fn journal_clean(&self, context: &str) -> Result<(), AppError> {
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

    pub(crate) fn journal_error(&self, context: &str) -> Result<(), AppError> {
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
    pub(crate) fn journal_has_outstanding_records(&self) -> Result<bool, AppError> {
        let journal = TunJournal::load(&self.paths.tun_state()).map_err(map_tun)?;
        Ok(journal.is_some_and(|journal| journal.state != JournalState::Clean))
    }
}
