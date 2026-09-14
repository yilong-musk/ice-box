// SPDX-License-Identifier: GPL-3.0-or-later

//! Open a new interactive terminal with Mixed proxy env for *this session only*.
//!
//! Env is injected into the new process (or via a one-shot `export` / `$env:` in
//! the launch command). Never uses User/Machine environment APIs, or shell
//! profile edits.

use ice_config::{AppError, ErrorCode};
use std::io::Write;
use std::process::{Command, Stdio};

const NO_PROXY: &str = "localhost,127.0.0.1,::1";

/// Loopback rewrite for unspecified Mixed listens (terminal clients on this host).
pub(crate) fn shell_proxy_host(listen: &str) -> String {
    let host = listen.trim();
    if host.is_empty() || host == "0.0.0.0" || host == "::" {
        return "127.0.0.1".into();
    }
    host.to_string()
}

fn wrap_host_for_url(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

/// Upper + lower case proxy vars so curl, git, and most CLIs pick them up.
pub(crate) fn proxy_env_pairs(host: &str, port: u16) -> Vec<(String, String)> {
    let wrapped = wrap_host_for_url(host);
    let http = format!("http://{wrapped}:{port}");
    let socks = format!("socks5://{wrapped}:{port}");
    vec![
        ("HTTP_PROXY".into(), http.clone()),
        ("HTTPS_PROXY".into(), http.clone()),
        ("http_proxy".into(), http.clone()),
        ("https_proxy".into(), http.clone()),
        ("ALL_PROXY".into(), socks.clone()),
        ("all_proxy".into(), socks),
        ("NO_PROXY".into(), NO_PROXY.into()),
        ("no_proxy".into(), NO_PROXY.into()),
    ]
}

#[cfg(not(target_os = "macos"))]
fn apply_envs(cmd: &mut Command, envs: &[(String, String)]) {
    for (key, value) in envs {
        cmd.env(key, value);
    }
}

/// `cmd /k` prelude using `&&` only.
///
/// Windows Terminal treats `;` as a *wt* command separator, so a PowerShell
/// `$env:A=...; $env:B=...` string must never appear on the `wt.exe` command
/// line — it splits into multiple bogus launches (0x80070002).
#[cfg(any(test, windows))]
fn cmd_session_prelude(envs: &[(String, String)]) -> String {
    let sets = envs
        .iter()
        .map(|(k, v)| format!("set \"{k}={v}\""))
        .collect::<Vec<_>>()
        .join("&&");
    // Hand off to PowerShell in the same tab; env is inherited from cmd.
    format!("{sets}&& powershell -NoExit -NoLogo")
}

fn spawn_err(context: &str, err: std::io::Error) -> AppError {
    AppError::new(
        ErrorCode::ConfigInvalid,
        format!("open proxy terminal ({context}): {err}"),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellProxyPlatform {
    Windows,
    Posix,
}

pub(crate) fn current_shell_proxy_platform() -> ShellProxyPlatform {
    if cfg!(windows) {
        ShellProxyPlatform::Windows
    } else {
        ShellProxyPlatform::Posix
    }
}

/// One line for the *current terminal session only* (same text Home copies).
pub(crate) fn format_shell_proxy_command(
    platform: ShellProxyPlatform,
    host: &str,
    port: u16,
) -> String {
    let wrapped = wrap_host_for_url(host);
    let http = format!("http://{wrapped}:{port}");
    let socks = format!("socks5://{wrapped}:{port}");
    if platform == ShellProxyPlatform::Windows {
        format!(
            "$env:HTTP_PROXY='{http}'; $env:HTTPS_PROXY='{http}'; $env:ALL_PROXY='{socks}'; $env:NO_PROXY='{NO_PROXY}'"
        )
    } else {
        format!("export http_proxy={http} https_proxy={http} all_proxy={socks} no_proxy={NO_PROXY}")
    }
}

pub(crate) fn copy_shell_proxy_command(host: &str, port: u16) -> Result<(), AppError> {
    if port == 0 {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            "copy proxy command: invalid mixed port",
        ));
    }
    copy_text(&format_shell_proxy_command(
        current_shell_proxy_platform(),
        host,
        port,
    ))
}

fn copy_text(text: &str) -> Result<(), AppError> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut child = Command::new("cmd")
            .args(["/c", "clip"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| spawn_err("clipboard", e))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| spawn_err("clipboard", e))?;
        }
        let status = child.wait().map_err(|e| spawn_err("clipboard", e))?;
        if status.success() {
            Ok(())
        } else {
            Err(AppError::new(
                ErrorCode::ConfigInvalid,
                "copy proxy command: clip failed",
            ))
        }
    }
    #[cfg(target_os = "macos")]
    {
        pipe_to_clipboard(Command::new("pbcopy"), text, "pbcopy")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for (program, args, label) in [
            ("wl-copy", &[] as &[&str], "wl-copy"),
            ("xclip", &["-selection", "clipboard"] as &[&str], "xclip"),
            ("xsel", &["--clipboard", "--input"] as &[&str], "xsel"),
        ] {
            let mut cmd = Command::new(program);
            cmd.args(args);
            if pipe_to_clipboard(cmd, text, label).is_ok() {
                return Ok(());
            }
        }
        Err(AppError::new(
            ErrorCode::ConfigInvalid,
            "copy proxy command: no clipboard tool (wl-copy, xclip, or xsel)",
        ))
    }
}

#[cfg(unix)]
fn pipe_to_clipboard(mut cmd: Command, text: &str, label: &str) -> Result<(), AppError> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| spawn_err(label, e))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| spawn_err(label, e))?;
    }
    let status = child.wait().map_err(|e| spawn_err(label, e))?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::ConfigInvalid,
            format!("copy proxy command: {label} failed"),
        ))
    }
}

/// Spawn the platform default interactive terminal with session proxy env.
pub(crate) fn open_proxy_terminal(host: &str, port: u16) -> Result<(), AppError> {
    if port == 0 {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            "open proxy terminal: invalid mixed port",
        ));
    }
    let envs = proxy_env_pairs(host, port);
    open_proxy_terminal_with_envs(&envs)
}

#[cfg(windows)]
fn open_proxy_terminal_with_envs(envs: &[(String, String)]) -> Result<(), AppError> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

    let prelude = cmd_session_prelude(envs);

    // Prefer Windows Terminal: one tab, cmd sets env then starts PowerShell.
    // Never put `;` on this command line (wt command separator).
    let mut wt = Command::new("wt.exe");
    wt.args(["-w", "0", "nt", "cmd", "/k", &prelude]);
    if wt.spawn().is_ok() {
        return Ok(());
    }

    // Fallback: console PowerShell inherits process env from this spawn.
    let mut ps = Command::new("powershell.exe");
    apply_envs(&mut ps, envs);
    ps.creation_flags(CREATE_NEW_CONSOLE);
    ps.args(["-NoExit", "-NoLogo"]);
    ps.spawn().map_err(|e| spawn_err("powershell", e))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_proxy_terminal_with_envs(envs: &[(String, String)]) -> Result<(), AppError> {
    // Terminal.app does not inherit our process env; set vars in the do-script.
    let exports: String = envs
        .iter()
        .map(|(k, v)| format!("export {k}={v}"))
        .collect::<Vec<_>>()
        .join("; ");
    let shell_cmd = format!("{exports}; exec \"$SHELL\" -l");
    // Escape for AppleScript double quotes only; `$SHELL` must reach the shell.
    let escaped = shell_cmd.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("tell application \"Terminal\" to do script \"{escaped}\"");
    Command::new("osascript")
        .args(["-e", &script])
        .spawn()
        .map_err(|e| spawn_err("Terminal.app", e))?;
    let _ = Command::new("osascript")
        .args(["-e", "tell application \"Terminal\" to activate"])
        .spawn();
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_proxy_terminal_with_envs(envs: &[(String, String)]) -> Result<(), AppError> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());

    if let Ok(term) = std::env::var("TERMINAL") {
        if !term.is_empty() {
            let mut cmd = Command::new(&term);
            apply_envs(&mut cmd, envs);
            cmd.arg(&shell);
            if cmd.spawn().is_ok() {
                return Ok(());
            }
        }
    }

    // (program, args before the shell binary)
    let candidates: &[(&str, &[&str])] = &[
        ("x-terminal-emulator", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("kgx", &["--"]),
        ("konsole", &["-e"]),
        ("xfce4-terminal", &["-e"]),
        ("mate-terminal", &["-e"]),
        ("xterm", &["-e"]),
    ];

    let mut last_err: Option<std::io::Error> = None;
    for (program, prefix) in candidates {
        let mut cmd = Command::new(program);
        apply_envs(&mut cmd, envs);
        cmd.args(*prefix);
        cmd.arg(&shell);
        match cmd.spawn() {
            Ok(_) => return Ok(()),
            Err(err) => last_err = Some(err),
        }
    }

    Err(spawn_err(
        "no terminal emulator",
        last_err.unwrap_or_else(|| std::io::Error::other("no candidate succeeded")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_proxy_host_rewrites_unspecified() {
        assert_eq!(shell_proxy_host("0.0.0.0"), "127.0.0.1");
        assert_eq!(shell_proxy_host("::"), "127.0.0.1");
        assert_eq!(shell_proxy_host(""), "127.0.0.1");
        assert_eq!(shell_proxy_host("192.168.1.8"), "192.168.1.8");
    }

    #[test]
    fn proxy_env_pairs_are_session_scoped_and_bracket_ipv6() {
        let pairs = proxy_env_pairs("::1", 17890);
        let map: std::collections::HashMap<_, _> = pairs.into_iter().collect();
        assert_eq!(
            map.get("HTTP_PROXY").map(String::as_str),
            Some("http://[::1]:17890")
        );
        assert_eq!(
            map.get("all_proxy").map(String::as_str),
            Some("socks5://[::1]:17890")
        );
        assert_eq!(map.get("NO_PROXY").map(String::as_str), Some(NO_PROXY));
    }

    #[test]
    fn cmd_session_prelude_avoids_wt_command_separator() {
        let pairs = proxy_env_pairs("127.0.0.1", 17890);
        let prelude = cmd_session_prelude(&pairs);
        assert!(prelude.contains("set \"HTTP_PROXY=http://127.0.0.1:17890\""));
        assert!(prelude.contains("set \"ALL_PROXY=socks5://127.0.0.1:17890\""));
        assert!(prelude.contains("&& powershell -NoExit -NoLogo"));
        // ';' would be parsed by wt.exe as another wt command.
        assert!(!prelude.contains(';'));
        assert!(!prelude.contains("setx"));
    }

    #[test]
    fn format_shell_proxy_command_matches_home_copy() {
        assert_eq!(
            format_shell_proxy_command(ShellProxyPlatform::Posix, "127.0.0.1", 17890),
            "export http_proxy=http://127.0.0.1:17890 https_proxy=http://127.0.0.1:17890 all_proxy=socks5://127.0.0.1:17890 no_proxy=localhost,127.0.0.1,::1"
        );
        let windows = format_shell_proxy_command(ShellProxyPlatform::Windows, "127.0.0.1", 17890);
        assert_eq!(
            windows,
            "$env:HTTP_PROXY='http://127.0.0.1:17890'; $env:HTTPS_PROXY='http://127.0.0.1:17890'; $env:ALL_PROXY='socks5://127.0.0.1:17890'; $env:NO_PROXY='localhost,127.0.0.1,::1'"
        );
        assert!(!windows.contains("setx"));
    }
}
