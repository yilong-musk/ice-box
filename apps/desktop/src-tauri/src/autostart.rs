// SPDX-License-Identifier: GPL-3.0-or-later

//! Login item ("launch at login") registration.
//!
//! The desktop app ships unsigned — `SMAppService` is deliberately not part of
//! the product (see `CHANGELOG.md`, 0.1.1) — so the macOS login item is a
//! per-user LaunchAgent plist, the same mechanism the bundled helper uses one
//! level up for its root daemon. Windows uses the per-user
//! `HKCU\...\CurrentVersion\Run` value, which matches the per-user NSIS
//! install: no elevation, no machine-wide state.
//!
//! Both registrations carry [`AUTOSTART_FLAG`], so the process knows it was
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

/// Register (or drop) the login item for this process's executable.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let exe =
        std::env::current_exe().map_err(|err| format!("resolve the running executable: {err}"))?;
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

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod imp {
    use super::*;

    pub fn set_enabled(_exe: &Path, _enabled: bool) -> Result<(), String> {
        Err("launch at login is only supported on macOS and Windows".to_string())
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
}
