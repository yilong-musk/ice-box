// SPDX-License-Identifier: GPL-3.0-or-later

//! TUN capture platform boundary (`docs/tun.md`).
//!
//! `ice-tun-sys` owns the TUN mutation journal, the platform backend
//! contract, and the startup/watchdog recovery driver. It performs no
//! OS mutation itself: platform backends do, recording every journaled
//! mutation boundary. System-proxy backup data is never reused for TUN
//! state (see `ice-proxy-sys`).
//!
//! Host-free tests cover the journal, backend contract, fake backend, and
//! recovery idempotence. macOS uses the native backend with a privileged
//! helper (or a dev-only `sudo` runner). Other platforms get a fail-closed
//! backend until their gate is green. Without a helper or sudo opt-in,
//! `DeferredCoreCoordinator` fails cleanly with `tun.permission_required`.

use std::path::PathBuf;

pub mod backend;
pub mod coordinator;
pub mod error;
pub mod fake;
#[cfg(unix)]
pub mod helper;
pub use ice_tun_helper_proto as helper_protocol;
pub use ice_tun_helper_proto::install_paths;
pub use ice_tun_journal as journal;
pub mod macos;
pub mod recovery;
pub mod routes;
pub mod unsupported;
pub mod windows;

pub use backend::{
    unsupported_capability, AppliedTun, PreparedTun, RecoveryOutcome, TunBackend, TunCapability,
    TunConfig, TunHealth, TunStack,
};
#[cfg(target_os = "windows")]
pub use coordinator::{
    current_user_is_local_admin, current_user_sid_string, process_is_elevated, run_elevated_wait,
    signal_tun_stop_event,
};
pub use coordinator::{
    quote_windows_args, schtasks_command_line, tun_task_exists, tun_task_has_pin,
    tun_task_pin_matches, tun_task_xml_create_args, write_tun_task_xml, CoreCoordinator,
    DeferredCoreCoordinator, SudoCoreCoordinator, TUN_TASK_NAME,
};
pub use error::{ErrorCode, TunError};
pub use ice_tun_pin::{
    format_tun_task_pin, program_data_dir, program_files_dir, protected_bin_dir,
    protected_core_log_path, protected_install_error_path, protected_pidfile_path,
    read_last_tun_install_error, sha256_of_file, TUN_STOP_EVENT_NAME,
};
pub use journal::{steps, CidrRecord, DnsSnapshot, JournalState, RouteRecord, TunJournal};
pub use macos::{
    is_tunnel_interface, outbound_interface_is_safe, pin_outbound_interface, utun_index,
    MacInterfaceState, MacOsHost, MacosTunBackend, ProcessMacOsHost,
};
pub use recovery::RecoveryDriver;
pub use unsupported::UnsupportedTunBackend;
pub use windows::{
    ProcessWindowsHost, WindowsHost, WindowsInterfaceState, WindowsTunBackend, DEFAULT_WINTUN_NAME,
};

/// Create the platform backend selected for this host (`docs/tun.md`).
///
/// macOS (gate green) gets the native-path backend; every other platform
/// gets a fail-closed backend whose capability reports `supported=false`
/// with a stable reason, so a TUN transition is never attempted there.
///
/// `config_path` is the runtime `config.json` the injected core coordinator
/// starts; `owner_token` identifies this installation in the journal.
/// `binary` / `log_path` feed the dev `sudo` runner (macOS live gate):
/// `binary` is `None` when the bundled sing-box could not be resolved,
/// which keeps the fail-closed deferred runner in place.
///
/// Coordinator selection on macOS (`docs/tun.md`): the explicit
/// `ICE_BOX_TUN_DEV_SUDO` opt-in wins (live gate), otherwise the privileged
/// helper is used when it is installed and authorized (probed
/// read-only via a `Status` frame at construction), otherwise the fail-closed
/// `DeferredCoreCoordinator` keeps every transition at
/// `tun.permission_required` with no OS mutation.
pub fn create_backend(
    owner_token: &str,
    config_path: PathBuf,
    binary: Option<PathBuf>,
    log_path: PathBuf,
) -> Box<dyn TunBackend + Send> {
    #[cfg(target_os = "macos")]
    {
        let coordinator: Box<dyn CoreCoordinator + Send> = if dev_sudo_runner_enabled() {
            match binary {
                Some(binary) => Box::new(SudoCoreCoordinator::new(binary, log_path)),
                None => {
                    tracing::warn!(
                        "ICE_BOX_TUN_DEV_SUDO is set but no sing-box binary was resolved; TUN transitions stay fail-closed"
                    );
                    Box::new(DeferredCoreCoordinator)
                }
            }
        } else if let Some(helper) = helper_coordinator(&config_path) {
            helper
        } else {
            Box::new(DeferredCoreCoordinator)
        };
        Box::new(MacosTunBackend::new(
            owner_token,
            Box::new(ProcessMacOsHost),
            coordinator,
            config_path,
        ))
    }
    #[cfg(target_os = "windows")]
    {
        // windows_tun_ready flipped 2026-09-03 (`docs/tun.md`): the
        // production path is the real backend. The elevated runner needs a
        // bundled binary; without one every transition stays fail-closed at
        // `tun.permission_required` (no OS mutation). The scheduled-task
        // runner (plan B) is preferred when the TUN task exists and the
        // bundled launcher is present — the app then never needs elevation;
        // otherwise the UAC relaunch runner stays as the fallback (task
        // missing/disabled, dev runs).
        let coordinator: Box<dyn CoreCoordinator + Send> = match binary {
            Some(binary) => {
                let task_coordinator = (|| {
                    let launcher = binary.parent()?.join("ice-tun-launcher.exe");
                    if !launcher.is_file() {
                        return None;
                    }
                    let pidfile =
                        ice_tun_pin::protected_pidfile_path(&ice_tun_pin::program_data_dir());
                    let coordinator =
                        crate::coordinator::TaskCoreCoordinator::new(launcher, pidfile);
                    if crate::coordinator::tun_task_exists() {
                        Some(coordinator)
                    } else {
                        None
                    }
                })();
                match task_coordinator {
                    Some(coordinator) => Box::new(coordinator),
                    None => Box::new(crate::coordinator::WindowsElevatedCoreCoordinator::new(
                        binary, log_path,
                    )),
                }
            }
            None => {
                tracing::warn!(
                    "no sing-box binary was resolved; Windows TUN transitions stay fail-closed"
                );
                Box::new(DeferredCoreCoordinator)
            }
        };
        Box::new(WindowsTunBackend::new(
            owner_token,
            Box::new(ProcessWindowsHost),
            coordinator,
            config_path,
        ))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // The parameters feed only the platform backends; drop them here so
        // the platform gate branch stays warn-free on every other host.
        let _ = (owner_token, config_path, binary, log_path);
        let reason = "tun.unsupportedPlatform";
        Box::new(UnsupportedTunBackend::new(reason))
    }
}

/// Elevated core stdout/stderr: macOS helper log, or the Windows
/// ProgramData run-dir log. `None` on platforms with no privileged core log.
pub fn elevated_core_log_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        Some(ice_tun_pin::protected_core_log_path(
            &ice_tun_pin::program_data_dir(),
        ))
    }
    #[cfg(target_os = "macos")]
    {
        Some(PathBuf::from(install_paths::CORE_LOG_DEST))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// Try to build the privileged-helper coordinator for `config_path`'s data
/// dir. Returns `None` (fail-closed, no OS mutation) when the helper is not
/// installed, not authorized, or unreachable. The probe is read-only: one
/// `Status` frame over a bounded-timeout connection.
#[cfg(target_os = "macos")]
fn helper_coordinator(config_path: &std::path::Path) -> Option<Box<dyn CoreCoordinator + Send>> {
    let data_dir = config_path.parent()?;
    let socket = crate::helper::helper_socket_path();
    let token = crate::helper::helper_token(data_dir).ok()?;
    // Bounded probe: a dead-but-present daemon (e.g. mid-Stop) must never
    // stall app startup, backend refresh, or the Home start path for the full
    // IPC timeout.
    if crate::helper::helper_reachable_bounded(&socket, &token) {
        tracing::info!(socket = %socket.display(), "privileged helper authorized; using it for elevated core runs");
        Some(Box::new(crate::helper::HelperCoreCoordinator::new(
            socket,
            token,
            data_dir.to_path_buf(),
        )))
    } else {
        tracing::warn!(
            socket = %socket.display(),
            "privileged helper not reachable; TUN transitions fail closed with tun.permission_required"
        );
        None
    }
}

/// Dev-only opt-in for the `sudo` runner (macOS live tests).
/// Set `ICE_BOX_TUN_DEV_SUDO=1` to run the macOS native TUN path with
/// a cached root credential instead of the installed privileged helper.
/// Anything else (unset, empty, `0`) keeps the fail-closed deferred runner:
/// no OS mutation happens without an explicit opt-in.
pub fn dev_sudo_runner_enabled() -> bool {
    std::env::var("ICE_BOX_TUN_DEV_SUDO")
        .map(|value| !value.is_empty() && value != "0")
        .unwrap_or(false)
}
