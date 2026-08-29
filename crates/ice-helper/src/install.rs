//! Privileged install / uninstall modes for the helper daemon.
//!
//! These modes run **as root** and are the only thing the unsigned elevation
//! path (AuthorizationServices password dialog, `ice-elevate`) is allowed to
//! execute; the desktop process itself never runs elevated. They replace the
//! old shell-script installer so there is exactly one implementation of the
//! installation logic (plist, token, ownership, pinned hash), shared by the
//! app's in-app installer and the manual/CI script.
//!
//! Security model (unchanged from the script, plan §7):
//!
//! - The install must be started as root ([`require_root`]).
//! - The source core binary must be a regular file a non-root user cannot
//!   replace (group/world-write refused); only a root-owned copy is ever
//!   executed elevated.
//! - The installed core's SHA-256 is pinned in the launchd plist; the daemon
//!   refuses to start when the on-disk binary does not match.
//! - The per-installation token is generated here (inside the elevated
//!   process, from `/dev/urandom`) and written root-owned 0644 into the app
//!   data dir.
//! - Logs are recreated as root-owned fixed files so a stale symlink can
//!   never become the target of a privileged append.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

pub use ice_tun_sys::install_paths::{
    CORE_BIN_DEST, CORE_BIN_DEST_DIR, CORE_LOG_DEST, ENV_ALLOWED_UID, ENV_CORE_BIN,
    ENV_CORE_BIN_SHA256, ENV_CORE_LOG, ENV_DATA_DIR, ENV_SOCKET, ENV_TOKEN, HELPER_BIN_DEST,
    HELPER_LOG_DEST, LAUNCHD_LABEL, PLIST_DEST, SOCKET_PATH, TOKEN_FILE_NAME,
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

/// Refuse a source binary that any non-root user could replace: it must be a
/// regular file without group/world write bits.
fn reject_writable_source(path: &Path, what: &str) -> Result<(), String> {
    let meta = fs::metadata(path).map_err(|e| format!("stat {what} {}: {e}", path.display()))?;
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

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|e| format!("chmod {mode:o} {}: {e}", path.display()))
}

fn copy_root_owned(source: &Path, dest: &Path, mode: u32) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    fs::copy(source, dest)
        .map_err(|e| format!("copy {} -> {}: {e}", source.display(), dest.display()))?;
    chown_root(dest)?;
    set_mode(dest, mode)
}

/// Render the launchd plist that pins token / data dir / core binary (with
/// SHA-256) / log / authorized uid / socket. Host-free and tested.
pub fn render_plist(
    token: &str,
    data_dir: &Path,
    core_bin_dest: &str,
    core_bin_sha256: &str,
    core_log: &str,
    allowed_uid: u32,
    socket: &str,
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
  </dict>
  <key>StandardOutPath</key>
  <string>{HELPER_LOG_DEST}</string>
  <key>StandardErrorPath</key>
  <string>{HELPER_LOG_DEST}</string>
</dict>
</plist>
"#,
        data_dir.display(),
    )
}

/// `launchctl bootstrap system <plist>`. `bootout` failure is ignored (first
/// install has nothing to boot out); `bootstrap` failure is fatal.
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

/// Install the helper daemon as root. `core_src` is the bundled sing-box
/// binary; the helper itself is copied from the running executable
/// (`current_exe`), so the same binary installs itself.
pub fn install(data_dir: &Path, core_src: &Path, allowed_uid: u32) -> Result<(), String> {
    require_root()?;

    if !data_dir.is_dir() {
        return Err(format!("data dir not found: {}", data_dir.display()));
    }
    reject_writable_source(core_src, "core binary")?;

    let helper_src = std::env::current_exe().map_err(|e| format!("resolve own executable: {e}"))?;
    if !helper_src.is_file() {
        return Err(format!("helper binary not found: {}", helper_src.display()));
    }

    // 1. Per-installation token (root-owned 0644 in the app data dir).
    let token = generate_token()?;
    let token_file = data_dir.join(TOKEN_FILE_NAME);
    fs::write(&token_file, format!("{token}\n"))
        .map_err(|e| format!("write token {}: {e}", token_file.display()))?;
    chown_root(&token_file)?;
    set_mode(&token_file, 0o644)?;

    // 2. Helper binary, root-owned 0755.
    copy_root_owned(&helper_src, Path::new(HELPER_BIN_DEST), 0o755)?;

    // 3. Core binary into the root-owned directory, root-owned 0755.
    copy_root_owned(core_src, Path::new(CORE_BIN_DEST), 0o755)?;
    if let Some(dir) = Path::new(CORE_BIN_DEST).parent() {
        chown_root(dir)?;
        set_mode(dir, 0o755)?;
    }
    let core_sha256 = sha256_of_file(Path::new(CORE_BIN_DEST))?;

    // 4. Root-owned fixed log files (stale symlink can never be appended to).
    for log in [CORE_LOG_DEST, HELPER_LOG_DEST] {
        let _ = fs::remove_file(log);
        fs::write(log, "").map_err(|e| format!("create log {log}: {e}"))?;
        chown_root(Path::new(log))?;
        set_mode(Path::new(log), 0o644)?;
    }

    // 5. Plist with pinned env, then launchctl bootstrap.
    let plist = render_plist(
        &token,
        data_dir,
        CORE_BIN_DEST,
        &core_sha256,
        CORE_LOG_DEST,
        allowed_uid,
        SOCKET_PATH,
    );
    fs::write(PLIST_DEST, plist).map_err(|e| format!("write plist {PLIST_DEST}: {e}"))?;
    chown_root(Path::new(PLIST_DEST))?;
    set_mode(Path::new(PLIST_DEST), 0o644)?;
    launchctl_bootstrap(Path::new(PLIST_DEST))?;

    tracing::info!(
        data_dir = %data_dir.display(),
        core = CORE_BIN_DEST,
        "ice-helper installed"
    );
    Ok(())
}

/// Uninstall the helper daemon as root. Never touches routes / adapters /
/// DNS; removes only the files and the launchd job this installer owns.
pub fn uninstall(data_dir: &Path) -> Result<(), String> {
    require_root()?;

    let _ = Command::new("/bin/launchctl")
        .args(["bootout", "system"])
        .arg(PLIST_DEST)
        .output();

    for path in [
        PathBuf::from(PLIST_DEST),
        PathBuf::from(HELPER_BIN_DEST),
        PathBuf::from(CORE_BIN_DEST),
        PathBuf::from(CORE_LOG_DEST),
        PathBuf::from(HELPER_LOG_DEST),
        PathBuf::from(SOCKET_PATH),
        data_dir.join(TOKEN_FILE_NAME),
    ] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "uninstall: remove failed");
            }
        }
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
        );
        assert!(plist.contains("<string>com.yilong-musk.icebox.helper</string>"));
        assert!(plist.contains("<string>tok123</string>"));
        assert!(plist.contains("/Users/u/Library/Application Support/com.yilong-musk.icebox"));
        assert!(plist.contains("<string>sha256abc</string>"));
        assert!(plist.contains("<string>501</string>"));
        assert!(plist.contains("<string>/var/run/ice-box-helper.sock</string>"));
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
        ] {
            assert!(plist.contains(&format!("<key>{key}</key>")), "{key}");
        }
    }

    #[test]
    fn writable_source_is_rejected() {
        let dir = temp_dir("writable");
        let world_writable = dir.join("core");
        fs::write(&world_writable, b"x").expect("write");
        fs::set_permissions(&world_writable, fs::Permissions::from_mode(0o666)).expect("mode");
        let err = reject_writable_source(&world_writable, "core binary").expect_err("writable");
        assert!(err.contains("world-writable"));

        let non_regular = dir.join("sock");
        std::os::unix::net::UnixListener::bind(&non_regular).expect("bind");
        let err = reject_writable_source(&non_regular, "core binary").expect_err("socket");
        assert!(err.contains("not a regular file"));

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
    fn result_line_contract_is_parseable() {
        assert!(result_line(true, "installed").starts_with("OK "));
        assert!(result_line(false, "boom").starts_with("ERROR: "));
    }

    #[test]
    fn constants_match_the_legacy_script_paths() {
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
        assert_eq!(CORE_LOG_DEST, "/var/log/ice-box-core.log");
        assert_eq!(HELPER_LOG_DEST, "/var/log/ice-box-helper.log");
    }
}
