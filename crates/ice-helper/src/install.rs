// SPDX-License-Identifier: GPL-3.0-or-later

//! Privileged install / uninstall modes for the helper daemon.
//!
//! These modes run **as root** and are the only thing the unsigned elevation
//! path (AuthorizationServices password dialog, `ice-elevate`) is allowed to
//! execute; the desktop process itself never runs elevated. They replace the
//! old shell-script installer so there is exactly one implementation of the
//! installation logic (plist, token, ownership, pinned hash), shared by the
//! app's in-app installer and the manual/CI script.
//!
//! Security model (unchanged from the script, `docs/tun.md`):
//!
//! - The install must be started as root ([`require_root`]).
//! - The source core and helper binaries must be regular files (not
//!   symlinks), owned by root or the installing uid, without group/world
//!   write. Only a root-owned copy is ever executed elevated.
//! - The installed core's SHA-256 is pinned in the launchd plist; the daemon
//!   refuses to start when the on-disk binary does not match.
//! - The per-installation token is generated here (inside the elevated
//!   process, from `/dev/urandom`) and written `0600` owned by the
//!   installing uid. The name is unlinked first, then created exclusive
//!   (`O_CREAT|O_EXCL|O_NOFOLLOW`); ownership is applied on the open fd
//!   (`fchown` / `fchmod`) so a pre-planted symlink in the user data dir
//!   cannot redirect the privileged write or chown. The daemon runs as
//!   root and can still read it; other users cannot. The peer uid check is
//!   the primary control; the token is secondary.
//! - Logs are recreated as root-owned fixed files so a stale symlink can
//!   never become the target of a privileged append.

use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(any(target_os = "macos", test))]
pub use ice_tun_helper_proto::install_paths::LAUNCHD_LABEL;
#[cfg(any(target_os = "linux", test))]
pub use ice_tun_helper_proto::install_paths::RESOURCES_DEST_DIR;
pub use ice_tun_helper_proto::install_paths::{
    CORE_BIN_DEST, CORE_BIN_DEST_DIR, CORE_LOG_DEST, CORE_RUN_DIR, ENV_ALLOWED_UID, ENV_CORE_BIN,
    ENV_CORE_BIN_SHA256, ENV_CORE_LOG, ENV_DATA_DIR, ENV_RESOURCES_DIR, ENV_SOCKET, ENV_TOKEN,
    HELPER_BIN_DEST, HELPER_LOG_DEST, PLIST_DEST, SOCKET_PATH, TOKEN_FILE_NAME,
};
#[cfg(target_os = "linux")]
pub use ice_tun_helper_proto::install_paths::{SYSTEMD_UNIT_DEST, SYSTEMD_UNIT_NAME};

/// The installer must be running as root; everything below is a privileged
/// OS mutation. The app only reaches this code through the elevation dialog.
fn require_root() -> Result<(), String> {
    let euid = unsafe { libc::geteuid() };
    if euid != 0 {
        return Err(format!(
            "refusing to run as uid {euid}: install/uninstall must be executed as root via the authorization dialog"
        ));
    }
    Ok(())
}

/// Random 32-byte hex token from `/dev/urandom`.
fn generate_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    let file =
        std::fs::File::open("/dev/urandom").map_err(|e| format!("open /dev/urandom: {e}"))?;
    use std::io::Read;
    let mut reader = std::io::BufReader::new(file);
    reader
        .read_exact(&mut bytes)
        .map_err(|e| format!("read /dev/urandom: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// SHA-256 of a file, hex-encoded (pinned in the plist).
pub fn sha256_of_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Refuse a source binary that any other user could replace. It must be a
/// regular file (not a symlink), without group/world write bits, owned by
/// root or the installing uid. Bundled app binaries are user-owned; they
/// are copied to a root-owned destination before the helper executes them.
fn reject_untrusted_source(path: &Path, what: &str, allowed_uid: u32) -> Result<(), String> {
    let meta =
        fs::symlink_metadata(path).map_err(|e| format!("lstat {what} {}: {e}", path.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!("{what} must not be a symlink: {}", path.display()));
    }
    if !meta.is_file() {
        return Err(format!("{what} is not a regular file: {}", path.display()));
    }
    let mode = meta.mode() & 0o777;
    if mode & 0o022 != 0 {
        return Err(format!(
            "{what} is group/world-writable ({mode:o}): {} — a root-executed binary must not be replaceable by non-root users",
            path.display()
        ));
    }
    let owner = meta.uid();
    if owner != 0 && owner != allowed_uid {
        return Err(format!(
            "{what} owner uid {owner} is neither root nor the installing user {allowed_uid}: {}",
            path.display()
        ));
    }
    Ok(())
}

fn chown_root(path: &Path) -> Result<(), String> {
    let c_path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| format!("path contains NUL: {}", path.display()))?;
    let rc = unsafe { libc::chown(c_path.as_ptr(), 0, 0) };
    if rc != 0 {
        return Err(format!(
            "chown root {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn chown_uid(path: &Path, uid: u32) -> Result<(), String> {
    let c_path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| format!("path contains NUL: {}", path.display()))?;
    let rc = unsafe { libc::chown(c_path.as_ptr(), uid, 0) };
    if rc != 0 {
        return Err(format!(
            "chown uid {uid} {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|e| format!("chmod {mode:o} {}: {e}", path.display()))
}

/// Write `contents` at `path` as a new regular file owned by `uid` mode 0600.
///
/// The data dir is user-writable, so `path` may already be a symlink to an
/// arbitrary file. `remove_file` drops a pre-planted name (a symlink is
/// unlinked without touching its target). `create_new` + `O_NOFOLLOW` then
/// refuses a name that reappeared. `fchown`/`fchmod` the open fd so a
/// replace-after-create race cannot redirect ownership.
fn write_user_owned_secret(path: &Path, contents: &str, uid: u32) -> Result<(), String> {
    let _ = fs::remove_file(path);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| format!("create token {}: {e}", path.display()))?;
    file.write_all(contents.as_bytes())
        .map_err(|e| format!("write token {}: {e}", path.display()))?;
    file.sync_all()
        .map_err(|e| format!("sync token {}: {e}", path.display()))?;
    let fd = file.as_raw_fd();
    // `chown(2)`: -1 leaves an id unchanged. Production install is root, so
    // pin gid 0; a non-root test must not attempt `fchown(..., 0)`.
    let gid = if unsafe { libc::geteuid() } == 0 {
        0
    } else {
        u32::MAX
    };
    let rc = unsafe { libc::fchown(fd, uid, gid) };
    if rc != 0 {
        return Err(format!(
            "fchown uid {uid} {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    let rc = unsafe { libc::fchmod(fd, 0o600) };
    if rc != 0 {
        return Err(format!(
            "fchmod 0600 {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn copy_root_owned(source: &Path, dest: &Path, mode: u32) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let mut src = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(source)
        .map_err(|e| format!("open source {}: {e}", source.display()))?;
    let mut dst = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dest)
        .map_err(|e| format!("open dest {}: {e}", dest.display()))?;
    io::copy(&mut src, &mut dst)
        .map_err(|e| format!("copy {} -> {}: {e}", source.display(), dest.display()))?;
    dst.sync_all()
        .map_err(|e| format!("sync {}: {e}", dest.display()))?;
    drop(dst);
    chown_root(dest)?;
    set_mode(dest, mode)
}

/// Escape a string for inclusion as XML text content. The plist interpolates
/// user-controlled values (the app data dir); `&`, `<`, `>`, quotes and
/// apostrophes must never reach launchd's parser as raw markup.
#[cfg(any(target_os = "macos", test))]
pub fn xml_escape(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

/// Render the launchd plist that pins token / data dir / core binary (with
/// SHA-256) / log / authorized uid / socket. Host-free and tested.
#[cfg(any(target_os = "macos", test))]
#[allow(clippy::too_many_arguments)]
pub fn render_plist(
    token: &str,
    data_dir: &Path,
    core_bin_dest: &str,
    core_bin_sha256: &str,
    core_log: &str,
    allowed_uid: u32,
    socket: &str,
    resources_dir: &Path,
) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LAUNCHD_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{HELPER_BIN_DEST}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <false/>
  <key>ProcessType</key>
  <string>Background</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>{ENV_TOKEN}</key>
    <string>{token}</string>
    <key>{ENV_DATA_DIR}</key>
    <string>{}</string>
    <key>{ENV_CORE_BIN}</key>
    <string>{core_bin_dest}</string>
    <key>{ENV_CORE_BIN_SHA256}</key>
    <string>{core_bin_sha256}</string>
    <key>{ENV_CORE_LOG}</key>
    <string>{core_log}</string>
    <key>{ENV_ALLOWED_UID}</key>
    <string>{allowed_uid}</string>
    <key>{ENV_SOCKET}</key>
    <string>{socket}</string>
    <key>{ENV_RESOURCES_DIR}</key>
    <string>{}</string>
  </dict>
  <key>StandardOutPath</key>
  <string>{HELPER_LOG_DEST}</string>
  <key>StandardErrorPath</key>
  <string>{HELPER_LOG_DEST}</string>
</dict>
</plist>
"#,
        xml_escape(&data_dir.display().to_string()),
        xml_escape(&resources_dir.display().to_string()),
    )
}

/// Escape a value for a systemd `Environment="KEY=VALUE"` assignment.
///
/// Rejects control characters (`\n`, `\r`, `\0`) that would inject extra unit
/// directives, escapes `\` and `"` inside double quotes, and doubles `%` so
/// systemd does not expand specifiers such as `%h` or `%i`.
#[cfg(any(target_os = "linux", test))]
pub fn systemd_escape_env_value(input: &str) -> Result<String, String> {
    if input.chars().any(|c| matches!(c, '\n' | '\r' | '\0')) {
        return Err("systemd env value contains forbidden control character".to_string());
    }
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '%' => out.push_str("%%"),
            _ => out.push(ch),
        }
    }
    Ok(out)
}

/// Render the Linux systemd unit that pins token / data dir / core binary
/// (with SHA-256) / log / authorized uid / socket / resources dir.
#[cfg(any(target_os = "linux", test))]
#[allow(clippy::too_many_arguments)]
pub fn render_systemd_unit(
    token: &str,
    data_dir: &Path,
    core_bin_dest: &str,
    core_bin_sha256: &str,
    core_log: &str,
    allowed_uid: u32,
    socket: &str,
    resources_dir: &Path,
) -> Result<String, String> {
    let data_dir_str = systemd_escape_env_value(&data_dir.display().to_string())?;
    let resources_dir_str = systemd_escape_env_value(&resources_dir.display().to_string())?;
    let token_str = systemd_escape_env_value(token)?;
    let core_bin_str = systemd_escape_env_value(core_bin_dest)?;
    let core_sha_str = systemd_escape_env_value(core_bin_sha256)?;
    let core_log_str = systemd_escape_env_value(core_log)?;
    let socket_str = systemd_escape_env_value(socket)?;
    Ok(format!(
        "[Unit]\n\
         Description=ice-box privileged TUN helper daemon\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={HELPER_BIN_DEST}\n\
         Restart=no\n\
         StandardOutput=append:{HELPER_LOG_DEST}\n\
         StandardError=append:{HELPER_LOG_DEST}\n\
         Environment=\"{ENV_TOKEN}={token_str}\"\n\
         Environment=\"{ENV_DATA_DIR}={data_dir_str}\"\n\
         Environment=\"{ENV_CORE_BIN}={core_bin_str}\"\n\
         Environment=\"{ENV_CORE_BIN_SHA256}={core_sha_str}\"\n\
         Environment=\"{ENV_CORE_LOG}={core_log_str}\"\n\
         Environment=\"{ENV_ALLOWED_UID}={allowed_uid}\"\n\
         Environment=\"{ENV_SOCKET}={socket_str}\"\n\
         Environment=\"{ENV_RESOURCES_DIR}={resources_dir_str}\"\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n"
    ))
}

/// Copy bundled `geoip/*.srs` files from `src_geoip` into `dest_geoip`,
/// rejecting symlinks or group/world-writable inputs so the root helper's
/// fixed resources directory never depends on an ephemeral AppImage mount.
#[cfg(any(target_os = "linux", test))]
fn copy_geoip_srs_files(
    src_geoip: &Path,
    dest_geoip: &Path,
    allowed_uid: u32,
    chown_to_root: bool,
) -> Result<usize, String> {
    fs::create_dir_all(dest_geoip).map_err(|e| format!("mkdir {}: {e}", dest_geoip.display()))?;
    if chown_to_root {
        chown_root(dest_geoip)?;
    }
    set_mode(dest_geoip, 0o755)?;
    if !src_geoip.is_dir() {
        return Ok(0);
    }
    let mut copied = 0usize;
    let entries =
        fs::read_dir(src_geoip).map_err(|e| format!("read {}: {e}", src_geoip.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read_dir {}: {e}", src_geoip.display()))?;
        let name = entry.file_name();
        if !name.to_string_lossy().ends_with(".srs") {
            continue;
        }
        let src = entry.path();
        reject_untrusted_source(&src, "geoip rule-set", allowed_uid)?;
        let dest = dest_geoip.join(&name);
        let _ = fs::remove_file(&dest);
        let mut reader = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&src)
            .map_err(|e| format!("open source {}: {e}", src.display()))?;
        let mut writer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&dest)
            .map_err(|e| format!("open dest {}: {e}", dest.display()))?;
        io::copy(&mut reader, &mut writer)
            .map_err(|e| format!("copy {} -> {}: {e}", src.display(), dest.display()))?;
        writer
            .sync_all()
            .map_err(|e| format!("sync {}: {e}", dest.display()))?;
        drop(writer);
        if chown_to_root {
            chown_root(&dest)?;
        }
        set_mode(&dest, 0o644)?;
        copied += 1;
    }
    Ok(copied)
}

/// Bundled resources live next to the source core (app `resources/`), not
/// next to the root-owned copy the helper executes.
fn resources_dir_from_core_src(core_src: &Path) -> PathBuf {
    core_src
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(CORE_BIN_DEST_DIR))
}

/// `launchctl bootstrap system <plist>`. `bootout` failure is ignored (first
/// install has nothing to boot out); `bootstrap` failure is fatal.
#[cfg(target_os = "macos")]
fn launchctl_bootstrap(plist: &Path) -> Result<(), String> {
    let bootout = Command::new("/bin/launchctl")
        .args(["bootout", "system"])
        .arg(plist)
        .output()
        .map_err(|e| format!("launchctl bootout: {e}"))?;
    if !bootout.status.success() {
        tracing::debug!(
            status = %bootout.status,
            "launchctl bootout had nothing to unload (expected on first install)"
        );
    }
    let bootstrap = Command::new("/bin/launchctl")
        .args(["bootstrap", "system"])
        .arg(plist)
        .output()
        .map_err(|e| format!("launchctl bootstrap: {e}"))?;
    if !bootstrap.status.success() {
        return Err(format!(
            "launchctl bootstrap failed: {}",
            String::from_utf8_lossy(&bootstrap.stderr).trim()
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_systemctl(args: &[&str]) -> Result<(), String> {
    let out = Command::new("systemctl")
        .args(args)
        .output()
        .map_err(|e| format!("systemctl {args:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "systemctl {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// Install the helper daemon as root. `core_src` is the bundled sing-box
/// binary; the helper itself is copied from the running executable
/// (`current_exe`), so the same binary installs itself.
pub fn install(data_dir: &Path, core_src: &Path, allowed_uid: u32) -> Result<(), String> {
    require_root()?;

    #[cfg(target_os = "linux")]
    if !Path::new("/run/systemd/system").is_dir() {
        return Err(
            "systemd is not running (/run/systemd/system not found); Linux TUN helper requires systemd"
                .to_string(),
        );
    }

    if !data_dir.is_dir() {
        return Err(format!("data dir not found: {}", data_dir.display()));
    }
    reject_untrusted_source(core_src, "core binary", allowed_uid)?;

    let helper_src = std::env::current_exe().map_err(|e| format!("resolve own executable: {e}"))?;
    reject_untrusted_source(&helper_src, "helper binary", allowed_uid)?;

    // Stop an already-running Linux systemd daemon before replacing binaries so
    // copying over `HELPER_BIN_DEST` never hits `ETXTBSY`.
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("systemctl")
            .args(["stop", SYSTEMD_UNIT_NAME])
            .output();
    }

    // 1. Per-installation token (installing-uid owned 0600 in the app data dir).
    let token = generate_token()?;
    let token_file = data_dir.join(TOKEN_FILE_NAME);
    write_user_owned_secret(&token_file, &format!("{token}\n"), allowed_uid)?;

    // 2. Helper binary, root-owned 0755.
    copy_root_owned(&helper_src, Path::new(HELPER_BIN_DEST), 0o755)?;

    // 3. Core binary into the root-owned directory, root-owned 0755.
    copy_root_owned(core_src, Path::new(CORE_BIN_DEST), 0o755)?;
    if let Some(dir) = Path::new(CORE_BIN_DEST).parent() {
        chown_root(dir)?;
        set_mode(dir, 0o755)?;
    }
    let core_sha256 = sha256_of_file(Path::new(CORE_BIN_DEST))?;

    // 3b. Root-owned run dir for the sanitised config (0700).
    let run_dir = Path::new(CORE_RUN_DIR);
    fs::create_dir_all(run_dir).map_err(|e| format!("mkdir {}: {e}", run_dir.display()))?;
    chown_root(run_dir)?;
    set_mode(run_dir, 0o700)?;

    // 4. Root-owned helper log; core log is owned by the installing uid so
    // the unelevated app can truncate it in place (CORE-7) without replacing
    // the path (the parent directory stays root-owned).
    {
        let helper_log = Path::new(HELPER_LOG_DEST);
        let _ = fs::remove_file(helper_log);
        fs::write(helper_log, "").map_err(|e| format!("create log {}: {e}", HELPER_LOG_DEST))?;
        chown_root(helper_log)?;
        set_mode(helper_log, 0o644)?;

        let core_log = Path::new(CORE_LOG_DEST);
        let _ = fs::remove_file(core_log);
        fs::write(core_log, "").map_err(|e| format!("create log {}: {e}", CORE_LOG_DEST))?;
        chown_uid(core_log, allowed_uid)?;
        set_mode(core_log, 0o644)?;
    }

    // 5. Service descriptor with pinned env, then activate daemon.
    #[cfg(target_os = "macos")]
    {
        let plist = render_plist(
            &token,
            data_dir,
            CORE_BIN_DEST,
            &core_sha256,
            CORE_LOG_DEST,
            allowed_uid,
            SOCKET_PATH,
            &resources_dir_from_core_src(core_src),
        );
        fs::write(PLIST_DEST, plist).map_err(|e| format!("write plist {PLIST_DEST}: {e}"))?;
        chown_root(Path::new(PLIST_DEST))?;
        set_mode(Path::new(PLIST_DEST), 0o644)?;
        launchctl_bootstrap(Path::new(PLIST_DEST))?;
    }

    #[cfg(target_os = "linux")]
    {
        // Copy bundled geoip `.srs` files into a root-owned fixed directory so
        // AppImage ephemeral `/tmp/.mount_*` paths remain valid across restarts.
        let src_resources = resources_dir_from_core_src(core_src);
        let dest_resources = Path::new(RESOURCES_DEST_DIR);
        fs::create_dir_all(dest_resources)
            .map_err(|e| format!("mkdir {}: {e}", dest_resources.display()))?;
        chown_root(dest_resources)?;
        set_mode(dest_resources, 0o755)?;
        let _ = copy_geoip_srs_files(
            &src_resources.join("geoip"),
            &dest_resources.join("geoip"),
            allowed_uid,
            true,
        )?;

        let unit = render_systemd_unit(
            &token,
            data_dir,
            CORE_BIN_DEST,
            &core_sha256,
            CORE_LOG_DEST,
            allowed_uid,
            SOCKET_PATH,
            dest_resources,
        )?;
        let unit_path = Path::new(SYSTEMD_UNIT_DEST);
        fs::write(unit_path, unit)
            .map_err(|e| format!("write unit {}: {e}", unit_path.display()))?;
        chown_root(unit_path)?;
        set_mode(unit_path, 0o644)?;
        run_systemctl(&["daemon-reload"])?;
        run_systemctl(&["enable", "--now", SYSTEMD_UNIT_NAME])?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (&token, &core_sha256);
        return Err("helper installation is only supported on macOS and Linux".to_string());
    }

    tracing::info!(
        data_dir = %data_dir.display(),
        core = CORE_BIN_DEST,
        "ice-helper installed"
    );
    Ok(())
}

/// Uninstall the helper daemon as root. Never touches routes / adapters /
/// DNS; removes only the files and the service job this installer owns.
pub fn uninstall(data_dir: &Path) -> Result<(), String> {
    require_root()?;

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("/bin/launchctl")
            .args(["bootout", "system"])
            .arg(PLIST_DEST)
            .output();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("systemctl")
            .args(["disable", "--now", SYSTEMD_UNIT_NAME])
            .output();
    }

    for path in [
        PathBuf::from(PLIST_DEST),
        PathBuf::from(HELPER_BIN_DEST),
        PathBuf::from(CORE_BIN_DEST),
        PathBuf::from(CORE_LOG_DEST),
        PathBuf::from(HELPER_LOG_DEST),
        PathBuf::from(SOCKET_PATH),
        data_dir.join(TOKEN_FILE_NAME),
        PathBuf::from(CORE_RUN_DIR).join("config.json"),
        PathBuf::from(CORE_RUN_DIR).join("cache.db"),
    ] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "uninstall: remove failed");
            }
        }
    }
    let _ = fs::remove_dir_all(PathBuf::from(CORE_RUN_DIR).join("rule-sets"));
    let _ = fs::remove_dir(Path::new(CORE_RUN_DIR));

    #[cfg(target_os = "linux")]
    {
        let _ = fs::remove_dir_all(Path::new(RESOURCES_DEST_DIR));
        let _ = Command::new("systemctl").args(["daemon-reload"]).output();
    }

    let _ = fs::remove_dir(Path::new(CORE_BIN_DEST_DIR));

    tracing::info!("ice-helper uninstalled");
    Ok(())
}

/// One-line result printed by the CLI modes; the app parses the prefix.
pub fn result_line(ok: bool, detail: &str) -> String {
    if ok {
        format!("OK {detail}")
    } else {
        format!("ERROR: {detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ice-helper-install-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn plist_pins_env_and_paths() {
        let plist = render_plist(
            "tok123",
            Path::new("/Users/u/Library/Application Support/com.yilong-musk.icebox"),
            CORE_BIN_DEST,
            "sha256abc",
            CORE_LOG_DEST,
            501,
            SOCKET_PATH,
            Path::new("/Applications/ice-box.app/Contents/Resources"),
        );
        assert!(plist.contains("<string>com.yilong-musk.icebox.helper</string>"));
        assert!(plist.contains("<string>tok123</string>"));
        assert!(plist.contains("/Users/u/Library/Application Support/com.yilong-musk.icebox"));
        assert!(plist.contains("<string>sha256abc</string>"));
        assert!(plist.contains("<string>501</string>"));
        assert!(plist.contains(&format!("<string>{SOCKET_PATH}</string>")));
        assert!(plist.contains("ICE_HELPER_CORE_BIN_SHA256"));
        assert!(plist.contains("ICE_HELPER_ALLOWED_UID"));
        // All env keys the daemon's load_config reads are present.
        for key in [
            ENV_TOKEN,
            ENV_DATA_DIR,
            ENV_CORE_BIN,
            ENV_CORE_BIN_SHA256,
            ENV_CORE_LOG,
            ENV_ALLOWED_UID,
            ENV_SOCKET,
            ENV_RESOURCES_DIR,
        ] {
            assert!(plist.contains(&format!("<key>{key}</key>")), "{key}");
        }
    }

    #[test]
    fn plist_escapes_user_controlled_values() {
        let plist = render_plist(
            "tok123",
            Path::new("/Users/u/Library/Application Support/a&b<c>\"d'e"),
            CORE_BIN_DEST,
            "sha256abc",
            CORE_LOG_DEST,
            501,
            SOCKET_PATH,
            Path::new("/Applications/ice-box.app/Contents/Resources"),
        );
        assert!(plist.contains("&amp;"));
        assert!(plist.contains("&lt;"));
        assert!(plist.contains("&gt;"));
        assert!(plist.contains("&quot;"));
        assert!(plist.contains("&apos;"));
        assert!(
            !plist.contains("a&b"),
            "raw ampersand must not reach the plist"
        );
        assert!(!plist.contains("<c>"), "raw angle brackets must be escaped");
    }

    #[test]
    fn writable_source_is_rejected() {
        let dir = temp_dir("writable");
        let world_writable = dir.join("core");
        fs::write(&world_writable, b"x").expect("write");
        fs::set_permissions(&world_writable, fs::Permissions::from_mode(0o666)).expect("mode");
        let err = reject_untrusted_source(&world_writable, "core binary", 0).expect_err("writable");
        assert!(err.contains("world-writable"));

        let non_regular = dir.join("sock");
        std::os::unix::net::UnixListener::bind(&non_regular).expect("bind");
        let err = reject_untrusted_source(&non_regular, "core binary", 0).expect_err("socket");
        assert!(err.contains("not a regular file"));

        let uid = unsafe { libc::getuid() };
        let owned = dir.join("owned");
        fs::write(&owned, b"x").expect("write");
        fs::set_permissions(&owned, fs::Permissions::from_mode(0o755)).expect("mode");
        reject_untrusted_source(&owned, "core binary", uid).expect("self-owned");

        let target = dir.join("target");
        fs::write(&target, b"x").expect("target");
        let link = dir.join("link");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        let err = reject_untrusted_source(&link, "core binary", uid).expect_err("symlink");
        assert!(err.contains("symlink"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sha256_is_hex_and_stable() {
        let dir = temp_dir("sha");
        let file = dir.join("f");
        fs::write(&file, b"hello").expect("write");
        let sum = sha256_of_file(&file).expect("sum");
        assert_eq!(sum.len(), 64);
        assert!(sum.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(sha256_of_file(&file).expect("again"), sum);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn token_is_hex_64_chars() {
        let token = generate_token().expect("token");
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn token_write_replaces_symlink_without_touching_target() {
        let dir = temp_dir("token-symlink");
        let target = dir.join("secret");
        fs::write(&target, b"keep-me").expect("target");
        let token_path = dir.join(TOKEN_FILE_NAME);
        std::os::unix::fs::symlink(&target, &token_path).expect("symlink");
        let uid = unsafe { libc::getuid() };
        write_user_owned_secret(&token_path, "tok\n", uid).expect("write");
        assert_eq!(fs::read(&target).expect("target intact"), b"keep-me");
        let meta = fs::symlink_metadata(&token_path).expect("meta");
        assert!(meta.file_type().is_file());
        assert!(!meta.file_type().is_symlink());
        assert_eq!(fs::read_to_string(&token_path).expect("token"), "tok\n");
        assert_eq!(meta.mode() & 0o777, 0o600);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn result_line_contract_is_parseable() {
        assert!(result_line(true, "installed").starts_with("OK "));
        assert!(result_line(false, "boom").starts_with("ERROR: "));
    }

    #[test]
    fn systemd_unit_pins_env_and_escapes_special_characters() {
        let unit = render_systemd_unit(
            "tok123",
            Path::new("/home/user%1/My \"Data\"\\dir"),
            CORE_BIN_DEST,
            "sha256abc",
            CORE_LOG_DEST,
            1000,
            SOCKET_PATH,
            Path::new(RESOURCES_DEST_DIR),
        )
        .expect("render");
        assert!(unit.contains(&format!("ExecStart={HELPER_BIN_DEST}")));
        assert!(unit.contains("Restart=no"));
        assert!(unit.contains(&format!("StandardOutput=append:{HELPER_LOG_DEST}")));
        assert!(unit.contains("ICE_HELPER_DATA_DIR=/home/user%%1/My \\\"Data\\\"\\\\dir"));
        for key in [
            ENV_TOKEN,
            ENV_DATA_DIR,
            ENV_CORE_BIN,
            ENV_CORE_BIN_SHA256,
            ENV_CORE_LOG,
            ENV_ALLOWED_UID,
            ENV_SOCKET,
            ENV_RESOURCES_DIR,
        ] {
            assert!(unit.contains(&format!("Environment=\"{key}=")), "{key}");
        }
        assert!(systemd_escape_env_value("bad\nline").is_err());
        assert!(systemd_escape_env_value("bad\rline").is_err());
    }

    #[test]
    fn copy_geoip_srs_files_copies_regular_files_and_rejects_symlinks() {
        let dir = temp_dir("geoip-copy");
        let src = dir.join("src_geoip");
        let dst = dir.join("dst_geoip");
        fs::create_dir_all(&src).unwrap();
        let cn = src.join("geoip-cn.srs");
        fs::write(&cn, b"srs-data").unwrap();
        fs::set_permissions(&cn, fs::Permissions::from_mode(0o644)).unwrap();
        fs::write(src.join("ignored.txt"), b"ignore").unwrap();

        let uid = unsafe { libc::getuid() };
        let count = copy_geoip_srs_files(&src, &dst, uid, false).expect("copy");
        assert_eq!(count, 1);
        assert_eq!(fs::read(dst.join("geoip-cn.srs")).unwrap(), b"srs-data");
        assert!(!dst.join("ignored.txt").exists());

        // Symlinked .srs must be rejected.
        let target = dir.join("secret");
        fs::write(&target, b"nope").unwrap();
        std::os::unix::fs::symlink(&target, src.join("evil.srs")).unwrap();
        let err = copy_geoip_srs_files(&src, &dst, uid, false).expect_err("symlink rejected");
        assert!(err.contains("symlink"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn constants_match_the_legacy_script_paths() {
        #[cfg(not(target_os = "linux"))]
        {
            assert_eq!(
                HELPER_BIN_DEST,
                "/Library/PrivilegedHelperTools/com.yilong-musk.icebox.helper"
            );
            assert_eq!(
                PLIST_DEST,
                "/Library/LaunchDaemons/com.yilong-musk.icebox.helper.plist"
            );
            assert_eq!(
                CORE_BIN_DEST,
                "/Library/PrivilegedHelperTools/com.yilong-musk.icebox/sing-box"
            );
            assert_eq!(SOCKET_PATH, "/var/run/ice-box-helper.sock");
        }
        #[cfg(target_os = "linux")]
        {
            assert_eq!(HELPER_BIN_DEST, "/usr/local/libexec/ice-box/ice-helper");
            assert_eq!(PLIST_DEST, "/etc/systemd/system/ice-box-helper.service");
            assert_eq!(CORE_BIN_DEST, "/usr/local/libexec/ice-box/sing-box");
            assert_eq!(SOCKET_PATH, "/run/ice-box-helper.sock");
            assert_eq!(CORE_RUN_DIR, "/usr/local/libexec/ice-box/run");
            assert_eq!(RESOURCES_DEST_DIR, "/usr/local/libexec/ice-box/resources");
        }
        assert_eq!(CORE_LOG_DEST, "/var/log/ice-box-core.log");
        assert_eq!(HELPER_LOG_DEST, "/var/log/ice-box-helper.log");
    }
}
