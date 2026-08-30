//! In-app privileged helper install / uninstall (unsigned elevation path).
//!
//! The app never signs or notarizes (documented product decision), so helper
//! installation cannot use SMAppService. Instead the user is prompted with
//! the system authorization dialog (`ice-elevate`), which runs the bundled
//! `ice-helper install <data-dir> <core-src> <allowed-uid>` as root. The
//! desktop process itself stays unelevated; only the narrow installer mode
//! of the helper runs with root (see `crates/ice-helper/src/install.rs`).
//!
//! On success the capture controller's backend is refreshed so the newly
//! installed helper becomes the active elevated-core coordinator; the user
//! can then retry the TUN transition (the permission error clears on the
//! next status poll).
//!
//! The elevated `install` mode restarts the launchd daemon (bootout +
//! bootstrap), which would orphan a running elevated core. Install/uninstall
//! are therefore refused while TUN capture is active (fail-closed); the UI
//! disables the buttons in that state and the Rust guard is defense-in-depth.
//!
//! Failure modes are fail-closed: cancellation leaves the system untouched,
//! and a failed install reports a stable `tun.*` error code without claiming
//! success.

use crate::capture::TrafficCapture;
use crate::AppState;
use ice_config::AppError;
use ice_elevate::{ElevateError, ElevateOutcome};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::Manager;

/// Bundle resource name of the helper binary.
const HELPER_RESOURCE_NAME: &str = "ice-helper";

/// Stable error codes surfaced to the UI (plan §4.5 extension).
pub const ERR_HELPER_INSTALL_FAILED: &str = "tun.helper_install_failed";
pub const ERR_HELPER_INSTALL_CANCELLED: &str = "tun.helper_install_cancelled";
/// The elevated install reported OK but the daemon did not accept status
/// probes within the readiness window. The helper *is* installed; this is a
/// transient "not ready yet" state, distinct from an install failure so the
/// UI does not claim nothing was modified (which would push the user into a
/// needless reinstall + password re-prompt).
pub const ERR_HELPER_NOT_READY: &str = "tun.helper_not_ready";

/// Refuse install/uninstall while TUN capture is active: the elevated modes
/// restart (or remove) the launchd daemon, which would orphan the running
/// elevated core and leave it impossible to stop or verify (fail-closed).
fn refuse_while_tun_active(state: &AppState) -> Result<(), AppError> {
    if state.capture.active_backend() == TrafficCapture::Tun {
        return Err(AppError::with_code(
            ERR_HELPER_INSTALL_FAILED,
            "TUN 捕获仍处于激活状态：请先关闭 TUN 再操作辅助组件",
        ));
    }
    Ok(())
}

/// Resolve the bundled helper binary. Dev builds get the real binary from
/// `resources/ice-helper` (built by `prepare-singbox-resource.sh`); plain
/// `cargo check` leaves an empty marker, which is rejected here so nothing
/// useless is ever executed elevated.
pub fn helper_binary(resource_dir: Option<&Path>) -> Option<PathBuf> {
    let res = resource_dir?;
    let candidates = [
        res.join(HELPER_RESOURCE_NAME),
        res.join("resources").join(HELPER_RESOURCE_NAME),
    ];
    candidates
        .into_iter()
        .find(|path| path.is_file() && path.metadata().is_ok_and(|meta| meta.len() > 0))
}

/// Run the elevated installer and parse its one-line result contract
/// (`OK ...` / `ERROR: ...`). The AuthorizationServices pipe does not expose
/// the tool's exit code, so the printed line is the outcome signal.
fn run_elevated(tool: &Path, args: &[&str]) -> Result<ElevateOutcome, AppError> {
    match ice_elevate::run_as_admin(tool, args) {
        Ok(outcome) => Ok(outcome),
        Err(ElevateError::Cancelled) => Err(AppError::with_code(
            ERR_HELPER_INSTALL_CANCELLED,
            "用户取消了系统授权",
        )),
        Err(err) => Err(AppError::with_code(
            ERR_HELPER_INSTALL_FAILED,
            format!("提权执行失败：{err}"),
        )),
    }
}

fn parse_outcome(outcome: ElevateOutcome) -> Result<(), AppError> {
    let line = outcome.output.lines().next().unwrap_or("").trim();
    if let Some(detail) = line.strip_prefix("OK ") {
        tracing::info!(detail, "privileged helper install/uninstall succeeded");
        Ok(())
    } else {
        let detail = line.strip_prefix("ERROR: ").unwrap_or(line);
        Err(AppError::with_code(
            ERR_HELPER_INSTALL_FAILED,
            format!("辅助组件操作失败：{detail}"),
        ))
    }
}

/// Resolve the tool + core binary pair for an elevated run.
fn resolve_install_inputs(app: &tauri::AppHandle) -> Result<(PathBuf, PathBuf), AppError> {
    let resource_dir = app.path().resource_dir().ok();
    let helper = helper_binary(resource_dir.as_deref()).ok_or_else(|| {
        AppError::with_code(
            ERR_HELPER_INSTALL_FAILED,
            "未找到辅助组件资源（ice-helper）；请重新构建应用后重试",
        )
    })?;
    let core = crate::orchestrate::resolve_binary(resource_dir.as_deref()).map_err(|err| {
        AppError::with_code(ERR_HELPER_INSTALL_FAILED, format!("未找到内核资源：{err}"))
    })?;
    Ok((helper, core))
}

fn current_uid() -> u32 {
    #[cfg(unix)]
    {
        unsafe { libc::getuid() }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

/// Whether an authorized helper daemon is reachable right now (read-only
/// `Status` probe, short bound). Drives the Settings install/uninstall
/// buttons and the Home permission action; never mutates the OS.
pub fn helper_installed(state: &AppState) -> bool {
    #[cfg(target_os = "macos")]
    {
        let data_dir = state.paths.root();
        let socket = ice_tun_sys::helper::helper_socket_path();
        match ice_tun_sys::helper::helper_token(data_dir) {
            Ok(token) => ice_tun_sys::helper::helper_reachable_bounded(&socket, &token),
            Err(_) => false,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = state;
        false
    }
}

/// Wait for the freshly installed daemon to accept `Status` probes. `launchctl
/// bootstrap` returns once launchd loaded the job, which can precede the
/// daemon binding its socket by a moment; probing too early would report the
/// helper as missing and stall the backend refresh / UI state. A slow boot
/// (heavily loaded machine, cold disk) is given a generous window before the
/// install is reported as "installed but not ready" (fail-closed, distinct
/// from an install failure). Unix-only: the helper IPC (`ice_tun_sys::helper`)
/// exists only on unix platforms.
#[cfg(unix)]
fn wait_for_helper_ready(data_dir: &Path) -> Result<(), AppError> {
    const ATTEMPTS: u32 = 20;
    const DELAY_MS: u64 = 500;
    let socket = ice_tun_sys::helper::helper_socket_path();
    let token = ice_tun_sys::helper::helper_token(data_dir).map_err(|err| {
        AppError::with_code(
            ERR_HELPER_INSTALL_FAILED,
            format!("辅助组件未授权：{}", err.message),
        )
    })?;
    for _ in 0..ATTEMPTS {
        if ice_tun_sys::helper::helper_reachable_bounded(&socket, &token) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(DELAY_MS));
    }
    Err(AppError::with_code(
        ERR_HELPER_NOT_READY,
        "辅助组件已安装但守护进程尚未就绪，请稍后重试",
    ))
}

// --- Core version drift (one installed core version at a time) -----------
//
// The helper runs the root-owned core copy it installed and pinned. When the
// app ships a new sing-box, that copy must be replaced (the elevated
// `install` mode overwrites it, so only one version ever exists on disk).
// The bundle copy changes only across app rebuilds (process restart), so its
// hash is cached once; the installed copy changes only via the elevated
// install/uninstall modes, which reset its cache explicitly. The drift check
// drives the macOS helper refresh gate, so the drift machinery compiles on
// macOS only; the hashing primitives stay available in tests everywhere
// (host-free, temp-file based).

#[cfg(target_os = "macos")]
static BUNDLE_CORE_SHA: OnceLock<Option<String>> = OnceLock::new();
static INSTALLED_CORE_SHA: Mutex<Option<Option<String>>> = Mutex::new(None);

#[cfg(any(target_os = "macos", test))]
fn sha256_of_file(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Some(format!("{:x}", hasher.finalize()))
}

#[cfg(target_os = "macos")]
fn bundle_core_sha(resource_dir: Option<&Path>) -> Option<String> {
    BUNDLE_CORE_SHA
        .get_or_init(|| {
            crate::orchestrate::resolve_binary(resource_dir)
                .ok()
                .and_then(|path| sha256_of_file(&path))
        })
        .clone()
}

#[cfg(target_os = "macos")]
fn installed_core_sha() -> Option<String> {
    let mut slot = INSTALLED_CORE_SHA.lock().expect("helper sha lock");
    match slot.as_ref() {
        Some(cached) => cached.clone(),
        None => {
            let path = Path::new(ice_tun_sys::install_paths::CORE_BIN_DEST);
            let value = if path.is_file() {
                sha256_of_file(path)
            } else {
                None
            };
            *slot = Some(value.clone());
            value
        }
    }
}

/// Clear the cached installed-core hash after an elevated install/uninstall
/// (the root-owned copy changed).
pub fn reset_helper_core_cache() {
    *INSTALLED_CORE_SHA.lock().expect("helper sha lock") = None;
}

/// Whether the installed root-owned core differs from the app's bundled core.
/// True means the helper would still run the previous core version: TUN
/// activation is blocked until the elevated install refreshes the copy.
pub fn helper_core_stale(resource_dir: Option<&Path>) -> bool {
    #[cfg(target_os = "macos")]
    {
        cores_differ(&bundle_core_sha(resource_dir), &installed_core_sha())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = resource_dir;
        false
    }
}

/// Host-free comparison (tested with temp files).
#[cfg(any(target_os = "macos", test))]
fn cores_differ(bundle: &Option<String>, installed: &Option<String>) -> bool {
    match (bundle, installed) {
        (Some(bundle), Some(installed)) => bundle != installed,
        _ => false,
    }
}

/// Install the privileged helper through the system authorization dialog.
/// Prompts the user for their admin password; nothing is mutated before the
/// dialog is authorized, and cancellation leaves the system untouched.
pub fn install_helper_inner(app: &tauri::AppHandle) -> Result<(), AppError> {
    if !cfg!(target_os = "macos") {
        return Err(AppError::with_code(
            "tun.not_supported",
            "辅助组件安装仅支持 macOS",
        ));
    }
    let state = app.state::<AppState>();
    refuse_while_tun_active(&state)?;
    let (helper, core) = resolve_install_inputs(app)?;
    let data_dir = state.paths.root();
    if !data_dir.is_dir() {
        return Err(AppError::with_code(
            ERR_HELPER_INSTALL_FAILED,
            format!("数据目录不存在：{}", data_dir.display()),
        ));
    }
    let args = [
        "install",
        data_dir.to_str().unwrap_or_default(),
        core.to_str().unwrap_or_default(),
        &current_uid().to_string(),
    ];
    let outcome = run_elevated(&helper, &args)?;
    parse_outcome(outcome)?;
    reset_helper_core_cache();
    // The daemon may not have bound its socket yet (launchctl bootstrap
    // returns before the process is serving); wait for it so the backend
    // refresh below actually probes a reachable helper. Unix-only: the helper
    // IPC does not exist on Windows, where install is refused anyway.
    #[cfg(unix)]
    wait_for_helper_ready(data_dir)?;
    // Make the freshly installed helper the active coordinator (the probe is
    // read-only; no capture is enabled here).
    state.capture.refresh_backend()?;
    Ok(())
}

/// Uninstall the privileged helper through the system authorization dialog.
pub fn uninstall_helper_inner(app: &tauri::AppHandle) -> Result<(), AppError> {
    if !cfg!(target_os = "macos") {
        return Err(AppError::with_code(
            "tun.not_supported",
            "辅助组件卸载仅支持 macOS",
        ));
    }
    let state = app.state::<AppState>();
    refuse_while_tun_active(&state)?;
    // Prefer the installed root copy; fall back to the bundled binary.
    let helper = Some(PathBuf::from(ice_tun_sys::install_paths::HELPER_BIN_DEST))
        .filter(|p| p.is_file())
        .or_else(|| helper_binary(app.path().resource_dir().ok().as_deref()))
        .ok_or_else(|| {
            AppError::with_code(ERR_HELPER_INSTALL_FAILED, "未找到辅助组件二进制；无法卸载")
        })?;
    let data_dir = state.paths.root();
    let args = ["uninstall", data_dir.to_str().unwrap_or_default()];
    let outcome = run_elevated(&helper, &args)?;
    parse_outcome(outcome)?;
    reset_helper_core_cache();
    state.capture.refresh_backend()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_outcome_accepts_ok_line() {
        let ok = ElevateOutcome {
            output: "OK helper installed\n".into(),
        };
        assert!(parse_outcome(ok).is_ok());
    }

    #[test]
    fn parse_outcome_maps_error_line_to_friendly_error() {
        let err = ElevateOutcome {
            output: "ERROR: core binary is group/world-writable\n".into(),
        };
        let app_err = parse_outcome(err).expect_err("error line");
        assert_eq!(app_err.code, ERR_HELPER_INSTALL_FAILED);
        assert!(app_err.message.contains("group/world-writable"));
    }

    #[test]
    fn parse_outcome_handles_empty_output() {
        let err = parse_outcome(ElevateOutcome {
            output: String::new(),
        })
        .expect_err("empty");
        assert_eq!(err.code, ERR_HELPER_INSTALL_FAILED);
    }

    #[test]
    fn helper_binary_rejects_empty_marker_files() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-helper-bin-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(HELPER_RESOURCE_NAME), b"").unwrap();
        assert!(helper_binary(Some(&dir)).is_none());

        std::fs::write(dir.join(HELPER_RESOURCE_NAME), b"real").unwrap();
        assert_eq!(
            helper_binary(Some(&dir)),
            Some(dir.join(HELPER_RESOURCE_NAME))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cores_differ_only_when_both_hashes_exist_and_differ() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-core-drift-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("old");
        let new = dir.join("new");
        std::fs::write(&old, b"old-core").unwrap();
        std::fs::write(&new, b"new-core").unwrap();
        std::fs::write(dir.join("same"), b"same").unwrap();

        let old_sha = sha256_of_file(&old);
        let new_sha = sha256_of_file(&new);
        let same_sha = sha256_of_file(&dir.join("same"));

        // Version change (both exist, different content) -> stale.
        assert!(cores_differ(&old_sha, &new_sha));
        assert!(cores_differ(&new_sha, &old_sha));
        // Same content -> not stale.
        assert!(!cores_differ(&same_sha, &same_sha));
        // Missing installed core -> not stale (helper absent or broken).
        assert!(!cores_differ(&old_sha, &None));
        assert!(!cores_differ(&None, &old_sha));
        assert!(!cores_differ(&None, &None));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
