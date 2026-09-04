//! TUN capture platform boundary (plan §4.5, T0 slice).
//!
//! `ice-tun-sys` owns the TUN mutation journal, the platform backend
//! contract, and the startup/watchdog recovery driver. It performs no
//! OS mutation itself: platform backends do, recording every journaled
//! mutation boundary. System-proxy backup data is never reused for TUN
//! state (see `ice-proxy-sys`).
//!
//! T0 shipped the host-free core (journal + contract + fake backend +
//! recovery) and the fault-injection tests that prove recovery is
//! idempotent. T2 adds the macOS backend (native sing-box ownership,
//! `MacosTunBackend`), the fail-closed `UnsupportedTunBackend` for
//! platforms whose gate is pending, the shared auto-route model, and
//! the `CoreCoordinator` boundary for the elevated core. T3 wires the
//! dev `sudo` runner (`SudoCoreCoordinator`, opt-in via
//! `ICE_BOX_TUN_DEV_SUDO`) so the macOS live gate runs on a real host;
//! without the opt-in `DeferredCoreCoordinator` fails cleanly with
//! `tun.permission_required`, and the production helper remains
//! slice T5.

use std::path::PathBuf;

pub mod backend;
pub mod coordinator;
pub mod error;
pub mod fake;
#[cfg(unix)]
pub mod helper;
pub mod helper_protocol;
pub mod install_paths;
pub mod journal;
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
pub use coordinator::{process_is_elevated, tun_task_create_args, tun_task_create_command};
pub use coordinator::{
    tun_task_exists, CoreCoordinator, DeferredCoreCoordinator, SudoCoreCoordinator, TUN_TASK_NAME,
};
pub use error::{TunError, TunErrorCode};
pub use journal::{steps, CidrRecord, DnsSnapshot, JournalState, RouteRecord, TunJournal};
pub use macos::{utun_index, MacInterfaceState, MacOsHost, MacosTunBackend, ProcessMacOsHost};
pub use recovery::RecoveryDriver;
pub use unsupported::UnsupportedTunBackend;
pub use windows::{
    ProcessWindowsHost, WindowsHost, WindowsInterfaceState, WindowsTunBackend, DEFAULT_WINTUN_NAME,
};

/// Create the platform backend selected for this host (plan §3.2 / §5 T2).
///
/// macOS (gate green) gets the native-path backend; every other platform
/// gets a fail-closed backend whose capability reports `supported=false`
/// with a stable reason, so a TUN transition is never attempted there.
///
/// `config_path` is the runtime `config.json` the injected core coordinator
/// starts; `owner_token` identifies this installation in the journal.
/// `binary` / `log_path` feed the dev `sudo` runner (macOS live gate,
/// plan §5 T3): `binary` is `None` when the bundled sing-box could not be
/// resolved, which keeps the fail-closed deferred runner in place.
///
/// Coordinator selection on macOS (T5 production path): the explicit
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
        // windows_tun_ready flipped 2026-09-03 (design note §1.2): the
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
                    let pidfile = config_path.parent()?.join("tun-task.pid");
                    let stopfile = config_path.parent()?.join("tun-task.stop");
                    let coordinator =
                        crate::coordinator::TaskCoreCoordinator::new(launcher, pidfile, stopfile);
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
        let reason = "TUN is supported on macOS and Windows only in the first release";
        Box::new(UnsupportedTunBackend::new(reason))
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

/// Dev-only opt-in for the `sudo` runner (plan §5 T3 exit gate, macOS live
/// gate). Set `ICE_BOX_TUN_DEV_SUDO=1` to run the macOS native TUN path with
/// a cached root credential instead of the installed privileged helper (T5).
/// Anything else (unset, empty, `0`) keeps the fail-closed deferred runner:
/// no OS mutation happens without an explicit opt-in.
pub fn dev_sudo_runner_enabled() -> bool {
    std::env::var("ICE_BOX_TUN_DEV_SUDO")
        .map(|value| !value.is_empty() && value != "0")
        .unwrap_or(false)
}
