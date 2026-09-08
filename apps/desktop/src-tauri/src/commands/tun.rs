// SPDX-License-Identifier: GPL-3.0-or-later

use super::core::disable_active_backend_inner;
use super::*;

/// Home「重试恢复」: on-demand retry of TUN recovery (plan §4.3 / §4.4). Runs
/// the journal recovery driver under the orchestration lock; never enables
/// capture. Returns a warning message when cleanup is still uncertain.
#[tauri::command]
pub async fn recover_tun(app: AppHandle) -> Result<Option<String>, AppError> {
    run_blocking("recover_tun", move || {
        let state = app.state::<AppState>();
        let _orch = lock_orchestrate(&state)?;
        state.capture.refresh_backend()?;
        let mut core = state.core.lock().map_err(|_| lock_poisoned("core"))?;
        let warning = state.capture.recover(&mut **core)?;
        if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
            *slot = warning.clone();
        }
        Ok(warning)
    })
    .await
}

#[tauri::command]
pub async fn stop_system_proxy(app: AppHandle) -> Result<(), AppError> {
    run_blocking("stop_system_proxy", move || {
        let state = app.state::<AppState>();
        disable_active_backend_inner(&app, &state)
    })
    .await
}

/// Install and authorize the trusted helper component (TUN elevation path):
/// prompts with the system authorization dialog and runs the bundled
/// `ice-helper install` as root. macOS only; cancelling modifies nothing.
/// On success the backend is refreshed so the fresh helper is usable at once.
#[tauri::command]
pub async fn install_helper(app: AppHandle) -> Result<(), AppError> {
    run_blocking("install_helper", move || {
        let result = crate::helper_install::install_helper_inner(&app);
        let state = app.state::<AppState>();
        reset_helper_probe_cache(&state);
        result
    })
    .await
}

/// Uninstall the trusted helper component: prompts with the system
/// authorization dialog, runs `ice-helper uninstall` as root, and refreshes
/// the backend (back to fail-closed).
#[tauri::command]
pub async fn uninstall_helper(app: AppHandle) -> Result<(), AppError> {
    run_blocking("uninstall_helper", move || {
        let result = crate::helper_install::uninstall_helper_inner(&app);
        let state = app.state::<AppState>();
        reset_helper_probe_cache(&state);
        result
    })
    .await
}

/// One-time Windows TUN elevation setup (plan B): installs the scheduled task
/// that runs the TUN core elevated. The only elevation the user ever sees —
/// a single UAC prompt to create the task; afterwards TUN transitions
/// (`schtasks /Run` / `/End`) work from an unelevated app without prompts.
/// No-op when the task already exists; always `Ok` on non-Windows hosts.
#[tauri::command]
pub async fn ensure_tun_elevation(app: AppHandle) -> Result<(), AppError> {
    run_blocking("ensure_tun_elevation", move || {
        let state = app.state::<AppState>();
        ensure_tun_elevation_inner(&app, &state)
    })
    .await
}

/// Remove the Windows TUN scheduled task (one UAC prompt). No-op when the
/// task does not exist; always `Ok` on non-Windows hosts.
#[tauri::command]
pub async fn remove_tun_elevation(app: AppHandle) -> Result<(), AppError> {
    run_blocking("remove_tun_elevation", move || {
        let state = app.state::<AppState>();
        remove_tun_elevation_inner(&app, &state)
    })
    .await
}

#[cfg(target_os = "windows")]
pub(crate) fn tun_task_paths(
    app: &AppHandle,
    state: &AppState,
) -> Result<(PathBuf, PathBuf), AppError> {
    let resource = resource_dir(app)
        .ok_or_else(|| launcher_failed("cannot resolve the bundled resources directory"))?;
    let launcher = resource.join("ice-tun-launcher.exe");
    let data_dir = state.paths.root().to_path_buf();
    Ok((launcher, data_dir))
}

#[cfg(target_os = "windows")]
pub(crate) fn run_elevated_launcher(launcher: &Path, args: &[String]) -> Result<(), AppError> {
    // UAC launches the GUI-subsystem ice-tun-launcher (no console). That
    // process then runs schtasks with CREATE_NO_WINDOW. Elevating cmd.exe
    // or schtasks.exe directly always flashes a black console because
    // ShellExecute cannot pass CREATE_NO_WINDOW.
    match ice_tun_sys::run_elevated_wait(launcher, args) {
        Ok(0) => Ok(()),
        Ok(code) => Err(launcher_failed(format!(
            "the TUN launcher exited with status {code}"
        ))),
        Err(err) if err.raw_os_error() == Some(1223) => Err(AppError::with_code(
            crate::windows_elevation::ERR_ELEVATION_CANCELLED,
            "the one-time TUN elevation setup was not granted; enable TUN again to retry",
        )),
        Err(err) => Err(launcher_failed(format!(
            "spawn elevated TUN launcher: {err}"
        ))),
    }
}

#[cfg(target_os = "windows")]
fn launcher_failed(message: impl Into<String>) -> AppError {
    AppError::with_code(ErrorCode::TunHelperInstallFailed, message)
}

#[cfg(target_os = "windows")]
pub(crate) fn ensure_tun_elevation_inner(
    app: &AppHandle,
    state: &AppState,
) -> Result<(), AppError> {
    let (launcher, data_dir) = tun_task_paths(app, state)?;
    if !launcher.is_file() {
        return Err(launcher_failed(format!(
            "TUN task launcher not found at {} (reinstall the app)",
            launcher.display()
        )));
    }
    let core = launcher
        .parent()
        .map(|dir| dir.join("sing-box.exe"))
        .ok_or_else(|| {
            launcher_failed(format!(
                "TUN task launcher path {} has no parent",
                launcher.display()
            ))
        })?;
    if !core.is_file() {
        return Err(launcher_failed(format!(
            "TUN core binary not found at {} (reinstall the app)",
            core.display()
        )));
    }
    // Recreate when the task is missing *or* the stored pin / Command no
    // longer matches the protected copies (app update, or a replaced binary).
    if ice_tun_sys::tun_task_exists() && ice_tun_sys::tun_task_pin_matches(&launcher) {
        return Ok(());
    }
    let install_args = vec![
        "--install".to_string(),
        "--data".to_string(),
        data_dir.to_string_lossy().into_owned(),
    ];
    let create_result = if ice_tun_sys::process_is_elevated() {
        use std::os::windows::process::CommandExt;
        let status = std::process::Command::new(&launcher)
            .args(&install_args)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .status()
            .map_err(|err| launcher_failed(format!("create the TUN scheduled task: {err}")));
        match status {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(launcher_failed(format!(
                "the TUN scheduled task could not be created (exit {})",
                status.code().unwrap_or(-1)
            ))),
            Err(err) => Err(err),
        }
    } else {
        run_elevated_launcher(&launcher, &install_args)
    };
    let pin_ok = ice_tun_sys::tun_task_exists() && ice_tun_sys::tun_task_pin_matches(&launcher);
    create_result?;
    if !pin_ok {
        tracing::warn!("TUN scheduled task missing or pin not persisted after the setup run");
        return Err(launcher_failed(
            "the TUN scheduled task pin was not stored; enable TUN again to retry",
        ));
    }
    reset_tun_task_cache(state);
    tracing::info!("TUN scheduled task installed (one-time elevation complete)");
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn ensure_tun_elevation_inner(
    _app: &AppHandle,
    _state: &AppState,
) -> Result<(), AppError> {
    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn remove_tun_elevation_inner(
    app: &AppHandle,
    state: &AppState,
) -> Result<(), AppError> {
    let (launcher, _) = tun_task_paths(app, state)?;
    if !ice_tun_sys::tun_task_exists() && !launcher.is_file() {
        return Ok(());
    }
    if !launcher.is_file() {
        return Err(launcher_failed(format!(
            "TUN task exists but the launcher is missing at {} (reinstall the app)",
            launcher.display()
        )));
    }
    let args = vec!["--delete-task".to_string()];
    if ice_tun_sys::process_is_elevated() {
        use std::os::windows::process::CommandExt;
        let status = std::process::Command::new(&launcher)
            .args(&args)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .status()
            .map_err(|err| launcher_failed(format!("delete the TUN scheduled task: {err}")))?;
        if !status.success() {
            return Err(launcher_failed(
                "the TUN scheduled task could not be deleted",
            ));
        }
    } else {
        run_elevated_launcher(&launcher, &args)?;
    }
    reset_tun_task_cache(state);
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn remove_tun_elevation_inner(
    _app: &AppHandle,
    _state: &AppState,
) -> Result<(), AppError> {
    Ok(())
}
