// SPDX-License-Identifier: GPL-3.0-or-later

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

use crate::capture::{TrafficCapture, TunStatus};
use crate::AppState;
use ice_config::{AppError, ErrorCode};
use ice_elevate::{ElevateError, ElevateOutcome};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::sync::OnceLock;

/// Bundle resource name of the helper binary.
const HELPER_RESOURCE_NAME: &str = "ice-helper";

/// Stable error codes surfaced to the UI.
pub const ERR_HELPER_INSTALL_FAILED: ErrorCode = ErrorCode::TunHelperInstallFailed;
pub const ERR_HELPER_INSTALL_CANCELLED: ErrorCode = ErrorCode::TunHelperInstallCancelled;
/// The elevated install reported OK but the daemon did not accept status
/// probes within the readiness window. The helper *is* installed; this is a
/// transient "not ready yet" state, distinct from an install failure so the
/// UI does not claim nothing was modified (which would push the user into a
/// needless reinstall + password re-prompt). Created only on unix hosts (the
/// readiness wait itself is unix-only).
#[cfg(unix)]
pub const ERR_HELPER_NOT_READY: ErrorCode = ErrorCode::TunHelperNotReady;

/// Refuse install/uninstall while TUN capture is active *or* a transition is
/// in flight: the elevated modes restart (or remove) the launchd daemon,
/// which would orphan the running elevated core and leave it impossible to
/// stop or verify (fail-closed). The `Preparing` / `Stopping` window is
/// covered too — that is exactly when the elevated core may be starting or
/// stopping, and `active_backend()` still reads `Inactive` there (it is set
/// only at `finish_transition`).
fn refuse_while_tun_active(state: &AppState) -> Result<(), AppError> {
    if state.capture.active_backend() == TrafficCapture::Tun
        || matches!(
            state.capture.tun_status(),
            TunStatus::Preparing | TunStatus::Stopping
        )
    {
        return Err(AppError::with_code(
            ERR_HELPER_INSTALL_FAILED,
            "TUN capture is still active: disable TUN before managing the helper",
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
            "system authorization cancelled by the user",
        )),
        Err(err) => Err(AppError::with_code(
            ERR_HELPER_INSTALL_FAILED,
            format!("privileged execution failed: {err}"),
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
            format!("helper operation failed: {detail}"),
        ))
    }
}

/// Resolve the tool + core binary pair for an elevated run.
fn resolve_install_inputs(
    resources: &impl crate::application::AppResources,
) -> Result<(PathBuf, PathBuf), AppError> {
    let resource_dir = resources.resource_dir();
    let helper = helper_binary(resource_dir.as_deref()).ok_or_else(|| {
        AppError::with_code(
            ERR_HELPER_INSTALL_FAILED,
            "helper resource (ice-helper) not found; rebuild the app and retry",
        )
    })?;
    let core = crate::orchestrate::resolve_binary(resource_dir.as_deref()).map_err(|err| {
        AppError::with_code(
            ERR_HELPER_INSTALL_FAILED,
            format!("core resource not found: {err}"),
        )
    })?;
    Ok((helper, core))
}

// --- Linux AppImage private-FUSE-mount staging ---------------------------
//
// An AppImage mounts its SquashFS payload via FUSE at `/tmp/.mount_*` without
// `allow_other`/`allow_root`, so root (`pkexec` and the elevated installer)
// receives `EACCES` when trying to execute `ice-helper` or read `sing-box` and
// `geoip/*.srs`. When running from an AppImage mount, stage the files into a
// fresh user-owned `0700` temp directory on a regular filesystem and remove it
// via RAII as soon as the elevated command finishes.

#[cfg(any(target_os = "linux", test))]
fn path_under_tmp_mount(path: &Path) -> bool {
    let mut comps = path.components();
    matches!(
        (comps.next(), comps.next(), comps.next()),
        (
            Some(std::path::Component::RootDir),
            Some(std::path::Component::Normal(tmp)),
            Some(std::path::Component::Normal(mount)),
        ) if tmp == "tmp" && mount.to_string_lossy().starts_with(".mount_")
    )
}

/// Pure check for whether `resource_path` lies inside an AppImage private FUSE
/// mount that root cannot access.
///
/// Checking `current_exe` / `resource_path` against `APPDIR` (in addition to
/// `APPIMAGE` being non-empty) prevents false positives when a `.deb` install
/// inherits `$APPIMAGE` / `$APPDIR` from a parent AppImage (e.g., an IDE's
/// integrated terminal).
#[cfg(any(target_os = "linux", test))]
fn needs_appimage_staging(
    appimage: Option<&std::ffi::OsStr>,
    appdir: Option<&std::ffi::OsStr>,
    current_exe: Option<&Path>,
    resource_path: &Path,
) -> bool {
    if path_under_tmp_mount(resource_path) {
        return true;
    }
    let has_appimage = appimage.is_some_and(|v| !v.is_empty());
    if !has_appimage {
        return false;
    }
    let Some(appdir_path) = appdir.filter(|v| !v.is_empty()).map(Path::new).filter(|p| {
        (p.is_absolute() || p.to_string_lossy().starts_with('/')) && *p != Path::new("/")
    }) else {
        return false;
    };
    let exe_in_appdir = current_exe.is_some_and(|exe| exe.starts_with(appdir_path));
    let resource_in_appdir = resource_path.starts_with(appdir_path);
    exe_in_appdir || resource_in_appdir
}

/// RAII guard for a `0700` temporary staging directory used during Linux
/// AppImage helper install/uninstall. Always removes the directory on drop.
#[cfg(any(target_os = "linux", all(unix, test)))]
struct HelperStagingDir {
    dir: PathBuf,
}

#[cfg(any(target_os = "linux", all(unix, test)))]
impl Drop for HelperStagingDir {
    fn drop(&mut self) {
        if let Err(err) = std::fs::remove_dir_all(&self.dir) {
            if err.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    path = %self.dir.display(),
                    error = %err,
                    "failed to remove helper staging directory"
                );
            }
        }
    }
}

#[cfg(any(target_os = "linux", all(unix, test)))]
fn create_staging_dir(base_tmp: &Path) -> Result<HelperStagingDir, AppError> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let pid = std::process::id();
    for _ in 0..16 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = base_tmp.join(format!("ice-box-helper-stage-{pid}-{nanos}-{seq}"));
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {
                // Construct the RAII guard immediately so any later failure cleans up.
                let guard = HelperStagingDir { dir };
                std::fs::set_permissions(&guard.dir, std::fs::Permissions::from_mode(0o700))
                    .map_err(|err| {
                        AppError::with_code(
                            ERR_HELPER_INSTALL_FAILED,
                            format!(
                                "failed to set permissions on helper staging dir {}: {err}",
                                guard.dir.display()
                            ),
                        )
                    })?;
                return Ok(guard);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(AppError::with_code(
                    ERR_HELPER_INSTALL_FAILED,
                    format!(
                        "failed to create helper staging dir {}: {err}",
                        dir.display()
                    ),
                ));
            }
        }
    }
    Err(AppError::with_code(
        ERR_HELPER_INSTALL_FAILED,
        "failed to create a unique helper staging directory",
    ))
}

#[cfg(any(target_os = "linux", all(unix, test)))]
fn copy_staged_file(src: &Path, dst: &Path, mode: u32) -> Result<(), AppError> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut reader = std::fs::File::open(src).map_err(|err| {
        AppError::with_code(
            ERR_HELPER_INSTALL_FAILED,
            format!("failed to read bundled file {}: {err}", src.display()),
        )
    })?;
    let mut writer = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(dst)
        .map_err(|err| {
            AppError::with_code(
                ERR_HELPER_INSTALL_FAILED,
                format!("failed to stage file {}: {err}", dst.display()),
            )
        })?;
    std::io::copy(&mut reader, &mut writer).map_err(|err| {
        AppError::with_code(
            ERR_HELPER_INSTALL_FAILED,
            format!(
                "failed to copy {} to staging {}: {err}",
                src.display(),
                dst.display()
            ),
        )
    })?;
    drop(writer);
    // Explicitly chmod so a permissive user umask (e.g. 0002) never leaves a
    // group-writable staged file that the root installer would reject.
    std::fs::set_permissions(dst, std::fs::Permissions::from_mode(mode)).map_err(|err| {
        AppError::with_code(
            ERR_HELPER_INSTALL_FAILED,
            format!(
                "failed to set mode {mode:o} on staged file {}: {err}",
                dst.display()
            ),
        )
    })?;
    Ok(())
}

/// Stage `ice-helper` (`0755`), `sing-box` (`0755`), and `geoip/*.srs` (`0644`)
/// into a fresh `0700` temp directory preserving the exact relative layout
/// `resources_dir_from_core_src(core)` expects (`stage/sing-box`,
/// `stage/geoip/*.srs`).
#[cfg(any(target_os = "linux", all(unix, test)))]
fn stage_install_inputs(
    base_tmp: &Path,
    helper_src: &Path,
    core_src: &Path,
) -> Result<(HelperStagingDir, PathBuf, PathBuf), AppError> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let guard = create_staging_dir(base_tmp)?;
    let staged_helper = guard.dir.join(HELPER_RESOURCE_NAME);
    let staged_core = guard.dir.join("sing-box");

    copy_staged_file(helper_src, &staged_helper, 0o755)?;
    copy_staged_file(core_src, &staged_core, 0o755)?;

    if let Some(src_geoip) = core_src.parent().map(|p| p.join("geoip")) {
        if src_geoip.is_dir() {
            let staged_geoip = guard.dir.join("geoip");
            std::fs::DirBuilder::new()
                .mode(0o755)
                .create(&staged_geoip)
                .map_err(|err| {
                    AppError::with_code(
                        ERR_HELPER_INSTALL_FAILED,
                        format!(
                            "failed to create staged geoip dir {}: {err}",
                            staged_geoip.display()
                        ),
                    )
                })?;
            std::fs::set_permissions(&staged_geoip, std::fs::Permissions::from_mode(0o755))
                .map_err(|err| {
                    AppError::with_code(
                        ERR_HELPER_INSTALL_FAILED,
                        format!(
                            "failed to chmod staged geoip dir {}: {err}",
                            staged_geoip.display()
                        ),
                    )
                })?;

            let entries = std::fs::read_dir(&src_geoip).map_err(|err| {
                AppError::with_code(
                    ERR_HELPER_INSTALL_FAILED,
                    format!(
                        "failed to list bundled geoip dir {}: {err}",
                        src_geoip.display()
                    ),
                )
            })?;
            for entry in entries {
                let entry = entry.map_err(|err| {
                    AppError::with_code(
                        ERR_HELPER_INSTALL_FAILED,
                        format!(
                            "failed to read bundled geoip dir {}: {err}",
                            src_geoip.display()
                        ),
                    )
                })?;
                let name = entry.file_name();
                if !name.to_string_lossy().ends_with(".srs") {
                    continue;
                }
                let src_path = entry.path();
                if !src_path.is_file() {
                    continue;
                }
                copy_staged_file(&src_path, &staged_geoip.join(&name), 0o644)?;
            }
        }
    }

    Ok((guard, staged_helper, staged_core))
}

/// Stage only `ice-helper` (`0755`) into a fresh `0700` temp directory when
/// `uninstall_helper_inner` falls back to the bundled helper inside an AppImage.
#[cfg(any(target_os = "linux", all(unix, test)))]
fn stage_uninstall_helper(
    base_tmp: &Path,
    helper_src: &Path,
) -> Result<(HelperStagingDir, PathBuf), AppError> {
    let guard = create_staging_dir(base_tmp)?;
    let staged_helper = guard.dir.join(HELPER_RESOURCE_NAME);
    copy_staged_file(helper_src, &staged_helper, 0o755)?;
    Ok((guard, staged_helper))
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
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let data_dir = state.paths.root();
        let socket = ice_tun_sys::helper::helper_socket_path();
        match ice_tun_sys::helper::helper_token(data_dir) {
            Ok(token) => ice_tun_sys::helper::helper_reachable_bounded(&socket, &token),
            Err(_) => false,
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
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
            format!("helper not authorized: {}", err.message),
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
        "helper installed, but its daemon is not ready yet; try again later",
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

#[cfg(any(target_os = "macos", target_os = "linux"))]
static BUNDLE_CORE_SHA: OnceLock<Option<String>> = OnceLock::new();
static INSTALLED_CORE_SHA: Mutex<Option<Option<String>>> = Mutex::new(None);

#[cfg(any(target_os = "macos", target_os = "linux", test))]
fn sha256_of_file(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Some(format!("{:x}", hasher.finalize()))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn bundle_core_sha(resource_dir: Option<&Path>) -> Option<String> {
    BUNDLE_CORE_SHA
        .get_or_init(|| {
            crate::orchestrate::resolve_binary(resource_dir)
                .ok()
                .and_then(|path| sha256_of_file(&path))
        })
        .clone()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
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
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        cores_differ(&bundle_core_sha(resource_dir), &installed_core_sha())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = resource_dir;
        false
    }
}

/// Host-free comparison (tested with temp files).
#[cfg(any(target_os = "macos", target_os = "linux", test))]
fn cores_differ(bundle: &Option<String>, installed: &Option<String>) -> bool {
    match (bundle, installed) {
        (Some(bundle), Some(installed)) => bundle != installed,
        _ => false,
    }
}

/// Install the helper (Settings, CLI, and MCP `install_tun_helper`), then drop
/// the cached helper probe whatever the outcome, so the backend sees the new
/// state at once.
pub fn install_helper_use_case(
    host: &impl crate::application::AppHost,
    state: &AppState,
) -> Result<(), AppError> {
    let result = install_helper_inner(host, state);
    after_helper_change(host, state);
    result
}

/// Uninstall the helper (Settings, CLI, and MCP `uninstall_tun_helper`); see
/// [`install_helper_use_case`].
pub fn uninstall_helper_use_case(
    host: &impl crate::application::AppHost,
    state: &AppState,
) -> Result<(), AppError> {
    let result = uninstall_helper_inner(host, state);
    after_helper_change(host, state);
    result
}

fn after_helper_change(host: &impl crate::application::AppHost, state: &AppState) {
    crate::application::reset_helper_probe_cache(state);
    crate::application::broadcast_state_change(host);
}

/// Install the privileged helper through the system authorization dialog.
/// Prompts the user for their admin password; nothing is mutated before the
/// dialog is authorized, and cancellation leaves the system untouched.
pub fn install_helper_inner(
    host: &impl crate::application::AppHost,
    state: &AppState,
) -> Result<(), AppError> {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        return Err(AppError::new(
            ErrorCode::TunNotSupported,
            "helper install is only supported on macOS and Linux",
        ));
    }
    refuse_while_tun_active(state)?;
    let (helper, core) = resolve_install_inputs(host)?;
    let data_dir = state.paths.root();
    if !host.interactive_elevation() {
        return Err(AppError::with_code(
            ERR_HELPER_INSTALL_FAILED,
            format!(
                "interactive elevation is disabled in headless mode; run: sudo {} install {} {} {}",
                helper.display(),
                data_dir.display(),
                core.display(),
                current_uid(),
            ),
        ));
    }
    #[cfg(target_os = "linux")]
    let _staging_guard;
    #[cfg(target_os = "linux")]
    let (helper, core) = {
        let current_exe = std::env::current_exe().ok();
        let appimage = std::env::var_os("APPIMAGE");
        let appdir = std::env::var_os("APPDIR");
        if needs_appimage_staging(
            appimage.as_deref(),
            appdir.as_deref(),
            current_exe.as_deref(),
            &helper,
        ) || needs_appimage_staging(
            appimage.as_deref(),
            appdir.as_deref(),
            current_exe.as_deref(),
            &core,
        ) {
            let (guard, staged_helper, staged_core) =
                stage_install_inputs(&std::env::temp_dir(), &helper, &core)?;
            _staging_guard = Some(guard);
            (staged_helper, staged_core)
        } else {
            _staging_guard = None;
            (helper, core)
        }
    };
    if !data_dir.is_dir() {
        return Err(AppError::with_code(
            ERR_HELPER_INSTALL_FAILED,
            format!("data directory does not exist: {}", data_dir.display()),
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
pub fn uninstall_helper_inner(
    host: &impl crate::application::AppHost,
    state: &AppState,
) -> Result<(), AppError> {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        return Err(AppError::new(
            ErrorCode::TunNotSupported,
            "helper uninstall is only supported on macOS and Linux",
        ));
    }
    refuse_while_tun_active(state)?;
    // Prefer the installed root copy; fall back to the bundled binary.
    let installed_helper = PathBuf::from(ice_tun_sys::install_paths::HELPER_BIN_DEST);
    #[cfg(target_os = "linux")]
    let _staging_guard;
    let helper = if installed_helper.is_file() {
        #[cfg(target_os = "linux")]
        {
            _staging_guard = None;
        }
        installed_helper
    } else {
        let bundled = helper_binary(host.resource_dir().as_deref()).ok_or_else(|| {
            AppError::with_code(
                ERR_HELPER_INSTALL_FAILED,
                "helper binary not found; cannot uninstall",
            )
        })?;
        #[cfg(target_os = "linux")]
        {
            let current_exe = std::env::current_exe().ok();
            let appimage = std::env::var_os("APPIMAGE");
            let appdir = std::env::var_os("APPDIR");
            if needs_appimage_staging(
                appimage.as_deref(),
                appdir.as_deref(),
                current_exe.as_deref(),
                &bundled,
            ) {
                let (guard, staged_helper) =
                    stage_uninstall_helper(&std::env::temp_dir(), &bundled)?;
                _staging_guard = Some(guard);
                staged_helper
            } else {
                _staging_guard = None;
                bundled
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            bundled
        }
    };
    let data_dir = state.paths.root();
    if !host.interactive_elevation() {
        return Err(AppError::with_code(
            ERR_HELPER_INSTALL_FAILED,
            format!(
                "interactive elevation is disabled in headless mode; run: sudo {} uninstall {}",
                helper.display(),
                data_dir.display(),
            ),
        ));
    }
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
        assert_eq!(app_err.code, ERR_HELPER_INSTALL_FAILED.as_str());
        assert!(app_err.message.contains("group/world-writable"));
    }

    #[test]
    fn parse_outcome_handles_empty_output() {
        let err = parse_outcome(ElevateOutcome {
            output: String::new(),
        })
        .expect_err("empty");
        assert_eq!(err.code, ERR_HELPER_INSTALL_FAILED.as_str());
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

    #[test]
    fn needs_appimage_staging_detects_real_appimage_and_ignores_inherited_env() {
        use std::ffi::OsStr;

        // Plain .deb install: no APPIMAGE, regular /usr paths -> false.
        assert!(!needs_appimage_staging(
            None,
            None,
            Some(Path::new("/usr/bin/ice-box")),
            Path::new("/usr/lib/ice-box/ice-helper"),
        ));

        // .deb launched from inside another AppImage's terminal (e.g. Cursor):
        // APPIMAGE + APPDIR belong to Cursor, not ice-box -> false.
        assert!(!needs_appimage_staging(
            Some(OsStr::new("/opt/cursor.AppImage")),
            Some(OsStr::new("/tmp/.mount_CursorABC")),
            Some(Path::new("/usr/bin/ice-box")),
            Path::new("/usr/lib/ice-box/ice-helper"),
        ));

        // Real ice-box AppImage in default /tmp/.mount_* -> true.
        assert!(needs_appimage_staging(
            Some(OsStr::new("/home/u/ice-box.AppImage")),
            Some(OsStr::new("/tmp/.mount_iceboxXYZ")),
            Some(Path::new("/tmp/.mount_iceboxXYZ/usr/bin/ice-box")),
            Path::new("/tmp/.mount_iceboxXYZ/usr/lib/ice-box/ice-helper"),
        ));

        // Real AppImage mounted under a custom TMPDIR -> true via APPDIR match.
        assert!(needs_appimage_staging(
            Some(OsStr::new("/home/u/ice-box.AppImage")),
            Some(OsStr::new("/run/user/1000/.mount_iceboxXYZ")),
            Some(Path::new("/run/user/1000/.mount_iceboxXYZ/usr/bin/ice-box")),
            Path::new("/run/user/1000/.mount_iceboxXYZ/usr/lib/ice-box/ice-helper"),
        ));

        // Path under /tmp/.mount_* is always treated as private FUSE mount.
        assert!(needs_appimage_staging(
            None,
            None,
            Some(Path::new("/usr/bin/ice-box")),
            Path::new("/tmp/.mount_iceboxXYZ/usr/lib/ice-box/sing-box"),
        ));

        // Empty APPIMAGE or malformed root APPDIR -> false.
        assert!(!needs_appimage_staging(
            Some(OsStr::new("")),
            Some(OsStr::new("/custom/mount")),
            Some(Path::new("/custom/mount/usr/bin/ice-box")),
            Path::new("/custom/mount/usr/lib/ice-box/ice-helper"),
        ));
        assert!(!needs_appimage_staging(
            Some(OsStr::new("/home/u/ice-box.AppImage")),
            Some(OsStr::new("/")),
            Some(Path::new("/usr/bin/ice-box")),
            Path::new("/usr/lib/ice-box/ice-helper"),
        ));
    }

    #[cfg(unix)]
    #[test]
    fn stage_install_inputs_preserves_layout_modes_and_cleans_up_on_drop() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "ice-box-stage-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let src = root.join("bundle");
        let tmp_base = root.join("tmp");
        std::fs::create_dir_all(src.join("geoip")).unwrap();
        std::fs::create_dir_all(&tmp_base).unwrap();

        let helper_src = src.join("ice-helper");
        let core_src = src.join("sing-box");
        let srs_src = src.join("geoip").join("geoip-cn.srs");
        std::fs::write(&helper_src, b"fake-helper").unwrap();
        std::fs::write(&core_src, b"fake-sing-box").unwrap();
        std::fs::write(&srs_src, b"fake-srs").unwrap();
        std::fs::write(src.join("geoip").join("README.txt"), b"skip").unwrap();

        let stage_dir_path;
        {
            let (guard, staged_helper, staged_core) =
                stage_install_inputs(&tmp_base, &helper_src, &core_src).expect("stage install");
            stage_dir_path = guard.dir.clone();
            assert_eq!(
                std::fs::metadata(&stage_dir_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(staged_helper, stage_dir_path.join("ice-helper"));
            assert_eq!(staged_core, stage_dir_path.join("sing-box"));
            assert_eq!(std::fs::read(&staged_helper).unwrap(), b"fake-helper");
            assert_eq!(std::fs::read(&staged_core).unwrap(), b"fake-sing-box");
            assert_eq!(
                std::fs::metadata(&staged_helper)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o755
            );
            assert_eq!(
                std::fs::metadata(&staged_core)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o755
            );

            // `resources_dir_from_core_src(staged_core)` expects `geoip/*.srs`
            // directly inside `staged_core.parent()`.
            let staged_geoip = staged_core.parent().unwrap().join("geoip");
            let staged_srs = staged_geoip.join("geoip-cn.srs");
            assert_eq!(std::fs::read(&staged_srs).unwrap(), b"fake-srs");
            assert_eq!(
                std::fs::metadata(&staged_srs).unwrap().permissions().mode() & 0o777,
                0o644
            );
            assert!(!staged_geoip.join("README.txt").exists());
        }
        // Guard dropped: staging directory must be completely removed.
        assert!(!stage_dir_path.exists());

        // Error during staging (missing core) must not leave a staging dir behind.
        let err = stage_install_inputs(&tmp_base, &helper_src, &src.join("missing-core"))
            .err()
            .expect("missing core fails");
        assert_eq!(err.code, ERR_HELPER_INSTALL_FAILED.as_str());
        assert_eq!(std::fs::read_dir(&tmp_base).unwrap().count(), 0);

        // Uninstall staging helper copies ice-helper (0755) and cleans up on drop.
        let uninstall_stage_path;
        {
            let (guard, staged_helper) =
                stage_uninstall_helper(&tmp_base, &helper_src).expect("stage uninstall");
            uninstall_stage_path = guard.dir.clone();
            assert_eq!(
                std::fs::metadata(&uninstall_stage_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(std::fs::read(&staged_helper).unwrap(), b"fake-helper");
            assert_eq!(
                std::fs::metadata(&staged_helper)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o755
            );
        }
        assert!(!uninstall_stage_path.exists());

        let _ = std::fs::remove_dir_all(&root);
    }
}
