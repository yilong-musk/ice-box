// SPDX-License-Identifier: GPL-3.0-or-later

//! Privileged install / uninstall modes for the helper daemon.
//!
//! These modes run **as root** and are the only thing the unsigned elevation
//! path (macOS AuthorizationServices / Linux Polkit `pkexec`, `ice-elevate`) is
//! allowed to execute; the desktop process itself never runs elevated. They
//! replace the old shell-script installer so there is exactly one
//! implementation of the installation logic (service descriptor, token,
//! ownership, pinned hash), shared by the app's in-app installer and the
//! manual/CI scripts.
//!
//! Security model (`docs/tun.md`):
//!
//! - The install must be started as root ([`require_root`]).
//! - Source inputs are opened with `O_NOFOLLOW | O_NONBLOCK` and verified via
//!   `fstat` on the open descriptor (regular file, owned by root or the
//!   installing uid, no group/world write). Copies and SHA-256 computation
//!   read from that same file descriptor, eliminating path TOCTOU races.
//! - Root-owned binaries, geoip files, systemd unit, and the Linux root-only
//!   `EnvironmentFile` (`/etc/ice-box/helper.env`, `0600`) are staged into
//!   sibling temp files (`O_CREAT|O_EXCL|O_NOFOLLOW`, `fchown`, `fchmod`,
//!   `sync_all`) and atomically renamed into place, never truncating an active
//!   binary in place.
//! - The installed core's SHA-256 is pinned in the launchd plist (macOS) or
//!   systemd unit (Linux); the daemon refuses to start when the on-disk binary
//!   does not match.
//! - The per-installation token is generated here (inside the elevated
//!   process, from `/dev/urandom`) and written `0600` owned by the
//!   installing uid. The name is unlinked first, then created exclusive
//!   (`O_CREAT|O_EXCL|O_NOFOLLOW`); ownership is applied on the open fd
//!   (`fchown` / `fchmod`) so a pre-planted symlink in the user data dir
//!   cannot redirect the privileged write or chown. On Linux the daemon's copy
//!   of the token lives in the root-only `0600` `EnvironmentFile`, never in
//!   the `0644` unit file.
//! - Logs are recreated exclusively (`O_CREAT|O_EXCL|O_NOFOLLOW`) so a stale
//!   symlink can never become the target of a privileged append.

use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(any(target_os = "macos", test))]
pub use ice_tun_helper_proto::install_paths::LAUNCHD_LABEL;
pub use ice_tun_helper_proto::install_paths::{
    CORE_BIN_DEST, CORE_BIN_DEST_DIR, CORE_LOG_DEST, CORE_RUN_DIR, ENV_ALLOWED_UID, ENV_CORE_BIN,
    ENV_CORE_BIN_SHA256, ENV_CORE_LOG, ENV_DATA_DIR, ENV_RESOURCES_DIR, ENV_SOCKET, ENV_TOKEN,
    HELPER_BIN_DEST, HELPER_LOG_DEST, PLIST_DEST, SOCKET_PATH, TOKEN_FILE_NAME,
};
#[cfg(any(target_os = "linux", test))]
pub use ice_tun_helper_proto::install_paths::{
    HELPER_ENV_DEST, HELPER_ENV_DIR, RESOURCES_DEST_DIR,
};
#[cfg(target_os = "linux")]
pub use ice_tun_helper_proto::install_paths::{
    RESOLV_CONF_BACKUP_DEST, SYSTEMD_UNIT_DEST, SYSTEMD_UNIT_NAME,
};

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

/// Open a source file with `O_NOFOLLOW | O_NONBLOCK` and verify via `fstat` on
/// the open descriptor that it is a regular file, owned by root or
/// `allowed_uid`, and not group/world-writable. Returning the open `File`
/// lets callers copy and hash directly from the validated fd with no TOCTOU
/// window.
fn open_trusted_source(path: &Path, what: &str, allowed_uid: u32) -> Result<fs::File, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| match e.raw_os_error() {
            Some(libc::ELOOP) => format!("{what} must not be a symlink: {}", path.display()),
            Some(libc::ENXIO) | Some(libc::EOPNOTSUPP) => {
                format!("{what} is not a regular file: {}", path.display())
            }
            _ => format!("open {what} {}: {e}", path.display()),
        })?;
    let meta = file
        .metadata()
        .map_err(|e| format!("fstat {what} {}: {e}", path.display()))?;
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
    Ok(file)
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
    write_user_owned_secret_bytes(path, contents.as_bytes(), uid)
}

fn write_user_owned_secret_bytes(path: &Path, contents: &[u8], uid: u32) -> Result<(), String> {
    let _ = fs::remove_file(path);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| format!("create token {}: {e}", path.display()))?;
    file.write_all(contents)
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

/// Recreate a fixed log file exclusively (`O_CREAT|O_EXCL|O_NOFOLLOW`) so a
/// stale symlink can never become the target of a privileged write or chown.
fn recreate_log_file(path: &Path, owner_uid: u32, mode: u32) -> Result<(), String> {
    let _ = fs::remove_file(path);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| format!("create log {}: {e}", path.display()))?;
    let fd = file.as_raw_fd();
    let rc = unsafe { libc::fchown(fd, owner_uid, 0) };
    if rc != 0 {
        return Err(format!(
            "fchown uid {owner_uid} {}: {}",
            path.display(),
            io::Error::last_os_error()
        ));
    }
    let rc = unsafe { libc::fchmod(fd, mode as libc::mode_t) };
    if rc != 0 {
        return Err(format!(
            "fchmod {mode:o} {}: {}",
            path.display(),
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Guard that unlinks a temporary sibling file on error before atomic rename.
struct TempFileGuard {
    path: Option<PathBuf>,
}

impl TempFileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if let Some(ref path) = self.path {
            let _ = fs::remove_file(path);
        }
    }
}

/// Atomically write `dest` by staging into a sibling temp file created with
/// `O_CREAT | O_EXCL | O_NOFOLLOW`, applying `fchown` (when `chown_to_root`)
/// and `fchmod`, calling `sync_all`, and renaming over `dest`.
///
/// Never opens `dest` with `O_TRUNC`, so a running binary or active config file
/// is never truncated in place (preventing `ETXTBSY` and half-written reads).
fn write_atomic_file<T, F>(
    dest: &Path,
    mode: u32,
    chown_to_root: bool,
    write_fn: F,
) -> Result<T, String>
where
    F: FnOnce(&mut fs::File) -> Result<T, String>,
{
    let parent = dest
        .parent()
        .ok_or_else(|| format!("destination has no parent directory: {}", dest.display()))?;
    fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    let file_name = dest
        .file_name()
        .ok_or_else(|| format!("destination has no file name: {}", dest.display()))?
        .to_string_lossy();

    let pid = std::process::id();
    let mut attempt = 0u32;
    let (tmp_path, mut tmp_file) = loop {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let candidate = parent.join(format!(".{file_name}.tmp-{pid}-{nanos}-{attempt}"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists && attempt < 16 => {
                attempt += 1;
            }
            Err(err) => {
                return Err(format!("create temp file {}: {err}", candidate.display()));
            }
        }
    };

    let mut guard = TempFileGuard::new(tmp_path.clone());
    let value = write_fn(&mut tmp_file)?;

    let fd = tmp_file.as_raw_fd();
    if chown_to_root {
        let rc = unsafe { libc::fchown(fd, 0, 0) };
        if rc != 0 {
            return Err(format!(
                "fchown root {}: {}",
                tmp_path.display(),
                io::Error::last_os_error()
            ));
        }
    }
    let rc = unsafe { libc::fchmod(fd, mode as libc::mode_t) };
    if rc != 0 {
        return Err(format!(
            "fchmod {mode:o} {}: {}",
            tmp_path.display(),
            io::Error::last_os_error()
        ));
    }
    tmp_file
        .sync_all()
        .map_err(|e| format!("sync {}: {e}", tmp_path.display()))?;
    drop(tmp_file);

    fs::rename(&tmp_path, dest)
        .map_err(|e| format!("rename {} -> {}: {e}", tmp_path.display(), dest.display()))?;
    guard.disarm();
    Ok(value)
}

fn write_atomic_bytes(
    dest: &Path,
    contents: &[u8],
    mode: u32,
    chown_to_root: bool,
) -> Result<(), String> {
    write_atomic_file(dest, mode, chown_to_root, |file| {
        file.write_all(contents)
            .map_err(|e| format!("write {}: {e}", dest.display()))
    })
}

/// Stream bytes from an already-validated open `src` file into a newly staged
/// sibling temp file, computing SHA-256 over the exact copied stream, then
/// atomically rename onto `dest`.
fn copy_root_owned_from_fd(
    src: &mut fs::File,
    src_desc: &Path,
    dest: &Path,
    mode: u32,
    chown_to_root: bool,
) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    write_atomic_file(dest, mode, chown_to_root, |dst| {
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = src
                .read(&mut buf)
                .map_err(|e| format!("read {}: {e}", src_desc.display()))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            dst.write_all(&buf[..n])
                .map_err(|e| format!("copy {} -> {}: {e}", src_desc.display(), dest.display()))?;
        }
        Ok(format!("{:x}", hasher.finalize()))
    })
}

/// Validate that `dir` is an absolute path to an existing directory and return
/// its canonical path.
fn canonicalize_absolute_dir(dir: &Path, what: &str) -> Result<PathBuf, String> {
    if !dir.is_absolute() {
        return Err(format!(
            "{what} must be an absolute path: {}",
            dir.display()
        ));
    }
    if !dir.is_dir() {
        return Err(format!("{what} not found: {}", dir.display()));
    }
    fs::canonicalize(dir).map_err(|e| format!("canonicalize {what} {}: {e}", dir.display()))
}

/// Check whether the canonicalized source helper path resolves to the installed
/// destination path itself (re-installing from `HELPER_BIN_DEST` risks
/// self-overwrite / `ETXTBSY`).
fn is_same_installed_helper(helper_src_canon: &Path, helper_dest: &Path) -> bool {
    let dest_canon = fs::canonicalize(helper_dest).unwrap_or_else(|_| helper_dest.to_path_buf());
    helper_src_canon == dest_canon
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

/// Render the root-only systemd `EnvironmentFile` (`0600`) carrying the helper
/// auth token so the `0644` unit file never exposes it to other local users.
#[cfg(any(target_os = "linux", test))]
pub fn render_systemd_env_file(token: &str) -> Result<String, String> {
    if token.chars().any(|c| matches!(c, '\n' | '\r' | '\0')) {
        return Err("systemd env file value contains forbidden control character".to_string());
    }
    let mut escaped = String::with_capacity(token.len());
    for ch in token.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            _ => escaped.push(ch),
        }
    }
    Ok(format!("{ENV_TOKEN}=\"{escaped}\"\n"))
}

/// Render the Linux systemd unit that pins data dir / core binary (with
/// SHA-256) / log / authorized uid / socket / resources dir and loads the
/// secret token from `EnvironmentFile={HELPER_ENV_DEST}`.
#[cfg(any(target_os = "linux", test))]
#[allow(clippy::too_many_arguments)]
pub fn render_systemd_unit(
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
    let core_bin_str = systemd_escape_env_value(core_bin_dest)?;
    let core_sha_str = systemd_escape_env_value(core_bin_sha256)?;
    let core_log_str = systemd_escape_env_value(core_log)?;
    let socket_str = systemd_escape_env_value(socket)?;
    Ok(format!(
        "[Unit]\n\
         Description=ice-box privileged TUN helper daemon\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={HELPER_BIN_DEST}\n\
         Restart=on-failure\n\
         RestartSec=2\n\
         StandardOutput=append:{HELPER_LOG_DEST}\n\
         StandardError=append:{HELPER_LOG_DEST}\n\
         EnvironmentFile={HELPER_ENV_DEST}\n\
         Environment=\"{ENV_DATA_DIR}={data_dir_str}\"\n\
         Environment=\"{ENV_CORE_BIN}={core_bin_str}\"\n\
         Environment=\"{ENV_CORE_BIN_SHA256}={core_sha_str}\"\n\
         Environment=\"{ENV_CORE_LOG}={core_log_str}\"\n\
         Environment=\"{ENV_ALLOWED_UID}={allowed_uid}\"\n\
         Environment=\"{ENV_SOCKET}={socket_str}\"\n\
         Environment=\"{ENV_RESOURCES_DIR}={resources_dir_str}\"\n\
         NoNewPrivileges=yes\n\
         PrivateTmp=yes\n\
         ProtectControlGroups=yes\n\
         ProtectClock=yes\n\
         ProtectHostname=yes\n\
         RestrictSUIDSGID=yes\n\
         LockPersonality=yes\n\
         RestrictRealtime=yes\n\
         SystemCallArchitectures=native\n\
         RestrictNamespaces=yes\n\
         CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_RAW CAP_NET_BIND_SERVICE CAP_KILL CAP_CHOWN CAP_FOWNER CAP_DAC_OVERRIDE CAP_DAC_READ_SEARCH\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n"
    ))
}

/// Copy bundled `geoip/*.srs` files from `src_geoip` into `dest_geoip`,
/// validating each file via `open_trusted_source` (O_NOFOLLOW + fstat) before
/// staging atomically so the root helper's fixed resources directory never
/// depends on an ephemeral AppImage mount or exposes partial files.
#[cfg(any(target_os = "linux", test))]
fn copy_geoip_srs_files(
    src_geoip: &Path,
    dest_geoip: &Path,
    allowed_uid: u32,
    chown_to_root: bool,
) -> Result<usize, String> {
    let mut validated: Vec<(std::ffi::OsString, PathBuf, fs::File)> = Vec::new();
    if src_geoip.is_dir() {
        let entries =
            fs::read_dir(src_geoip).map_err(|e| format!("read {}: {e}", src_geoip.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("read_dir {}: {e}", src_geoip.display()))?;
            let name = entry.file_name();
            if !name.to_string_lossy().ends_with(".srs") {
                continue;
            }
            let src = entry.path();
            let reader = open_trusted_source(&src, "geoip rule-set", allowed_uid)?;
            validated.push((name, src, reader));
        }
    }

    fs::create_dir_all(dest_geoip).map_err(|e| format!("mkdir {}: {e}", dest_geoip.display()))?;
    if chown_to_root {
        chown_root(dest_geoip)?;
    }
    set_mode(dest_geoip, 0o755)?;

    let mut copied = 0usize;
    for (name, src, mut reader) in validated {
        let dest = dest_geoip.join(&name);
        copy_root_owned_from_fd(&mut reader, &src, &dest, 0o644, chown_to_root)?;
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

#[cfg(target_os = "linux")]
fn systemctl_is_active(unit: &str) -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", unit])
        .status()
        .is_ok_and(|s| s.success())
}

/// Hard-link backup of an existing installed binary in the same directory so a
/// failed activation can atomically restore the previous inode (matching the
/// previous unit's pinned SHA-256). Always unlinks the backup path on drop.
#[cfg(any(target_os = "linux", test))]
struct PrevFileBackup {
    dest: PathBuf,
    backup: Option<PathBuf>,
}

#[cfg(any(target_os = "linux", test))]
impl PrevFileBackup {
    fn capture(dest: &Path) -> Option<Self> {
        if !dest.is_file() {
            return None;
        }
        let parent = dest.parent()?;
        let name = dest.file_name()?.to_string_lossy();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let backup = parent.join(format!(".{name}.prev-{pid}-{nanos}"));
        let _ = fs::remove_file(&backup);
        fs::hard_link(dest, &backup).ok()?;
        Some(Self {
            dest: dest.to_path_buf(),
            backup: Some(backup),
        })
    }

    fn restore(&mut self) {
        if let Some(backup) = self.backup.take() {
            if let Err(err) = fs::rename(&backup, &self.dest) {
                tracing::warn!(
                    dest = %self.dest.display(),
                    error = %err,
                    "rollback: failed to restore previous binary"
                );
                let _ = fs::remove_file(&backup);
            }
        }
    }
}

#[cfg(any(target_os = "linux", test))]
impl Drop for PrevFileBackup {
    fn drop(&mut self) {
        if let Some(ref backup) = self.backup {
            let _ = fs::remove_file(backup);
        }
    }
}

fn remove_file_if_present(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            tracing::warn!(path = %path.display(), error = %err, "uninstall: remove failed");
        }
    }
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

    // 1. Validate inputs, open trusted source fds (O_NOFOLLOW + fstat), and
    //    canonicalize paths before making any filesystem or service mutations.
    let data_dir = canonicalize_absolute_dir(data_dir, "data dir")?;
    if !core_src.is_absolute() {
        return Err(format!(
            "core binary path must be absolute: {}",
            core_src.display()
        ));
    }
    let mut core_file = open_trusted_source(core_src, "core binary", allowed_uid)?;
    let core_src = fs::canonicalize(core_src)
        .map_err(|e| format!("canonicalize core binary {}: {e}", core_src.display()))?;

    let helper_src_raw =
        std::env::current_exe().map_err(|e| format!("resolve own executable: {e}"))?;
    let mut helper_file = open_trusted_source(&helper_src_raw, "helper binary", allowed_uid)?;
    let helper_src = fs::canonicalize(&helper_src_raw).map_err(|e| {
        format!(
            "canonicalize helper binary {}: {e}",
            helper_src_raw.display()
        )
    })?;
    if is_same_installed_helper(&helper_src, Path::new(HELPER_BIN_DEST)) {
        return Err(format!(
            "refusing to install from the installed helper path itself: {}",
            helper_src.display()
        ));
    }

    let token = generate_token()?;

    #[cfg(target_os = "macos")]
    {
        // 1. Per-installation token (installing-uid owned 0600 in the app data dir).
        let token_file = data_dir.join(TOKEN_FILE_NAME);
        write_user_owned_secret(&token_file, &format!("{token}\n"), allowed_uid)?;

        // 2. Helper binary, root-owned 0755 (atomic temp + rename).
        copy_root_owned_from_fd(
            &mut helper_file,
            &helper_src,
            Path::new(HELPER_BIN_DEST),
            0o755,
            true,
        )?;

        // 3. Core binary into the root-owned directory, root-owned 0755.
        let core_sha256 = copy_root_owned_from_fd(
            &mut core_file,
            &core_src,
            Path::new(CORE_BIN_DEST),
            0o755,
            true,
        )?;
        if let Some(dir) = Path::new(CORE_BIN_DEST).parent() {
            chown_root(dir)?;
            set_mode(dir, 0o755)?;
        }

        // 3b. Root-owned run dir for the sanitised config (0700).
        let run_dir = Path::new(CORE_RUN_DIR);
        fs::create_dir_all(run_dir).map_err(|e| format!("mkdir {}: {e}", run_dir.display()))?;
        chown_root(run_dir)?;
        set_mode(run_dir, 0o700)?;

        // 4. Recreate log files exclusively so stale symlinks are never followed.
        recreate_log_file(Path::new(HELPER_LOG_DEST), 0, 0o644)?;
        recreate_log_file(Path::new(CORE_LOG_DEST), allowed_uid, 0o644)?;

        // 5. Launchd plist with pinned env, then activate daemon.
        let plist = render_plist(
            &token,
            &data_dir,
            CORE_BIN_DEST,
            &core_sha256,
            CORE_LOG_DEST,
            allowed_uid,
            SOCKET_PATH,
            &resources_dir_from_core_src(&core_src),
        );
        write_atomic_bytes(Path::new(PLIST_DEST), plist.as_bytes(), 0o644, true)?;
        launchctl_bootstrap(Path::new(PLIST_DEST))?;
    }

    #[cfg(target_os = "linux")]
    {
        // Pre-validate the secret env file before touching anything on disk.
        let env_file_contents = render_systemd_env_file(&token)?;

        // 2. Prepare root directories and stage geoip + binaries atomically
        //    BEFORE stopping any running daemon or overwriting the user's
        //    working token. Atomic rename avoids ETXTBSY while the old daemon
        //    is still executing.
        let core_dir = Path::new(CORE_BIN_DEST_DIR);
        fs::create_dir_all(core_dir).map_err(|e| format!("mkdir {}: {e}", core_dir.display()))?;
        chown_root(core_dir)?;
        set_mode(core_dir, 0o755)?;

        let run_dir = Path::new(CORE_RUN_DIR);
        fs::create_dir_all(run_dir).map_err(|e| format!("mkdir {}: {e}", run_dir.display()))?;
        chown_root(run_dir)?;
        set_mode(run_dir, 0o700)?;

        // Copy bundled geoip `.srs` files first so an untrusted/broken rule-set
        // fails before existing binaries, service, or token are touched.
        let src_resources = resources_dir_from_core_src(&core_src);
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

        // Capture hard-link backups of the previous binaries so rollback can
        // restore the exact inode matching the previous unit's pinned SHA-256.
        let mut prev_helper = PrevFileBackup::capture(Path::new(HELPER_BIN_DEST));
        let mut prev_core = PrevFileBackup::capture(Path::new(CORE_BIN_DEST));

        copy_root_owned_from_fd(
            &mut helper_file,
            &helper_src,
            Path::new(HELPER_BIN_DEST),
            0o755,
            true,
        )?;

        let core_sha256 = match copy_root_owned_from_fd(
            &mut core_file,
            &core_src,
            Path::new(CORE_BIN_DEST),
            0o755,
            true,
        ) {
            Ok(sha) => sha,
            Err(err) => {
                if let Some(ref mut backup) = prev_helper {
                    backup.restore();
                }
                return Err(err);
            }
        };

        let unit = match render_systemd_unit(
            &data_dir,
            CORE_BIN_DEST,
            &core_sha256,
            CORE_LOG_DEST,
            allowed_uid,
            SOCKET_PATH,
            dest_resources,
        ) {
            Ok(unit) => unit,
            Err(err) => {
                if let Some(ref mut backup) = prev_core {
                    backup.restore();
                }
                if let Some(ref mut backup) = prev_helper {
                    backup.restore();
                }
                return Err(err);
            }
        };

        // 3. Snapshot previous unit/env/token state, stop old service, install
        //    env + unit atomically, reload systemd, write user token, and
        //    enable --now. Roll back on activation failure.
        let unit_path = Path::new(SYSTEMD_UNIT_DEST);
        let env_dir = Path::new(HELPER_ENV_DIR);
        let env_path = Path::new(HELPER_ENV_DEST);
        let token_file = data_dir.join(TOKEN_FILE_NAME);

        let prev_unit = fs::read(unit_path).ok();
        let prev_env = fs::read(env_path).ok();
        let prev_token = fs::read(&token_file).ok();
        let was_active = systemctl_is_active(SYSTEMD_UNIT_NAME);

        let _ = Command::new("systemctl")
            .args(["stop", SYSTEMD_UNIT_NAME])
            .output();

        let activate = || -> Result<(), String> {
            recreate_log_file(Path::new(HELPER_LOG_DEST), 0, 0o644)?;
            recreate_log_file(Path::new(CORE_LOG_DEST), allowed_uid, 0o644)?;

            fs::create_dir_all(env_dir).map_err(|e| format!("mkdir {}: {e}", env_dir.display()))?;
            chown_root(env_dir)?;
            set_mode(env_dir, 0o755)?;
            write_atomic_bytes(env_path, env_file_contents.as_bytes(), 0o600, true)?;
            write_atomic_bytes(unit_path, unit.as_bytes(), 0o644, true)?;

            run_systemctl(&["daemon-reload"])?;
            write_user_owned_secret(&token_file, &format!("{token}\n"), allowed_uid)?;
            run_systemctl(&["enable", "--now", SYSTEMD_UNIT_NAME])?;
            Ok(())
        };

        if let Err(err) = activate() {
            tracing::warn!(error = %err, "install activation failed; attempting rollback");
            if let Some(ref mut backup) = prev_core {
                backup.restore();
            }
            if let Some(ref mut backup) = prev_helper {
                backup.restore();
            }
            match prev_env {
                Some(bytes) => {
                    let _ = write_atomic_bytes(env_path, &bytes, 0o600, true);
                }
                None => {
                    let _ = fs::remove_file(env_path);
                }
            }
            if let Some(bytes) = &prev_unit {
                let _ = write_atomic_bytes(unit_path, bytes, 0o644, true);
            }
            if let Some(bytes) = &prev_token {
                let _ = write_user_owned_secret_bytes(&token_file, bytes, allowed_uid);
            }
            let _ = Command::new("systemctl").args(["daemon-reload"]).output();
            if was_active && prev_unit.is_some() {
                if let Err(restart_err) = run_systemctl(&["start", SYSTEMD_UNIT_NAME]) {
                    tracing::warn!(
                        error = %restart_err,
                        "rollback: failed to restart previous helper unit"
                    );
                }
            }
            return Err(err);
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (&token, &mut core_file, &mut helper_file);
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
        match Command::new("systemctl")
            .args(["disable", "--now", SYSTEMD_UNIT_NAME])
            .output()
        {
            Ok(out) if !out.status.success() => {
                tracing::warn!(
                    status = %out.status,
                    stderr = %String::from_utf8_lossy(&out.stderr).trim(),
                    "uninstall: systemctl disable --now failed"
                );
            }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "uninstall: failed to execute systemctl disable --now"
                );
            }
            Ok(_) => {}
        }
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
        remove_file_if_present(&path);
    }
    let _ = fs::remove_dir_all(PathBuf::from(CORE_RUN_DIR).join("rule-sets"));
    #[cfg(target_os = "linux")]
    {
        match ice_helper::restore_static_resolv_conf_backup() {
            Ok(true) => {
                tracing::info!("restored static /etc/resolv.conf from backup during uninstall");
            }
            Ok(false) => {
                // Remove any stale backup file (e.g., when /etc/resolv.conf was
                // modified externally) so CORE_RUN_DIR can be removed cleanly.
                remove_file_if_present(Path::new(RESOLV_CONF_BACKUP_DEST));
            }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "uninstall: failed to restore static /etc/resolv.conf backup"
                );
            }
        }
    }
    let _ = fs::remove_dir(Path::new(CORE_RUN_DIR));

    #[cfg(target_os = "linux")]
    {
        remove_file_if_present(Path::new(HELPER_ENV_DEST));
        let _ = fs::remove_dir(Path::new(HELPER_ENV_DIR));
        let _ = fs::remove_dir_all(Path::new(RESOURCES_DEST_DIR));
        let _ = Command::new("systemctl").args(["daemon-reload"]).output();
        let _ = Command::new("systemctl")
            .args(["reset-failed", SYSTEMD_UNIT_NAME])
            .output();
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
        let err = open_trusted_source(&world_writable, "core binary", 0).expect_err("writable");
        assert!(err.contains("world-writable"));

        let non_regular = dir.join("sock");
        std::os::unix::net::UnixListener::bind(&non_regular).expect("bind");
        let err = open_trusted_source(&non_regular, "core binary", 0).expect_err("socket");
        assert!(err.contains("not a regular file"));

        let subdir = dir.join("subdir");
        fs::create_dir(&subdir).expect("subdir");
        let err = open_trusted_source(&subdir, "core binary", 0).expect_err("dir");
        assert!(err.contains("not a regular file"));

        let uid = unsafe { libc::getuid() };
        let owned = dir.join("owned");
        fs::write(&owned, b"x").expect("write");
        fs::set_permissions(&owned, fs::Permissions::from_mode(0o755)).expect("mode");
        open_trusted_source(&owned, "core binary", uid).expect("self-owned");

        let target = dir.join("target");
        fs::write(&target, b"x").expect("target");
        let link = dir.join("link");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        let err = open_trusted_source(&link, "core binary", uid).expect_err("symlink");
        assert!(err.contains("symlink"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_from_fd_atomic_stages_hashes_and_replaces_without_truncating_old_inode() {
        let dir = temp_dir("atomic-copy");
        let src_v1 = dir.join("src-v1");
        let src_v2 = dir.join("src-v2");
        let dest = dir.join("dest-bin");
        fs::write(&src_v1, b"version-one-bytes").expect("v1");
        fs::write(&src_v2, b"version-two-longer-bytes").expect("v2");
        fs::set_permissions(&src_v1, fs::Permissions::from_mode(0o755)).expect("mode1");
        fs::set_permissions(&src_v2, fs::Permissions::from_mode(0o755)).expect("mode2");

        let uid = unsafe { libc::getuid() };
        let mut f1 = open_trusted_source(&src_v1, "core binary", uid).expect("open v1");
        let sha1 = copy_root_owned_from_fd(&mut f1, &src_v1, &dest, 0o755, false).expect("copy1");
        assert_eq!(sha1, sha256_of_file(&dest).expect("sha1"));
        assert_eq!(fs::metadata(&dest).unwrap().mode() & 0o777, 0o755);

        // Hold an open reader on the v1 inode while replacing dest with v2;
        // atomic rename keeps the old inode intact instead of truncating it.
        let mut open_v1 = fs::File::open(&dest).expect("hold v1 fd");
        let mut backup = PrevFileBackup::capture(&dest).expect("capture backup");

        let mut f2 = open_trusted_source(&src_v2, "core binary", uid).expect("open v2");
        let sha2 = copy_root_owned_from_fd(&mut f2, &src_v2, &dest, 0o755, false).expect("copy2");
        assert_eq!(sha2, sha256_of_file(&dest).expect("sha2"));
        assert_ne!(sha1, sha2);

        let mut old_bytes = Vec::new();
        open_v1.read_to_end(&mut old_bytes).expect("read old inode");
        assert_eq!(old_bytes, b"version-one-bytes");
        assert_eq!(fs::read(&dest).unwrap(), b"version-two-longer-bytes");

        // Rollback restores the v1 inode atomically.
        backup.restore();
        assert_eq!(fs::read(&dest).unwrap(), b"version-one-bytes");
        assert_eq!(sha256_of_file(&dest).unwrap(), sha1);

        // No leftover temp/backup files in dir.
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "leftover temp files: {leftovers:?}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn canonicalize_absolute_dir_and_self_install_checks() {
        let dir = temp_dir("canon-check");
        assert!(canonicalize_absolute_dir(Path::new("relative/dir"), "data dir").is_err());
        let canon = canonicalize_absolute_dir(&dir, "data dir").expect("canon");
        assert!(canon.is_absolute());

        let helper = canon.join("ice-helper");
        fs::write(&helper, b"bin").expect("helper");
        let helper_canon = fs::canonicalize(&helper).expect("helper canon");
        assert!(is_same_installed_helper(&helper_canon, &helper));
        assert!(!is_same_installed_helper(
            &helper_canon,
            &canon.join("other-helper")
        ));

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
    fn systemd_unit_pins_env_hardening_and_escapes_special_characters() {
        let unit = render_systemd_unit(
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
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("RestartSec=2"));
        assert!(!unit.contains("network-online.target"));
        assert!(unit.contains(&format!("StandardOutput=append:{HELPER_LOG_DEST}")));
        assert!(unit.contains(&format!("EnvironmentFile={HELPER_ENV_DEST}")));
        // Secret token must live only in the 0600 EnvironmentFile, never in the 0644 unit.
        assert!(!unit.contains(ENV_TOKEN));
        assert!(unit.contains("ICE_HELPER_DATA_DIR=/home/user%%1/My \\\"Data\\\"\\\\dir"));
        for key in [
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
        for directive in [
            "NoNewPrivileges=yes",
            "PrivateTmp=yes",
            "ProtectControlGroups=yes",
            "ProtectClock=yes",
            "ProtectHostname=yes",
            "RestrictSUIDSGID=yes",
            "LockPersonality=yes",
            "RestrictRealtime=yes",
            "SystemCallArchitectures=native",
            "RestrictNamespaces=yes",
            "CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_RAW CAP_NET_BIND_SERVICE CAP_KILL CAP_CHOWN CAP_FOWNER CAP_DAC_OVERRIDE CAP_DAC_READ_SEARCH",
        ] {
            assert!(unit.contains(directive), "missing hardening: {directive}");
        }
        for forbidden in [
            "ProtectSystem=strict",
            "ProtectHome=",
            "ProtectKernelTunables=",
            "MemoryDenyWriteExecute=",
        ] {
            assert!(
                !unit.contains(forbidden),
                "unexpected directive: {forbidden}"
            );
        }
        assert!(systemd_escape_env_value("bad\nline").is_err());
        assert!(systemd_escape_env_value("bad\rline").is_err());
    }

    #[test]
    fn systemd_env_file_formats_token_and_rejects_control_chars() {
        let env = render_systemd_env_file("abcdef0123456789").expect("render env");
        assert_eq!(env, format!("{ENV_TOKEN}=\"abcdef0123456789\"\n"));

        let escaped = render_systemd_env_file("a\"b\\c").expect("escaped");
        assert_eq!(escaped, format!("{ENV_TOKEN}=\"a\\\"b\\\\c\"\n"));

        assert!(render_systemd_env_file("bad\nline").is_err());
        assert!(render_systemd_env_file("bad\rline").is_err());
        assert!(render_systemd_env_file("bad\0nul").is_err());
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

        // Symlinked .srs must be rejected before copying.
        let target = dir.join("secret");
        fs::write(&target, b"nope").unwrap();
        std::os::unix::fs::symlink(&target, src.join("evil.srs")).unwrap();
        let err = copy_geoip_srs_files(&src, &dst, uid, false).expect_err("symlink rejected");
        assert!(err.contains("symlink"));
        assert!(!dst.join("evil.srs").exists());
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
        assert_eq!(HELPER_ENV_DIR, "/etc/ice-box");
        assert_eq!(HELPER_ENV_DEST, "/etc/ice-box/helper.env");
        assert_eq!(CORE_LOG_DEST, "/var/log/ice-box-core.log");
        assert_eq!(HELPER_LOG_DEST, "/var/log/ice-box-helper.log");
    }
}
