// SPDX-License-Identifier: GPL-3.0-or-later

//! Login item ("launch at login") registration.
//!
//! The desktop app ships unsigned — `SMAppService` is deliberately not part of
//! the product (see `CHANGELOG.md`, 0.1.1) — so the macOS login item is a
//! per-user LaunchAgent plist, the same mechanism the bundled helper uses one
//! level up for its root daemon. Windows uses the per-user
//! `HKCU\...\CurrentVersion\Run` value, which matches the per-user NSIS
//! install: no elevation, no machine-wide state. Linux writes an XDG Autostart
//! `.desktop` file under `${XDG_CONFIG_HOME:-$HOME/.config}/autostart/` and
//! prefers `$APPIMAGE` when running from an AppImage so the login item does not
//! point at an ephemeral `/tmp/.mount_*` path.
//!
//! All registrations carry [`AUTOSTART_FLAG`], so the process knows it was
//! started by the login item and stays in the tray instead of opening the
//! window. The registered command line points at the running executable, which
//! can move between installs and versions, so the shell rewrites the entry on
//! every launch while `settings.json` says it is on.
//!
//! `settings.json` stays authoritative: enabling writes the OS entry first and
//! only then records the flag, so a refused write (read-only login items
//! directory, locked registry) is never remembered as enabled.

use std::path::Path;

/// Appended to the registered command line; marks a login-item launch.
pub const AUTOSTART_FLAG: &str = "--autostart";

/// Whether this build can register an OS login item. The Settings card is
/// hidden where it cannot, so the switch is never an error-only control.
pub const SUPPORTED: bool = cfg!(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "linux"
));

/// Whether this process was started by the login item.
pub fn is_autostart_launch() -> bool {
    // `args_os`, not `args`: a non-Unicode argv entry makes `env::args` panic,
    // and a login-item launch must never depend on how the shell was invoked.
    has_autostart_flag(std::env::args_os())
}

/// Pure flag scan (unit-tested; [`is_autostart_launch`] passes `env::args_os`).
fn has_autostart_flag<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    args.into_iter()
        .any(|arg| arg.as_ref() == std::ffi::OsStr::new(AUTOSTART_FLAG))
}

/// Return the enclosing `.AppImage` path only when `current_exe` actually runs
/// from this process's own AppImage mount.
///
/// Desktop terminals spawned inside another AppImage (such as an IDE's
/// integrated terminal) inherit the parent's `$APPIMAGE` / `$APPDIR`. Verifying
/// that `current_exe` sits under `$APPDIR` (or under `/tmp/.mount_` when
/// `$APPDIR` is absent) prevents a `.deb` install launched from such a terminal
/// from recording a foreign AppImage path.
#[cfg(any(target_os = "linux", test))]
pub fn appimage_path_for_exe(
    appimage: Option<&std::ffi::OsStr>,
    appdir: Option<&std::ffi::OsStr>,
    current_exe: &Path,
) -> Option<std::path::PathBuf> {
    let appimage = appimage.filter(|v| !v.is_empty())?;
    let exe_in_mount = match appdir.filter(|v| !v.is_empty()) {
        Some(dir) => path_is_under(current_exe, Path::new(dir)),
        None => is_appimage_mount_path(current_exe),
    };
    exe_in_mount.then(|| std::path::PathBuf::from(appimage))
}

#[cfg(any(target_os = "linux", test))]
fn path_is_under(exe: &Path, dir: &Path) -> bool {
    if let (Ok(canon_exe), Ok(canon_dir)) = (exe.canonicalize(), dir.canonicalize()) {
        return canon_dir.parent().is_some() && canon_exe.starts_with(&canon_dir);
    }
    dir.parent().is_some() && exe.starts_with(dir)
}

#[cfg(any(target_os = "linux", test))]
fn is_appimage_mount_path(exe: &Path) -> bool {
    let candidate = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    candidate.to_string_lossy().starts_with("/tmp/.mount_")
}

/// Resolve the stable executable path to record in the login item.
///
/// On Linux AppImage launches, `current_exe()` points inside the ephemeral
/// `/tmp/.mount_*` FUSE mount that vanishes on exit; `$APPIMAGE` carries the
/// real `.AppImage` file path on disk when verified against `$APPDIR` /
/// `current_exe`.
#[cfg(any(target_os = "linux", test))]
fn resolve_linux_executable(
    appimage_env: Option<&std::ffi::OsStr>,
    appdir_env: Option<&std::ffi::OsStr>,
    current_exe: &Path,
) -> std::path::PathBuf {
    appimage_path_for_exe(appimage_env, appdir_env, current_exe)
        .unwrap_or_else(|| current_exe.to_path_buf())
}

/// Register (or drop) the login item for this process's executable.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let current =
        std::env::current_exe().map_err(|err| format!("resolve the running executable: {err}"))?;
    #[cfg(target_os = "linux")]
    let exe = resolve_linux_executable(
        std::env::var_os("APPIMAGE").as_deref(),
        std::env::var_os("APPDIR").as_deref(),
        &current,
    );
    #[cfg(not(target_os = "linux"))]
    let exe = current;
    imp::set_enabled(&exe, enabled)
}

/// Escape the XML metacharacters a filesystem path may contain. `plist` text
/// nodes have no CDATA shortcut, so `<string>` values are escaped instead.
#[cfg(any(target_os = "macos", test))]
fn xml_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Reverse-DNS label of the macOS login item, matching the app's bundle
/// identifier. The plist file name below is what `launchd` scans in
/// `~/Library/LaunchAgents` at login.
#[cfg(any(target_os = "macos", test))]
const LAUNCH_AGENT_LABEL: &str = "com.yilong-musk.icebox";

/// `~/Library/LaunchAgents/<bundle id>.plist` for `home`.
#[cfg(any(target_os = "macos", test))]
fn launch_agent_path(home: &Path) -> std::path::PathBuf {
    home.join("Library")
        .join("LaunchAgents")
        .join(format!("{LAUNCH_AGENT_LABEL}.plist"))
}

/// Directory macOS mounts a quarantined disk-image launch from, read-only and
/// with a fresh random path per launch.
#[cfg(any(target_os = "macos", test))]
const APP_TRANSLOCATION_DIR: &str = "AppTranslocation";

/// True when `exe` sits inside a macOS App Translocation mount. A login item
/// registered for such a path breaks silently once that mount is gone, and the
/// app cannot repair it from there, so the registration is refused instead.
#[cfg(any(target_os = "macos", test))]
fn is_translocated(exe: &Path) -> bool {
    exe.components()
        .any(|component| component.as_os_str() == std::ffi::OsStr::new(APP_TRANSLOCATION_DIR))
}

/// LaunchAgent that runs `exe` once per login, in the GUI session only.
///
/// `KeepAlive` is false on purpose: the agent starts the app at login and then
/// forgets about it, so quitting from the tray stays quit instead of becoming a
/// relaunch loop. `LimitLoadToSessionType` keeps the entry out of non-GUI
/// sessions (`ssh`, background contexts), where a window app has nothing to
/// attach to.
#[cfg(any(target_os = "macos", test))]
fn render_launch_agent(exe: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LAUNCH_AGENT_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>{AUTOSTART_FLAG}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <false/>
  <key>LimitLoadToSessionType</key>
  <string>Aqua</string>
  <key>ProcessType</key>
  <string>Interactive</string>
</dict>
</plist>
"#,
        xml_escape(&exe.display().to_string())
    )
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn home_dir() -> Result<PathBuf, String> {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "HOME is not set".to_string())
    }

    pub fn set_enabled(exe: &Path, enabled: bool) -> Result<(), String> {
        // Checked before anything is written: an app opened from the disk image
        // runs from a read-only mount whose path the next launch will not have,
        // so the entry would be dead on arrival while `settings.json` still
        // claimed launch at login.
        if enabled && is_translocated(exe) {
            return Err(
                "ice-box is running from the read-only macOS App Translocation path; \
                 move it to /Applications (or another writable location) and turn \
                 launch at login on again"
                    .to_string(),
            );
        }
        let path = launch_agent_path(&home_dir()?);
        if !enabled {
            return match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(format!("remove {}: {err}", path.display())),
            };
        }
        let dir = path
            .parent()
            .ok_or_else(|| format!("no parent directory for {}", path.display()))?;
        fs::create_dir_all(dir).map_err(|err| format!("create {}: {err}", dir.display()))?;
        // Write beside the target and rename, so a crash mid-write cannot leave
        // `launchd` a truncated plist to parse at the next login.
        let staging = path.with_extension("plist.tmp");
        fs::write(&staging, render_launch_agent(exe))
            .map_err(|err| format!("write {}: {err}", staging.display()))?;
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&staging, fs::Permissions::from_mode(0o644))
                .map_err(|err| format!("chmod {}: {err}", staging.display()))?;
        }
        fs::rename(&staging, &path).map_err(|err| {
            let _ = fs::remove_file(&staging);
            format!("install {}: {err}", path.display())
        })
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    };

    /// Per-user startup key; the per-user NSIS install never needs elevation
    /// to reach it.
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// Value name; the NSIS uninstall hook deletes the same name.
    const RUN_VALUE: &str = "ice-box";

    /// NUL-terminated UTF-16, the encoding every `W` registry API expects.
    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// Open the per-user startup key, creating it only when `create` is set.
    /// `Ok(None)` means the key does not exist, so there is nothing to remove.
    fn open_run_key(create: bool) -> Result<Option<HKEY>, String> {
        let subkey = wide(RUN_KEY);
        let mut key: HKEY = std::ptr::null_mut();
        let status = unsafe {
            if create {
                let mut disposition = 0;
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    subkey.as_ptr(),
                    0,
                    std::ptr::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE,
                    std::ptr::null(),
                    &mut key,
                    &mut disposition,
                )
            } else {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    subkey.as_ptr(),
                    0,
                    KEY_SET_VALUE,
                    &mut key,
                )
            }
        };
        if status != ERROR_SUCCESS {
            // Nothing to remove is the state the caller asked for: a fresh
            // profile may never have created the key.
            if !create && status == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            return Err(format!("open HKCU\\{RUN_KEY} (Windows error {status})"));
        }
        Ok(Some(key))
    }

    pub fn set_enabled(exe: &Path, enabled: bool) -> Result<(), String> {
        let Some(key) = open_run_key(enabled)? else {
            // The key (and with it the value) is already gone.
            return Ok(());
        };
        let name = wide(RUN_VALUE);
        let status: WIN32_ERROR = if enabled {
            let data = wide(&run_value_data(exe));
            unsafe {
                RegSetValueExW(
                    key,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    data.as_ptr().cast::<u8>(),
                    (data.len() * std::mem::size_of::<u16>()) as u32,
                )
            }
        } else {
            let status = unsafe { RegDeleteValueW(key, name.as_ptr()) };
            // A missing value is the state the caller asked for.
            if status == ERROR_FILE_NOT_FOUND {
                ERROR_SUCCESS
            } else {
                status
            }
        };
        unsafe { RegCloseKey(key) };
        if status != ERROR_SUCCESS {
            return Err(format!(
                "update HKCU\\{RUN_KEY}\\{RUN_VALUE} (Windows error {status})"
            ));
        }
        Ok(())
    }
}

/// Desktop entry file stem matching the app's reverse-DNS bundle identifier.
#[cfg(any(target_os = "linux", test))]
const XDG_DESKTOP_ENTRY_ID: &str = "com.yilong-musk.icebox";

/// Resolve `${XDG_CONFIG_HOME:-$HOME/.config}/autostart/<id>.desktop`.
///
/// Per the XDG Base Directory Specification, a relative or empty
/// `XDG_CONFIG_HOME` is ignored in favour of `$HOME/.config`.
#[cfg(any(target_os = "linux", test))]
fn xdg_autostart_path(
    xdg_config_home: Option<&Path>,
    home: Option<&Path>,
) -> Result<std::path::PathBuf, String> {
    let config_root = match xdg_config_home {
        // XDG paths are POSIX paths starting with `/`; checking the prefix
        // keeps host-free tests consistent on Windows where `/custom/cfg` is
        // not considered absolute by `Path::is_absolute`.
        Some(path) if path.is_absolute() || path.to_string_lossy().starts_with('/') => {
            path.to_path_buf()
        }
        _ => home
            .ok_or_else(|| "HOME is not set".to_string())?
            .join(".config"),
    };
    Ok(config_root
        .join("autostart")
        .join(format!("{XDG_DESKTOP_ENTRY_ID}.desktop")))
}

/// Escape an executable path for a double-quoted argument in a `.desktop`
/// `Exec=` key according to the FreeDesktop Desktop Entry Specification.
#[cfg(any(target_os = "linux", test))]
fn desktop_exec_escape(value: &str) -> String {
    // 1. Exec quoting inside `"..."` reserves `"`, `` ` ``, `$`, `\` and `%`.
    let mut exec_quoted = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' | '"' | '`' | '$' => {
                exec_quoted.push('\\');
                exec_quoted.push(ch);
            }
            '%' => exec_quoted.push_str("%%"),
            _ => exec_quoted.push(ch),
        }
    }
    // 2. Desktop Entry string-value escaping runs before Exec parsing.
    let mut out = String::with_capacity(exec_quoted.len());
    for ch in exec_quoted.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

/// Render an XDG Autostart `.desktop` entry that launches `exe` with
/// [`AUTOSTART_FLAG`].
#[cfg(any(target_os = "linux", test))]
fn render_desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name=ice-box\n\
         Comment=Lightweight sing-box proxy client\n\
         Exec=\"{}\" {AUTOSTART_FLAG}\n\
         Icon=ice-box\n\
         Terminal=false\n\
         StartupNotify=false\n\
         X-GNOME-Autostart-enabled=true\n",
        desktop_exec_escape(&exe.display().to_string())
    )
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    pub fn set_enabled_at(path: &Path, exe: &Path, enabled: bool) -> Result<(), String> {
        if !enabled {
            return match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(format!("remove {}: {err}", path.display())),
            };
        }
        let dir = path
            .parent()
            .ok_or_else(|| format!("no parent directory for {}", path.display()))?;
        fs::create_dir_all(dir).map_err(|err| format!("create {}: {err}", dir.display()))?;
        let staging = path.with_extension("desktop.tmp");
        fs::write(&staging, render_desktop_entry(exe))
            .map_err(|err| format!("write {}: {err}", staging.display()))?;
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&staging, fs::Permissions::from_mode(0o644))
                .map_err(|err| format!("chmod {}: {err}", staging.display()))?;
        }
        fs::rename(&staging, path).map_err(|err| {
            let _ = fs::remove_file(&staging);
            format!("install {}: {err}", path.display())
        })
    }

    pub fn set_enabled(exe: &Path, enabled: bool) -> Result<(), String> {
        let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let path = xdg_autostart_path(xdg.as_deref(), home.as_deref())?;
        set_enabled_at(&path, exe, enabled)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod imp {
    use super::*;

    pub fn set_enabled(_exe: &Path, _enabled: bool) -> Result<(), String> {
        Err("launch at login is only supported on macOS, Windows, and Linux".to_string())
    }
}

/// Command line the Windows startup key runs at login: the quoted executable
/// (install paths contain spaces) plus the tray-only flag.
#[cfg(any(target_os = "windows", test))]
fn run_value_data(exe: &Path) -> String {
    format!("\"{}\" {AUTOSTART_FLAG}", exe.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn autostart_flag_is_recognized_anywhere_on_the_command_line() {
        assert!(has_autostart_flag(vec![AUTOSTART_FLAG.to_string()]));
        assert!(has_autostart_flag(vec![
            "ice-box".to_string(),
            AUTOSTART_FLAG.to_string(),
        ]));
        assert!(!has_autostart_flag(Vec::<&str>::new()));
        assert!(!has_autostart_flag(vec!["--other".to_string()]));
    }

    #[test]
    fn windows_run_value_quotes_the_executable() {
        let data = run_value_data(Path::new(r"C:\Program Files\ice-box\ice-box.exe"));
        assert_eq!(
            data,
            r#""C:\Program Files\ice-box\ice-box.exe" --autostart"#
        );
    }

    #[test]
    fn launch_agent_path_is_the_bundle_identifier_under_launch_agents() {
        let path = launch_agent_path(Path::new("/Users/example"));
        assert_eq!(
            path,
            PathBuf::from("/Users/example/Library/LaunchAgents/com.yilong-musk.icebox.plist")
        );
    }

    #[test]
    fn launch_agent_runs_the_executable_with_the_tray_flag_once_per_login() {
        let plist = render_launch_agent(Path::new(
            "/Applications/ice-box.app/Contents/MacOS/ice-box",
        ));
        assert!(plist.contains("<string>com.yilong-musk.icebox</string>"));
        assert!(plist.contains("<string>/Applications/ice-box.app/Contents/MacOS/ice-box</string>"));
        assert!(plist.contains("<string>--autostart</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(
            plist.contains("<key>KeepAlive</key>\n  <false/>"),
            "quitting at the login session must not become a relaunch loop"
        );
    }

    #[test]
    fn launch_agent_escapes_xml_metacharacters_in_the_executable_path() {
        let plist = render_launch_agent(Path::new("/Applications/a&b/<c>/ice-box"));
        assert!(plist.contains("<string>/Applications/a&amp;b/&lt;c&gt;/ice-box</string>"));
        assert!(!plist.contains("a&b"));
    }

    #[test]
    fn app_translocation_mounts_are_detected_by_path_component() {
        assert!(is_translocated(Path::new(
            "/private/var/folders/xy/1234567890/T/AppTranslocation/1A2B3C/d/ice-box.app/Contents/MacOS/ice-box"
        )));
        assert!(is_translocated(Path::new(
            "/AppTranslocation/1A2B3C/d/ice-box"
        )));
        assert!(!is_translocated(Path::new(
            "/Applications/ice-box.app/Contents/MacOS/ice-box"
        )));
        // A path that merely contains the word is not a translocation mount.
        assert!(!is_translocated(Path::new(
            "/Applications/MyAppTranslocationHolder/ice-box"
        )));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn translocated_instance_is_refused_before_the_login_item_is_written() {
        let translocated = Path::new(
            "/private/var/folders/xy/1234567890/T/AppTranslocation/1A2B3C/d/ice-box.app/Contents/MacOS/ice-box",
        );
        let err = imp::set_enabled(translocated, true).expect_err("must be refused");
        assert!(
            err.contains("/Applications"),
            "the refusal must say where to install: {err}"
        );
    }

    #[test]
    fn linux_executable_prefers_verified_appimage_env_and_rejects_inherited_appimage() {
        let mount_exe = Path::new("/tmp/.mount_ice-bo1234/usr/bin/ice-box");
        let appdir = std::ffi::OsStr::new("/tmp/.mount_ice-bo1234");
        let appimage = std::ffi::OsStr::new("/home/u/Apps/ice-box.AppImage");

        // Genuine AppImage with matching APPDIR.
        assert_eq!(
            resolve_linux_executable(Some(appimage), Some(appdir), mount_exe),
            PathBuf::from("/home/u/Apps/ice-box.AppImage")
        );
        // Genuine AppImage without APPDIR falls back to `/tmp/.mount_` check.
        assert_eq!(
            resolve_linux_executable(Some(appimage), None, mount_exe),
            PathBuf::from("/home/u/Apps/ice-box.AppImage")
        );
        // Empty or absent APPIMAGE keeps current_exe.
        assert_eq!(
            resolve_linux_executable(Some(std::ffi::OsStr::new("")), Some(appdir), mount_exe),
            mount_exe
        );
        assert_eq!(
            resolve_linux_executable(None, Some(appdir), mount_exe),
            mount_exe
        );

        // `.deb` install launched from another AppImage's terminal (foreign
        // $APPIMAGE and $APPDIR) must keep `/usr/bin/ice-box`.
        let deb_exe = Path::new("/usr/bin/ice-box");
        let foreign_appimage = std::ffi::OsStr::new("/home/u/Apps/Cursor.AppImage");
        let foreign_appdir = std::ffi::OsStr::new("/tmp/.mount_Cursor999");
        assert_eq!(
            resolve_linux_executable(Some(foreign_appimage), Some(foreign_appdir), deb_exe),
            deb_exe
        );
        assert_eq!(
            resolve_linux_executable(Some(foreign_appimage), None, deb_exe),
            deb_exe
        );
        // Root APPDIR must never falsely match arbitrary executables.
        assert_eq!(
            resolve_linux_executable(
                Some(foreign_appimage),
                Some(std::ffi::OsStr::new("/")),
                deb_exe
            ),
            deb_exe
        );
    }

    #[test]
    fn xdg_autostart_path_honors_absolute_xdg_config_home_and_falls_back_to_home() {
        assert_eq!(
            xdg_autostart_path(Some(Path::new("/custom/cfg")), Some(Path::new("/home/u"))).unwrap(),
            PathBuf::from("/custom/cfg")
                .join("autostart")
                .join("com.yilong-musk.icebox.desktop")
        );
        assert_eq!(
            xdg_autostart_path(Some(Path::new("relative/cfg")), Some(Path::new("/home/u")))
                .unwrap(),
            PathBuf::from("/home/u")
                .join(".config")
                .join("autostart")
                .join("com.yilong-musk.icebox.desktop")
        );
        assert_eq!(
            xdg_autostart_path(None, Some(Path::new("/home/u"))).unwrap(),
            PathBuf::from("/home/u")
                .join(".config")
                .join("autostart")
                .join("com.yilong-musk.icebox.desktop")
        );
        assert!(xdg_autostart_path(None, None).is_err());
    }

    #[test]
    fn desktop_entry_renders_and_escapes_special_characters() {
        let entry = render_desktop_entry(Path::new("/home/u/My Apps/ice$box%1.AppImage"));
        assert!(entry.contains("[Desktop Entry]"));
        assert!(entry.contains("Exec=\"/home/u/My Apps/ice\\\\$box%%1.AppImage\" --autostart"));
        assert!(entry.contains("X-GNOME-Autostart-enabled=true"));
        assert!(entry.contains("Terminal=false"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_set_enabled_at_writes_and_removes_desktop_entry() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-autostart-linux-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("autostart/com.yilong-musk.icebox.desktop");
        let exe = Path::new("/usr/bin/ice-box");

        imp::set_enabled_at(&path, exe, true).expect("enable");
        let content = std::fs::read_to_string(&path).expect("read");
        assert!(content.contains("Exec=\"/usr/bin/ice-box\" --autostart"));

        imp::set_enabled_at(&path, exe, false).expect("disable");
        assert!(!path.exists());
        imp::set_enabled_at(&path, exe, false).expect("disable idempotent");
        let _ = std::fs::remove_dir_all(dir);
    }
}
