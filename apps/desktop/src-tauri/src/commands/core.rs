// SPDX-License-Identifier: GPL-3.0-or-later

use super::tun::ensure_tun_elevation_inner;
use super::*;

/// Start the core process. Callers that already hold `orchestrate` skip
/// the orchestrate lock.
pub(crate) fn start_core_inner(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
    let settings = current_settings(&state.paths)?;
    {
        let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
        if core.state().status != CoreStatus::Running {
            // Do not hold `proxy` across spawn + healthcheck; start never applies OS proxy.
            let binary = binary_for(app)?;
            let _ = orchestrate_start(
                &state.paths,
                &settings,
                &mut **core,
                binary,
                resource_dir(app).as_deref(),
                CaptureIntent::Diagnostic,
            )?;
            clear_transient_recovery_warnings(state);
        }
    }
    attach_traffic(state, &settings);
    Ok(())
}

/// Home「启动代理服务」: ensure core is running on the Diagnostic config, then
/// start the configured capture backend (system proxy or TUN, plan §2).
pub(crate) fn start_service(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
    let _orch = lock_orchestrate(state)?;
    let settings = current_settings(&state.paths)?;
    {
        let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
        if core.state().status != CoreStatus::Running {
            let binary = binary_for(app)?;
            let _ = orchestrate_start(
                &state.paths,
                &settings,
                &mut **core,
                binary,
                resource_dir(app).as_deref(),
                CaptureIntent::Diagnostic,
            )?;
            clear_transient_recovery_warnings(state);
        }
    }
    if settings.tun.enabled {
        // TUN path: make sure the Windows scheduled task exists (imported
        // settings / auto-save can persist `tun.enabled` without the one-time
        // UAC setup). No-op on macOS and when the pinned task is already
        // installed. Then the controller stops the app-managed core and the
        // backend starts the elevated one (adopted afterwards).
        ensure_tun_elevation_inner(app, state)?;
        state.capture.refresh_backend()?;
        let binary = binary_for(app)?;
        let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
        let resolved = state
            .capture
            .enable_tun(&settings, &mut **core, binary.clone())?;
        // Persist the resolved interface name only after the transition is
        // healthy (plan §4.3 commit-after-health).
        if resolved
            .as_ref()
            .is_some_and(|name| settings.tun.interface_name.as_deref() != Some(name.as_str()))
        {
            let mut committed = settings.clone();
            committed.tun.interface_name = resolved;
            committed.proxy_service_enabled = true;
            if let Err(commit_err) =
                persist_settings(&state.paths.settings(), &committed, host_platform())
            {
                let rollback = {
                    let proxy = state.proxy.lock().map_err(|_| lock_poisoned("proxy"))?;
                    state.capture.disable_active_backend(
                        &settings,
                        &mut **core,
                        proxy.as_ref(),
                        binary,
                        true,
                    )
                };
                return match rollback {
                    Ok(()) => Err(commit_err),
                    Err(rollback_err) => Err(AppError::new(
                        ErrorCode::TunRecoveryRequired,
                        format!(
                            "TUN interface commit failed ({commit_err}) and rollback was not verified ({rollback_err})"
                        ),
                    )),
                };
            }
        }
        if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
            *slot = None;
        }
    } else {
        // System-proxy branch: exclusivity is enforced by the capture
        // controller (TUN active / preparing / stopping / recovery_required
        // all reject the enable), never inferred from `tun.enabled` alone.
        {
            let core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
            let proxy = state.proxy.lock().map_err(|_| lock_poisoned("proxy"))?;
            state
                .capture
                .enable_system_proxy(&settings, &**core, proxy.as_ref())?;
        }
        if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
            *slot = None;
        }
        if let Ok(mut cache) = state.proxy_applied_cache.lock() {
            *cache = None;
        }
    }
    // Persist after capture is on so a crash/quit still restores next launch.
    set_proxy_service_enabled_for(&state.paths.settings(), true, host_platform())?;
    attach_traffic(state, &settings);
    Ok(())
}

/// App-launch path: reclaim leftover cores, recover crash leftovers, then
/// start the core. Capture is restored after the first UI frame via
/// [`restore_proxy_service_on_launch`].
///
/// Holds the orchestrate lock for the whole sequence so `start` / restore
/// cannot race a still-bound orphan. `orch_acquired`, when set, is signalled
/// as soon as the lock is held so `setup` can return and show the window
/// without waiting for `ps` / helper / `networksetup` work.
pub fn auto_start_on_launch(
    app: &AppHandle,
    state: &AppState,
    orch_acquired: Option<SyncSender<()>>,
) -> Result<(), AppError> {
    // Always unblock `setup` if this worker returns or panics before the
    // handshake send below (otherwise the window never appears).
    struct Notify(Option<SyncSender<()>>);
    impl Drop for Notify {
        fn drop(&mut self) {
            if let Some(tx) = self.0.take() {
                let _ = tx.send(());
            }
        }
    }
    let mut notify = Notify(orch_acquired);
    let _orch = lock_orchestrate(state)?;
    if let Some(tx) = notify.0.take() {
        let _ = tx.send(());
    }
    recover_launch_leftovers(state);
    if state.shutdown_requested.load(Ordering::SeqCst) {
        return Ok(());
    }
    start_core_inner(app, state)
}

/// Reclaim leftover sing-box processes and restore crash leftovers (system
/// proxy backup + TUN journal). Never enables capture. Caller must hold the
/// orchestrate lock so a later start cannot bind while an orphan still owns
/// the ports.
pub(crate) fn recover_launch_leftovers(state: &AppState) {
    {
        let mut core = match state.core.lock() {
            Ok(core) => core,
            Err(_) => return,
        };
        if let Err(err) = core.reclaim_orphan_pid(&state.paths.pid()) {
            tracing::warn!(error = %err, "failed to reclaim orphan sing-box pid");
        }
    }
    // The pid file can be missing while a previous session's core is still
    // running (the app was killed mid-teardown after the pid record was
    // cleared); scan the process table for sing-box processes running this
    // installation's config and reclaim the user-owned ones, so the auto-start
    // never hits `bind: address already in use` with no way to recover.
    let reclaimed = ice_core::reclaim_orphan_cores_with_config(&state.paths.config());
    if reclaimed > 0 {
        tracing::warn!(
            reclaimed,
            "reclaimed orphan sing-box cores without a pid file"
        );
    }

    {
        let proxy = match state.proxy.lock() {
            Ok(proxy) => proxy,
            Err(_) => return,
        };
        let endpoints = current_settings(&state.paths)
            .ok()
            .map(|s| endpoints_from_settings(&s));
        match recover_if_applied_hinted(
            &state.paths.proxy_backup(),
            proxy.as_ref(),
            endpoints.as_ref(),
        ) {
            Ok(outcome) if outcome.restored() => {
                tracing::info!("restored system proxy from previous session");
                if outcome == RecoverOutcome::RestoredFromCorrupt {
                    append_recovery_warning(
                        state,
                        format!(
                            "{}: proxy-backup.json was corrupt; system proxy was reset to defaults",
                            ErrorCode::ProxyBackupCorrupt
                        ),
                    );
                }
            }
            Ok(_) => {
                tracing::debug!("no applied system proxy backup to restore");
            }
            Err(err) => {
                tracing::error!(error = %err, "system proxy crash recovery failed");
                append_recovery_warning(
                    state,
                    format!(
                        "{}: system proxy recovery failed: {err}",
                        ErrorCode::ProxyBackupCorrupt
                    ),
                );
            }
        }
    }
    // Fail-closed exclusivity: when startup proxy recovery failed, the OS
    // proxy is still applied and the app still owns it. Keep the capture
    // controller consistent with disk so TUN activation stays rejected
    // until the proxy is restored.
    if proxy_backup_indicates_ownership(&state.paths.proxy_backup()) {
        tracing::warn!(
            "system proxy backup still records applied after startup recovery; capture controller treats system proxy as the active backend"
        );
        let _ = state.capture.set_system_proxy_active();
    }

    let mut core = match state.core.lock() {
        Ok(core) => core,
        Err(_) => return,
    };
    // A leftover root-owned core from a previous session (the unprivileged
    // process could not signal it) still holds the ports; reclaim it through
    // the elevated coordinator before journal recovery so a later start /
    // re-enable never hits `bind: address already in use`.
    if let Err(err) = state.capture.reclaim_orphan_elevated_core(&mut **core) {
        tracing::warn!(error = %err, "failed to reclaim orphaned elevated core");
        append_recovery_warning(state, format!("残留内核清理未确认 ({err})"));
    }
    match state.capture.recover(&mut **core) {
        Ok(Some(warning)) => append_recovery_warning(state, warning),
        Ok(None) => {}
        Err(err) => {
            tracing::error!(error = %err, "startup tun recovery failed");
            append_recovery_warning(state, format!("TUN state recovery unconfirmed ({err})"));
        }
    }
}

/// After the window's first frame: if the last session left the proxy service
/// on, enable the configured capture backend. Core auto-start may still be in
/// flight; [`start_service`] waits on the orchestrate lock and no-ops the
/// core spawn when it is already Running. StrictMode remounts are ignored.
pub fn restore_proxy_service_on_launch(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
    if !take_launch_proxy_restore(state)? {
        return Ok(());
    }
    match start_service(app, state) {
        Ok(()) => Ok(()),
        Err(err) => {
            if state.shutdown_requested.load(Ordering::SeqCst) {
                return Ok(());
            }
            if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
                *slot = Some(format!("proxy service auto-start failed ({err})"));
            }
            Err(err)
        }
    }
}

/// Whether this process should enable capture for the post-paint restore.
pub(crate) fn take_launch_proxy_restore(state: &AppState) -> Result<bool, AppError> {
    if state.shutdown_requested.load(Ordering::SeqCst) {
        return Ok(false);
    }
    if !current_settings(&state.paths)?.proxy_service_enabled {
        return Ok(false);
    }
    if state.capture.active_backend() != TrafficCapture::Inactive {
        return Ok(false);
    }
    Ok(!state
        .launch_proxy_restore_attempted
        .swap(true, Ordering::SeqCst))
}

/// Home「启动代理服务」: ensure core is running, then take over the OS system proxy.
#[tauri::command]
pub async fn start(app: AppHandle) -> Result<(), AppError> {
    run_blocking("start", move || {
        let state = app.state::<AppState>();
        start_service(&app, &state)
    })
    .await
}

/// First-frame restore of the last proxy-service state. No-op when the last
/// session left capture off, or when capture is already active.
#[tauri::command]
pub async fn restore_launch_proxy(app: AppHandle) -> Result<(), AppError> {
    run_blocking("restore_launch_proxy", move || {
        let state = app.state::<AppState>();
        restore_proxy_service_on_launch(&app, &state)
    })
    .await
}

/// Home「停止代理服务」: disable whichever capture backend is active. The IPC
/// name is retained for compatibility (plan §4.3); it delegates to the
/// controller, so it restores the OS proxy for the system-proxy backend and
/// releases TUN capture (core may stay Running on the Diagnostic config).
pub(crate) fn disable_active_backend_inner(
    app: &AppHandle,
    state: &AppState,
) -> Result<(), AppError> {
    let _orch = lock_orchestrate(state)?;
    let settings = current_settings(&state.paths)?;
    let binary = binary_for(app)?;
    let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
    let proxy = state.proxy.lock().map_err(|_| lock_poisoned("proxy"))?;
    state
        .capture
        .disable_active_backend(&settings, &mut **core, proxy.as_ref(), binary, true)?;
    set_proxy_service_enabled_for(&state.paths.settings(), false, host_platform())?;
    if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
        *slot = None;
    }
    if let Ok(mut cache) = state.proxy_applied_cache.lock() {
        *cache = None;
    }
    Ok(())
}

#[tauri::command]
pub async fn stop(app: AppHandle) -> Result<(), AppError> {
    run_blocking("stop", move || {
        let state = app.state::<AppState>();
        let binary = binary_for(&app)?;
        graceful_stop(&state, binary)
    })
    .await
}
