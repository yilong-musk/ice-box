//! In-app UAC relaunch for TUN elevation (Windows only).
//!
//! Windows TUN transitions must run in an elevated context: the TUN core
//! creates the wintun adapter and owns routes/DNS, and the elevated runner
//! (`WindowsElevatedCoreCoordinator`) rejects unelevated processes with
//! `tun.permission_required`. There is no installable helper on Windows —
//! the privileged daemon model is macOS-only (design note ice-helper-design).
//! Instead the app uses the standard Windows pattern: relaunch itself via
//! the `runas` verb (UAC prompt) and let the elevated successor apply the
//! already-persisted `tun.enabled` setting on startup.
//!
//! Flow (`relaunch_elevated_for_tun`, driven by the TUN toggle):
//!   1. the frontend persists `tun.enabled = true`,
//!   2. `relaunch_elevated` spawns `powershell Start-Process -Verb RunAs`
//!      for the current executable with `--elevated-relaunch`,
//!   3. on approval the app schedules a graceful quit (`app.exit(0)`;
//!      `ExitRequested` stops the app-managed core and restores the system
//!      proxy before the process dies),
//!   4. the elevated successor starts, waits out the predecessor's instance
//!      lock (`instance::RELAUNCH_FLAG`), and applies TUN on startup.
//!
//! When the user cancels the UAC prompt the command fails with
//! `tun.elevation_cancelled` and nothing was modified; the frontend reverts
//! the persisted TUN setting. Already-elevated processes are a no-op
//! (`Ok(false)`), as are non-Windows hosts.

use ice_config::AppError;
#[cfg(target_os = "windows")]
use std::process::Command;
#[cfg(target_os = "windows")]
use tauri::Manager;

/// Stable error code when the UAC prompt was cancelled (or the relaunch
/// could not be started). Nothing was modified.
#[cfg(target_os = "windows")]
pub const ERR_ELEVATION_CANCELLED: &str = "tun.elevation_cancelled";

/// How long the old instance stays alive after a successful spawn so the
/// command response reaches the frontend before the process exits.
#[cfg(target_os = "windows")]
const EXIT_DELAY_MS: u64 = 1200;

/// Relaunch the current executable elevated via UAC. `Ok(true)` means the
/// relaunch was launched and this process will exit shortly (the frontend
/// shows a "restarting elevated…" state and does not touch the TUN setting
/// any further). `Ok(false)` means no relaunch was needed (already elevated,
/// or not Windows). `Err` means the user cancelled the prompt or the
/// relaunch failed — nothing was modified.
pub fn relaunch_elevated(app: &tauri::AppHandle) -> Result<bool, AppError> {
    #[cfg(target_os = "windows")]
    {
        if ice_tun_sys::process_is_elevated() {
            return Ok(false);
        }
        let exe = std::env::current_exe().map_err(|err| {
            AppError::with_code(
                ERR_ELEVATION_CANCELLED,
                format!("resolve current executable: {err}"),
            )
        })?;
        let exe_quoted = exe.to_string_lossy().replace('\'', "''");
        let script = format!(
            "try {{ Start-Process -FilePath '{exe_quoted}' -ArgumentList '{flag}' -Verb RunAs -ErrorAction Stop | Out-Null; exit 0 }} catch {{ exit 1223 }}",
            flag = crate::instance::RELAUNCH_FLAG
        );
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .map_err(|err| {
                AppError::with_code(
                    ERR_ELEVATION_CANCELLED,
                    format!("spawn UAC relaunch: {err}"),
                )
            })?;
        if !output.status.success() {
            return Err(AppError::with_code(
                ERR_ELEVATION_CANCELLED,
                "administrator authorization was not granted; TUN stays off — enable TUN again to retry, or start ice-box as administrator",
            ));
        }
        // Approved: the elevated successor is on its way. Give the response a
        // moment to reach the frontend, then quit through the normal cleanup
        // path (stop the app-managed core, restore the system proxy).
        let handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(EXIT_DELAY_MS));
            handle.exit(0);
        });
        Ok(true)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Ok(false)
    }
}
