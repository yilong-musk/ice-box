// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Result<AppSettings, AppError> {
    run_blocking("get_settings", move || {
        let state = app.state::<AppState>();
        current_settings(&state.paths)
    })
    .await
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, patch: SettingsPatch) -> Result<(), AppError> {
    // A TUN transition can take seconds (system-proxy restore via
    // `networksetup` + elevated core restart + readiness waits); a sync
    // command would block the main thread and freeze the UI event loop.
    run_blocking("save_settings", move || {
        let state = app.state::<AppState>();
        let _orch = lock_orchestrate(&state)?;
        let previous = current_settings(&state.paths)?;
        // Home start/stop own `proxy_service_enabled`; Settings omits it so a
        // form save cannot clobber on/off. An explicit patch field exists for
        // dedicated callers.
        let settings = previous.apply_patch(&patch);
        settings.validate_for(host_platform())?;
        let active = state.capture.active_backend();
        // `tun.enabled` is next-start desire only: persist it without starting,
        // stopping, or switching the live capture backend.
        if only_tun_enabled_changed(&previous, &settings) {
            persist_settings(&state.paths.settings(), &settings, host_platform())?;
            return Ok(());
        }
        if crate::app_update::only_check_app_updates_changed(&previous, &settings) {
            persist_settings(&state.paths.settings(), &settings, host_platform())?;
            return Ok(());
        }
        if only_log_debug_changed(&previous, &settings) {
            persist_settings(&state.paths.settings(), &settings, host_platform())?;
            return Ok(());
        }
        if only_tray_display_mode_changed(&previous, &settings) {
            persist_settings(&state.paths.settings(), &settings, host_platform())?;
            return Ok(());
        }
        // Live TUN topology reconfigure (addresses / MTU / stack / …) while
        // TUN capture stays the active backend. Enabled flips never belong
        // here — they were handled above.
        let tun_transition = active == TrafficCapture::Tun
            && previous.tun.enabled
            && settings.tun.enabled
            && tun_topology_changed(&previous.tun, &settings.tun);
        if tun_transition {
            // Serialized topology apply (`docs/tun.md`): the pending record is
            // committed only after the requested backend is healthy.
            let binary = binary_for(&app)?;
            let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
            let proxy = state.proxy.lock().map_err(|_| lock_poisoned("proxy"))?;
            state.capture.transition_tun_settings(
                &previous,
                &settings,
                &mut **core,
                proxy.as_ref(),
                binary,
            )?;
            if state.capture.active_backend() == TrafficCapture::Tun {
                // The transition re-enabled TUN from the full candidate, so the
                // runtime config already reflects the change (incl. ports/mode);
                // a second apply would tear down and re-create the TUN for
                // nothing. Re-target the traffic stream (endpoints may have
                // changed) — the mutation paths are the only re-attach points
                // since the 1s snapshot poll no longer reads settings.
                attach_traffic(&state, &settings)?;
                Ok(())
            } else {
                // TUN was disabled: the disable path restarted the app-managed
                // core on the previous config; apply the non-backend parts of
                // the change (ports, mode, rules) now.
                apply_after_change(&app, &state, &settings, &previous)
            }
        } else {
            persist_settings(&state.paths.settings(), &settings, host_platform())?;
            apply_after_change(&app, &state, &settings, &previous)
        }
    })
    .await
}

#[tauri::command]
pub fn set_tray_language(app: AppHandle, language: TrayLanguage) -> Result<(), AppError> {
    tray::set_language(&app, language)
}

#[derive(Deserialize)]
pub struct SetProxyModeRequest {
    /// `"rule"` | `"global"` | `"direct"`.
    pub mode: String,
}

/// Persist-only: log debug is a display filter, not a runtime-config knob.
fn only_log_debug_changed(previous: &AppSettings, next: &AppSettings) -> bool {
    let mut left = previous.clone();
    let mut right = next.clone();
    left.log_debug = false;
    right.log_debug = false;
    left == right && previous.log_debug != next.log_debug
}

/// Persist-only: the menu-bar item is drawn by the tray watchdog, which reads
/// `settings.json` every second, so this needs no runtime-config apply.
pub(crate) fn only_tray_display_mode_changed(previous: &AppSettings, next: &AppSettings) -> bool {
    let mut left = previous.clone();
    let mut right = next.clone();
    left.tray_display_mode = TrayDisplayMode::IconAndSpeed;
    right.tray_display_mode = TrayDisplayMode::IconAndSpeed;
    left == right && previous.tray_display_mode != next.tray_display_mode
}

pub(crate) fn parse_proxy_mode(mode: &str) -> Result<ProxyMode, AppError> {
    match mode {
        "rule" => Ok(ProxyMode::Rule),
        "global" => Ok(ProxyMode::Global),
        "direct" => Ok(ProxyMode::Direct),
        other => Err(AppError::new(
            ErrorCode::ConfigInvalid,
            format!("unknown proxy mode: {other}"),
        )),
    }
}

pub(crate) fn set_proxy_mode_inner(
    app: &AppHandle,
    state: &AppState,
    req: SetProxyModeRequest,
) -> Result<(), AppError> {
    let mode = parse_proxy_mode(&req.mode)?;
    apply_proxy_mode(app, state, mode)
}

/// Persist + apply a routing mode. Shared by the IPC command and the tray menu
/// so both paths run the same lock/rollback sequence.
pub(crate) fn apply_proxy_mode(
    app: &AppHandle,
    state: &AppState,
    mode: ProxyMode,
) -> Result<(), AppError> {
    let _orch = lock_orchestrate(state)?;
    let previous = current_settings(&state.paths)?;
    if previous.proxy_mode == mode {
        return Ok(());
    }
    let mut settings = previous.clone();
    settings.proxy_mode = mode;
    persist_settings(&state.paths.settings(), &settings, host_platform())?;
    let binary = binary_for(app)?;
    let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
    let proxy = state.proxy.lock().map_err(|_| lock_poisoned("proxy"))?;
    let tun_active = state.capture.active_backend() == TrafficCapture::Tun;
    let mut live_mode_ok = *state
        .clash_live_mode_cache
        .lock()
        .map_err(|_| lock_poisoned("clash_live_mode_cache"))?;
    let result = orchestrate_set_proxy_mode_with_apply(
        &state.paths,
        &settings,
        &previous,
        &mut **core,
        proxy.as_ref(),
        binary,
        resource_dir(app).as_deref(),
        state.capture.apply_intent(),
        &mut live_mode_ok,
        |paths, settings, previous, core, proxy, binary, resource_dir, intent| {
            if tun_active {
                // The rebuild + reload fallback cannot signal the elevated
                // core; route it through the capture controller so TUN health
                // is re-verified and failures fall back to disable/re-apply.
                state
                    .capture
                    .apply_while_tun_active(settings, previous, core, proxy, binary)
            } else {
                orchestrate_apply_with_cache(
                    paths,
                    settings,
                    previous,
                    core,
                    proxy,
                    binary,
                    resource_dir,
                    intent,
                    Some(state.profile_parse_cache.as_ref()),
                )
            }
        },
    );
    // Remember the probe outcome (one PATCH attempt per process; a core that
    // honors it keeps the fast path, one that ignores it skips future probes).
    if let Ok(mut cache) = state.clash_live_mode_cache.lock() {
        *cache = live_mode_ok;
    }
    drop(proxy);
    drop(core);
    drop(_orch);
    // The mode is persisted before the apply, so the window must re-read even
    // when the apply failed (the Home selector refreshes on failure too).
    broadcast_state_change(app);
    result
}

/// Switch routing mode. With the pinned sing-box 1.13.19 the runtime Clash `mode-list` is
/// only `[<default_mode>]`, so a `PATCH /configs` to another mode is silently ignored and
/// the switch always takes the rebuild + reload/restart path (the PATCH attempt is a
/// forward-compatible capability gate). Settings are always persisted so the next apply
/// builds the new `default_mode`.
#[tauri::command]
pub async fn set_proxy_mode(app: AppHandle, req: SetProxyModeRequest) -> Result<(), AppError> {
    run_blocking("set_proxy_mode", move || {
        let state = app.state::<AppState>();
        set_proxy_mode_inner(&app, &state, req)
    })
    .await
}

/// Apply after a settings / subscription mutation. `generate_config` falls back
/// to a direct-only config when no subscription exists, so this always writes a
/// valid config.json (and reloads while Running). The capture intent comes
/// from the runtime controller, never from `tun.enabled` alone.
///
/// While TUN capture is active the apply is routed through the capture
/// controller: the elevated core cannot be signalled by the app, so the
/// controller runs the stop/reconfigure/start sequence and re-verifies TUN
/// health instead of a plain `core.reload()`.
pub(crate) fn apply_after_change(
    app: &AppHandle,
    state: &AppState,
    settings: &AppSettings,
    previous_settings: &AppSettings,
) -> Result<(), AppError> {
    let binary = binary_for(app)?;
    let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
    let proxy = state.proxy.lock().map_err(|_| lock_poisoned("proxy"))?;

    if state.capture.active_backend() == TrafficCapture::Tun {
        state.capture.apply_while_tun_active(
            settings,
            previous_settings,
            &mut **core,
            proxy.as_ref(),
            binary,
        )?;
    } else {
        orchestrate_apply_with_cache(
            &state.paths,
            settings,
            previous_settings,
            &mut **core,
            proxy.as_ref(),
            binary,
            resource_dir(app).as_deref(),
            state.capture.apply_intent(),
            Some(state.profile_parse_cache.as_ref()),
        )?;
    }
    let running = core.state().status == CoreStatus::Running;
    drop(proxy);
    drop(core);
    if running {
        attach_traffic(state, settings)?;
    } else {
        detach_traffic(state);
    }
    Ok(())
}

pub(crate) fn apply_after_subscription_change(
    app: &AppHandle,
    state: &AppState,
    settings: &AppSettings,
) -> Option<AppError> {
    match apply_after_change(app, state, settings, settings) {
        Ok(()) => None,
        Err(err) => {
            tracing::warn!(code = %err.code, error = %err.message, "apply after subscription change failed");
            Some(err)
        }
    }
}

pub(crate) fn attach_apply_warning(value: &mut serde_json::Value, warning: Option<AppError>) {
    if let Some(w) = warning {
        value["apply_warning"] = serde_json::json!({
            "code": w.code,
            "message": w.message,
        });
    }
}
