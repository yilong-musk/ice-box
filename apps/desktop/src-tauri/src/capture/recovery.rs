// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

impl CaptureController {
    /// Watchdog path: sing-box exited unexpectedly while TUN was claimed.
    /// Runs the idempotent release + verification and writes the Diagnostic
    /// config so a later automatic core start cannot recreate TUN. Returns a
    /// UI warning when cleanup cannot be confirmed (fail-closed).
    pub fn handle_unexpected_core_exit(
        &self,
        core: &mut dyn CoreHandle,
        settings: &AppSettings,
    ) -> Option<UiMessage> {
        if self.active_backend() != TrafficCapture::Tun {
            return None;
        }
        let mut backend = match self.backend.lock() {
            Ok(backend) => backend,
            Err(_) => return Some(UiMessage::new("recover.controllerUnavailable")),
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
                    &AppError::new(
                        ErrorCode::TunRecoveryRequired,
                        "journal missing while TUN capture was active",
                    ),
                );
                return Some(UiMessage::new("recover.tunJournalMissing"));
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
                let _ = self.rewrite_config(settings, CaptureIntent::Diagnostic);
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
                    &AppError::new(
                        ErrorCode::TunRecoveryRequired,
                        format!("TUN cleanup unconfirmed ({err})"),
                    ),
                );
                Some(
                    UiMessage::new("recover.tunCleanupUnconfirmed").with("detail", err.to_string()),
                )
            }
        }
    }

    /// Reclaim a leftover root-owned core from a previous session. The
    /// unprivileged process cannot signal a helper-started core, so stop it
    /// through the elevated coordinator unconditionally (idempotent: a healthy
    /// session with no leftover core is a no-op) — this guarantees a later
    /// start / re-enable never hits `bind: address already in use` even when
    /// the pid file was cleared. Must only run while no TUN capture is active
    /// (startup, before any capture claims the backend).
    pub fn reclaim_orphan_elevated_core(&self, core: &mut dyn CoreHandle) -> Result<(), AppError> {
        {
            let mut backend = self
                .backend
                .lock()
                .map_err(|_| lock_poisoned("capture backend"))?;
            // The coordinator's Stop is idempotent and helper-side reclaims a
            // leftover core recorded in the pid file even after a helper
            // restart; a non-orphan / absent core is left untouched there.
            backend.stop_elevated_core().map_err(map_tun)?;
        }
        // Converge the app-side state and clear any stale pid file.
        core.reclaim_orphan_pid(&self.paths.pid())
            .map_err(AppError::from)
    }

    /// Self-heal after wake / network change: while TUN capture is active, if
    /// the interface and routes are intact but the platform DNS drifted from
    /// the journaled snapshot (the classic "TUN is on but nothing resolves"
    /// after sleep), re-apply the DNS snapshot through the elevated
    /// coordinator. Returns a UI warning when a repair fails.
    pub fn heal_tun_dns(&self) -> Option<UiMessage> {
        if self.active_backend() != TrafficCapture::Tun {
            return None;
        }
        let journal = match TunJournal::load(&self.paths.tun_state()) {
            Ok(Some(journal)) => journal,
            _ => return None,
        };
        let applied = AppliedTun::from_journal(&journal);
        let mut backend = match self.backend.lock() {
            Ok(backend) => backend,
            Err(_) => return Some(UiMessage::new("recover.controllerUnavailable")),
        };
        let health = match backend.verify(&applied) {
            Ok(health) => health,
            Err(_) => return None, // verify errors are owned by the core-exit watchdog
        };
        if !health.interface_up || !health.addresses_present || !health.routes_owned {
            return None; // the TUN itself is gone; the unexpected-exit watchdog owns it
        }
        if health.dns_consistent {
            return None;
        }
        match backend.reapply_dns_if_stale(&applied) {
            Ok(true) => {
                tracing::info!("TUN DNS re-applied after wake / network change");
                None
            }
            Ok(false) => None,
            Err(err) => {
                tracing::error!(error = %err, "TUN DNS re-apply failed");
                Some(UiMessage::new("recover.tunDnsFailed").with("detail", err.message.clone()))
            }
        }
    }

    /// Recovery (inside the orchestration lock, plan §4.4): discard an
    /// interrupted settings transaction, then run the journal recovery
    /// driver. Never enables capture. Returns a UI warning when anything
    /// needs attention. Used by startup (after orphan-core reclamation)
    /// and by the on-demand「重试恢复」action from the UI.
    pub fn recover(&self, core: &mut dyn CoreHandle) -> Result<Vec<UiMessage>, AppError> {
        let mut warnings: Vec<UiMessage> = Vec::new();

        if self.paths.pending_settings().is_file() {
            tracing::warn!(
                "interrupted settings transaction; committed settings are the old state"
            );
            let _ = fs::remove_file(self.paths.pending_settings());
            warnings.push(UiMessage::new("recover.pendingSettings"));
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
                    let recovery_err = AppError::new(
                        ErrorCode::TunRecoveryRequired,
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
                    let _ = self.rewrite_config(&settings, CaptureIntent::Diagnostic);
                }
                // The OS system proxy may still be applied (startup recovery
                // runs after the proxy backup was restored into the live OS,
                // or recovery is retried mid-session). The controller must
                // not report Inactive while the system proxy is still owned —
                // a later TUN enable would double-capture. The disk record is
                // authoritative: the memory flag alone could have been lost.
                if self.active_backend() == TrafficCapture::SystemProxy
                    || proxy_backup_indicates_ownership(&self.paths.proxy_backup())
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
                    &AppError::new(
                        ErrorCode::TunRecoveryRequired,
                        "TUN cleanup unconfirmed; new TUN activation is blocked",
                    ),
                );
                warnings.push(UiMessage::new("recover.tunCleanupRetry"));
            }
        }
        let _ = core;
        Ok(warnings)
    }

    /// Serialized live-TUN reconfigure when TUN topology changed while TUN
    /// capture is already active (plan §4.3). `tun.enabled` is *not* a live
    /// switch: flipping it is a next-start desire and is persisted without
    /// calling this. Writes the pending record first; commits `settings.json`
    /// only after the requested backend is healthy; on failure rolls back to
    /// the old backend; clears the pending record in both cases. An uncertain
    /// rollback leaves both backends disabled and enters `RecoveryRequired`.
    /// When the active backend and the committed settings already agree with
    /// the candidate, no transition runs: the candidate is persisted as-is.
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
            save_settings_for(&self.paths.settings(), &candidate, host_platform())?;
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
                match save_settings_for(&self.paths.settings(), &committed, host_platform()) {
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
                                    &AppError::new(
                                        ErrorCode::TunRecoveryRequired,
                                        format!(
                                            "settings commit failed ({commit_err}) and backend rollback is unconfirmed ({rollback_err})"
                                        ),
                                    ),
                                );
                                Err(AppError::new(
                                    ErrorCode::TunRecoveryRequired,
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
                            &AppError::new(
                                ErrorCode::TunRecoveryRequired,
                                format!("switch failed ({err}) and rollback is unconfirmed ({rollback_err})"),
                            ),
                        );
                        Err(AppError::new(
                            ErrorCode::TunRecoveryRequired,
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
    pub(crate) fn rollback_after_commit_failure(
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
    pub(crate) fn reconciled_candidate(&self, settings: &AppSettings) -> AppSettings {
        use ice_engine::{load_active_profile, load_index, SubscriptionPaths};
        let sub_paths = SubscriptionPaths::from_app(&self.paths);
        match load_index(&sub_paths)
            .ok()
            .and_then(|index| load_active_profile(&sub_paths, &index, host_platform()).ok())
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
                    Ok(_) => Err(AppError::new(
                        ErrorCode::TunRecoveryRequired,
                        "TUN recovery remains unverified after apply failure",
                    )),
                    Err(recovery_err) => Err(recovery_err),
                };
                match rollback {
                    Ok(_) => Err(err),
                    Err(rollback_err) => {
                        let _ = self.fail_transition(
                            TunStatus::RecoveryRequired,
                            &AppError::new(
                                ErrorCode::TunRecoveryRequired,
                                format!("apply failed ({err}) and TUN rollback is unconfirmed ({rollback_err})"),
                            ),
                        );
                        Err(AppError::new(
                            ErrorCode::TunRecoveryRequired,
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
    pub(crate) fn rollback_backend(
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
                return Err(AppError::new(
                    ErrorCode::TunRecoveryRequired,
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
