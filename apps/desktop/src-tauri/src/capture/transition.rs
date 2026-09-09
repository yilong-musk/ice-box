// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

impl CaptureController {
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
                    return Err(AppError::new(
                        ErrorCode::ProxyApplyFailed,
                        "TUN capture is still active; stop TUN before enabling the system proxy",
                    ));
                }
                TrafficCapture::Inactive => {}
            }
            match inner.tun_status {
                TunStatus::Preparing | TunStatus::Stopping | TunStatus::RecoveryRequired => {
                    return Err(AppError::new(
                        ErrorCode::ProxyApplyFailed,
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
    pub(crate) fn require_clean_journal_for_proxy(&self) -> Result<(), AppError> {
        let journal = match TunJournal::load(&self.paths.tun_state()) {
            Ok(journal) => journal,
            Err(err) => {
                let app_err = AppError::new(
                    ErrorCode::TunRecoveryRequired,
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
            let app_err = AppError::new(
                ErrorCode::TunRecoveryRequired,
                "TUN journal is not clean; run recovery before enabling system proxy",
            );
            let _ = self.fail_transition(TunStatus::RecoveryRequired, &app_err);
            return Err(app_err);
        }
        Ok(())
    }

    pub(crate) fn begin_transition(
        &self,
        status: TunStatus,
        transition_id: String,
    ) -> Result<(), AppError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| lock_poisoned("capture state"))?;
        inner.tun_status = status;
        inner.transition_id = Some(transition_id);
        inner.tun_error = None;
        Ok(())
    }

    pub(crate) fn finish_transition(
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
        // elevated core wrote to the helper log (macOS) or the ProgramData
        // run-dir log (Windows scheduled task). Latch so the log view keeps
        // merging that file after TUN stops.
        if active == TrafficCapture::Tun && !ice_tun_sys::dev_sudo_runner_enabled() {
            inner.helper_core_used = true;
        }
        Ok(())
    }

    /// Fail a transition: no backend claimed, status + error recorded.
    pub(crate) fn fail_transition(
        &self,
        status: TunStatus,
        err: &AppError,
    ) -> Result<(), AppError> {
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
            return Err(AppError::new(
                ErrorCode::TunNotSupported,
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
                return Err(AppError::new(
                    ErrorCode::TunRecoveryRequired,
                    "TUN cleanup is unverified; run recovery before enabling capture",
                ));
            }
            match inner.active {
                TrafficCapture::SystemProxy => {
                    return Err(AppError::new(
                        ErrorCode::TunApplyFailed,
                        "system proxy is active; disable it before enabling TUN",
                    ));
                }
                TrafficCapture::Tun if inner.tun_status == TunStatus::Enabled => {
                    // Already enabled: the resolved name is unchanged.
                    return Ok(None);
                }
                TrafficCapture::Tun => {
                    return Err(AppError::new(
                        ErrorCode::TunApplyFailed,
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
                let app_err = AppError::new(
                    ErrorCode::TunRecoveryRequired,
                    "TUN journal is not clean; run recovery before enabling capture",
                );
                let _ = self.fail_transition(TunStatus::RecoveryRequired, &app_err);
                return Err(app_err);
            }
            Ok(false) => {}
            Err(err) => {
                let app_err = AppError::new(
                    ErrorCode::TunRecoveryRequired,
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
            let app_err = AppError::new(
                ErrorCode::TunHelperStale,
                "the helper still runs the old core: update the helper in Settings first",
            );
            let _ = self.fail_transition(TunStatus::Error, &app_err);
            return Err(app_err);
        }

        // Reclaim any leftover core before starting a new capture (idempotent
        // through the elevated coordinator): a previous session may have left
        // a root-owned core holding the inbound/Clash API ports, which would
        // otherwise fail the next Start with `bind: address already in use`.
        // Runs after the active-backend checks above, so a running TUN is
        // never stopped here.
        backend.stop_elevated_core().map_err(map_tun)?;

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
                    let _ = self.rewrite_config(settings, CaptureIntent::Diagnostic);
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
                let status = if err.is_code(ErrorCode::TunPermissionRequired) {
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
    pub(crate) fn enable_tun_inner(
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
        if let Err(err) = self.rewrite_config(&tun_settings, CaptureIntent::Tun) {
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

        // The app's own clash API `/traffic` stream can keep the ports in
        // CLOSE_WAIT right after the previous core's shutdown; wait for them
        // to be bindable so the elevated core's fresh bind cannot fail with
        // `bind: address already in use` (macOS loopback behavior).
        if !wait_for_core_ports_released(&tun_settings) {
            tracing::warn!("enable_tun: core ports still occupied after core stop");
        }

        // The backend starts the elevated core with config.json (coordinator)
        // and journals the observed ownership.
        let applied = match backend.apply(&prepared) {
            Ok(applied) => applied,
            Err(err) => {
                if err.code == ErrorCode::TunPermissionRequired {
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
            tracing::error!(
                ?health,
                interface = %applied.interface_name.as_deref().unwrap_or("none"),
                expected_addresses = ?applied.expected_addresses,
                expected_routes = ?applied.expected_routes,
                dns_before_snapshot_len = applied
                    .dns_before
                    .as_ref()
                    .map(|snapshot| snapshot.platform_snapshot.len()),
                dns_after_snapshot_len = applied
                    .dns_after
                    .as_ref()
                    .map(|snapshot| snapshot.platform_snapshot.len()),
                "tun readiness checks disagreed"
            );
            let _ = core.stop(&self.paths.pid());
            let _ = backend.restore(&applied);
            self.journal_error("tun readiness checks disagreed")?;
            return Err(AppError::new(
                ErrorCode::TunHealthcheckFailed,
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
    pub(crate) fn fail_after_adopt_failure(
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
                let _ = self.rewrite_config(settings, CaptureIntent::Diagnostic);
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
                    &AppError::new(
                        ErrorCode::TunRecoveryRequired,
                        format!(
                            "failed to take over the elevated core ({err}) and cleanup is unconfirmed ({})",
                            release_app_err.message
                        ),
                    ),
                );
                Err(AppError::new(
                    ErrorCode::TunRecoveryRequired,
                    format!(
                        "failed to take over the elevated core and TUN cleanup is unconfirmed ({}); fail-closed",
                        release_app_err.message
                    ),
                ))
            }
        }
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

    pub(crate) fn disable_tun(
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
                AppError::new(
                    ErrorCode::TunRestoreFailed,
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
        tracing::info!(iface = ?applied.interface_name, "disable_tun: restore start");
        let restore_result = backend.restore(&applied);
        tracing::info!(result = ?restore_result.as_ref().map(|_| "ok").map_err(|e| e.message.as_str()), "disable_tun: restore done");
        let stop_result = core.stop(&self.paths.pid());
        tracing::info!(result = ?stop_result.as_ref().map(|_| "ok").map_err(|e| e.to_string()), "disable_tun: core stop done");
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
                return Err(AppError::new(
                    ErrorCode::TunRecoveryRequired,
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
                return Err(AppError::new(
                    ErrorCode::TunRecoveryRequired,
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
            tracing::info!("disable_tun: regenerating diagnostic config and restarting core");
            self.rewrite_config(settings, CaptureIntent::Diagnostic)?;
            let core_paths = build_core_paths(&self.paths, settings, binary);
            // The app's own clash API `/traffic` stream can still hold the
            // clash port in CLOSE_WAIT right after the elevated core's
            // shutdown; wait for the ports to be bindable again before the
            // restart (macOS would otherwise fail the fresh bind).
            if !wait_for_core_ports_released(settings) {
                tracing::warn!("disable_tun: core ports still occupied after restore");
            }
            core.start(&core_paths).map_err(AppError::from)?;
        }
        Ok(())
    }
}
