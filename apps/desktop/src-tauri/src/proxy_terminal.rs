// SPDX-License-Identifier: GPL-3.0-or-later

//! Open a new interactive terminal with Mixed proxy env for *this session only*.
//!
//! Env is injected into the new process (or via a one-shot `export` / `$env:` in
//! the launch command). Never uses `setx`, User/Machine environment APIs, or
//! shell profile edits.

use ice_config::{AppError, ErrorCode};
use std::process::Command;

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

#[cfg(all(unix, not(target_os = "macos")))]
fn apply_envs(cmd: &mut Command, envs: &[(String, String)]) {
    for (key, value) in envs {
        cmd.env(key, value);
    }
}

/// PowerShell session prelude: process-scoped `$env:` only (no User/Machine persistence).
#[cfg(any(test, windows))]
fn powershell_env_prelude(envs: &[(String, String)]) -> String {
    envs.iter()
        .map(|(k, v)| format!("$env:{k}='{v}'"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn spawn_err(context: &str, err: std::io::Error) -> AppError {
    AppError::new(
        ErrorCode::ConfigInvalid,
        format!("open proxy terminal ({context}): {err}"),
    )
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

    // Windows Terminal does not forward the launching process environment into
    // the profile shell. Set session vars inside PowerShell via -Command.
    let prelude = powershell_env_prelude(envs);

    let mut wt = Command::new("wt.exe");
    wt.args([
        "-w",
        "0",
        "nt",
        "powershell",
        "-NoExit",
        "-NoLogo",
        "-Command",
        &prelude,
    ]);
    if wt.spawn().is_ok() {
        return Ok(());
    }

    let mut ps = Command::new("powershell.exe");
    ps.creation_flags(CREATE_NEW_CONSOLE);
    ps.args(["-NoExit", "-NoLogo", "-Command", &prelude]);
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
    fn powershell_env_prelude_sets_process_scoped_vars() {
        let pairs = proxy_env_pairs("127.0.0.1", 17890);
        let prelude = powershell_env_prelude(&pairs);
        assert!(prelude.contains("$env:HTTP_PROXY='http://127.0.0.1:17890'"));
        assert!(prelude.contains("$env:ALL_PROXY='socks5://127.0.0.1:17890'"));
        assert!(prelude.contains("$env:NO_PROXY='localhost,127.0.0.1,::1'"));
        assert!(!prelude.contains("setx"));
        assert!(!prelude.contains("SetEnvironmentVariable"));
    }
}
