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

/// Longest host accepted before it is embedded in a generated shell command.
const MAX_PROXY_HOST_LEN: usize = 255;

/// Loopback rewrite for unspecified Mixed listens (terminal clients on this host).
pub(crate) fn shell_proxy_host(listen: &str) -> String {
    let host = listen.trim();
    if host.is_empty() || host == "0.0.0.0" || host == "::" {
        return "127.0.0.1".into();
    }
    host.to_string()
}

/// True when `host` can be embedded in a generated shell command verbatim.
///
/// The allow-list covers IPv4, bracketed IPv6, and hostnames; every accepted
/// character is inert in POSIX shells, `cmd.exe`, PowerShell, and AppleScript
/// string literals. `mixed_listen` is user-configurable and is not validated at
/// all while `allow_lan` is on, so this check is what keeps a crafted value from
/// turning `open proxy terminal` into command execution.
pub(crate) fn is_safe_shell_proxy_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= MAX_PROXY_HOST_LEN
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '[' | ']'))
}

pub(crate) fn validate_shell_proxy_host(host: &str) -> Result<(), AppError> {
    if is_safe_shell_proxy_host(host) {
        return Ok(());
    }
    Err(AppError::new(
        ErrorCode::ConfigInvalid,
        format!("proxy terminal: {host:?} is not a hostname, IPv4, or IPv6 address"),
    ))
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

/// POSIX-family shells do not share one assignment syntax: fish rejects
/// `export`, so the pasted one-liner has to match the user's login shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PosixShell {
    Sh,
    Fish,
}

pub(crate) fn current_shell_proxy_platform() -> ShellProxyPlatform {
    if cfg!(windows) {
        ShellProxyPlatform::Windows
    } else {
        ShellProxyPlatform::Posix
    }
}

/// Login shell of the user running the app, from `$SHELL`.
///
/// Terminal.app always sets `SHELL`; a GUI launch that lost it falls back to
/// the POSIX form, which is what every non-fish shell accepts.
pub(crate) fn posix_shell_of(shell_path: &str) -> PosixShell {
    let name = shell_path.rsplit('/').next().unwrap_or(shell_path);
    if name.eq_ignore_ascii_case("fish") {
        PosixShell::Fish
    } else {
        PosixShell::Sh
    }
}

fn current_posix_shell() -> PosixShell {
    posix_shell_of(&std::env::var("SHELL").unwrap_or_default())
}

/// One line for the *current terminal session only* (same text Home copies).
///
/// Every branch is derived from [`proxy_env_pairs`], so a copied command and
/// the env of a terminal opened from the app always set the same variables.
pub(crate) fn format_shell_proxy_command(
    platform: ShellProxyPlatform,
    posix_shell: PosixShell,
    host: &str,
    port: u16,
) -> String {
    let pairs = proxy_env_pairs(host, port);
    match (platform, posix_shell) {
        // Windows resolves environment names case-insensitively, so the
        // upper-case half of the pairs is the complete set there.
        (ShellProxyPlatform::Windows, _) => pairs
            .iter()
            .filter(|(key, _)| key.to_ascii_uppercase() == *key)
            .map(|(key, value)| format!("$env:{key}='{value}'"))
            .collect::<Vec<_>>()
            .join("; "),
        (ShellProxyPlatform::Posix, PosixShell::Fish) => pairs
            .iter()
            .map(|(key, value)| format!("set -gx {key} {value}"))
            .collect::<Vec<_>>()
            .join("; "),
        // `export A=1 B=2` also covers the upper + lower case pairs: POSIX
        // environment names are case-sensitive, so both are needed.
        (ShellProxyPlatform::Posix, PosixShell::Sh) => format!(
            "export {}",
            pairs
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    }
}

pub(crate) fn copy_shell_proxy_command(host: &str, port: u16) -> Result<(), AppError> {
    validate_shell_proxy_host(host)?;
    if port == 0 {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            "copy proxy command: invalid mixed port",
        ));
    }
    copy_text(&format_shell_proxy_command(
        current_shell_proxy_platform(),
        current_posix_shell(),
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
    validate_shell_proxy_host(host)?;
    if port == 0 {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            "open proxy terminal: invalid mixed port",
        ));
    }
    let envs = proxy_env_pairs(host, port);
    open_proxy_terminal_with_envs(&envs)
}

/// First non-empty osascript stderr line for the UI (`execution error: ... (-1743)`).
#[cfg(target_os = "macos")]
fn osascript_failure(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .chars()
        .take(200)
        .collect::<String>();
    if detail.is_empty() {
        format!("osascript failed ({})", output.status)
    } else {
        detail
    }
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

/// Terminal.app does not inherit our process env, so the do-script hands the
/// vars to the shell it starts. `env` takes them as arguments, which keeps the
/// line valid in every shell — fish rejects POSIX `export`.
#[cfg(any(test, target_os = "macos"))]
fn macos_do_script_command(envs: &[(String, String)]) -> String {
    let assignments: String = envs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!("exec env {assignments} \"${{SHELL:-/bin/sh}}\" -l")
}

#[cfg(target_os = "macos")]
fn open_proxy_terminal_with_envs(envs: &[(String, String)]) -> Result<(), AppError> {
    let shell_cmd = macos_do_script_command(envs);
    // Escape for AppleScript double quotes only; `$SHELL` must reach the shell.
    let escaped = shell_cmd.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("tell application \"Terminal\" to do script \"{escaped}\"");

    // Wait for osascript: the first call raises the one-time Automation consent
    // prompt, so a denial (`-1743`) has to reach the UI instead of failing
    // silently. The caller stays off the main thread because that prompt blocks
    // until the user answers.
    let output = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| spawn_err("Terminal.app", e))?;
    if !output.status.success() {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            format!(
                "open proxy terminal: {}; allow ice-box to control Terminal in System Settings > Privacy & Security > Automation",
                osascript_failure(&output)
            ),
        ));
    }
    // Cosmetic: bring the new window forward. A failure here changes nothing.
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
    fn shell_proxy_host_allowlist_covers_hosts_and_blocks_metacharacters() {
        for ok in [
            "127.0.0.1",
            "::1",
            "[::1]",
            "localhost",
            "my-host.local",
            "node_1",
        ] {
            assert!(is_safe_shell_proxy_host(ok), "{ok} must be accepted");
        }
        for bad in [
            "",
            "127.0.0.1; rm -rf ~",
            "127.0.0.1 && touch /tmp/pwned",
            "host$(id)",
            "host`id`",
            "host\"x",
            "host'x",
            "host|cat",
            "host\nx",
            "host x",
            "localhost:17890/share",
        ] {
            assert!(!is_safe_shell_proxy_host(bad), "{bad} must be rejected");
        }
        assert!(is_safe_shell_proxy_host(&"a".repeat(MAX_PROXY_HOST_LEN)));
        assert!(!is_safe_shell_proxy_host(
            &"a".repeat(MAX_PROXY_HOST_LEN + 1)
        ));
    }

    #[test]
    fn copy_shell_proxy_command_rejects_unsafe_hosts_before_the_clipboard() {
        // Fails during validation: nothing is written to the clipboard and no
        // host text reaches a shell.
        let err = copy_shell_proxy_command("127.0.0.1; open -a Calculator", 17890)
            .expect_err("unsafe host");
        assert!(err.is_code(ErrorCode::ConfigInvalid), "{err:?}");
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
    fn posix_shell_of_detects_fish() {
        assert_eq!(posix_shell_of("/opt/homebrew/bin/fish"), PosixShell::Fish);
        assert_eq!(posix_shell_of("fish"), PosixShell::Fish);
        assert_eq!(posix_shell_of("/bin/zsh"), PosixShell::Sh);
        assert_eq!(posix_shell_of(""), PosixShell::Sh);
    }

    #[test]
    fn format_shell_proxy_command_sets_the_same_vars_as_the_opened_terminal() {
        let pairs = proxy_env_pairs("127.0.0.1", 17890);
        let sh = format_shell_proxy_command(
            ShellProxyPlatform::Posix,
            PosixShell::Sh,
            "127.0.0.1",
            17890,
        );
        let fish = format_shell_proxy_command(
            ShellProxyPlatform::Posix,
            PosixShell::Fish,
            "127.0.0.1",
            17890,
        );
        assert!(sh.starts_with("export "), "{sh}");
        for (key, value) in &pairs {
            assert!(
                sh.contains(&format!("{key}={value}")),
                "{key} missing in {sh}"
            );
            assert!(
                fish.contains(&format!("set -gx {key} {value}")),
                "{key} missing in {fish}"
            );
        }
        // fish has no `export`; POSIX shells have no `set -gx`.
        assert!(!fish.contains("export"), "{fish}");
        assert!(!sh.contains("set -gx"), "{sh}");

        // Windows resolves environment names case-insensitively, so the
        // upper-case set is complete there.
        let windows = format_shell_proxy_command(
            ShellProxyPlatform::Windows,
            PosixShell::Sh,
            "127.0.0.1",
            17890,
        );
        assert_eq!(
            windows,
            "$env:HTTP_PROXY='http://127.0.0.1:17890'; $env:HTTPS_PROXY='http://127.0.0.1:17890'; $env:ALL_PROXY='socks5://127.0.0.1:17890'; $env:NO_PROXY='localhost,127.0.0.1,::1'"
        );
        assert!(!windows.contains("setx"));
    }

    #[test]
    fn macos_do_script_uses_env_so_it_works_in_fish() {
        let pairs = proxy_env_pairs("127.0.0.1", 17890);
        let line = macos_do_script_command(&pairs);
        assert!(
            line.starts_with("exec env HTTP_PROXY=http://127.0.0.1:17890 "),
            "{line}"
        );
        assert!(line.contains("no_proxy=localhost,127.0.0.1,::1 "), "{line}");
        assert!(line.ends_with(" \"${SHELL:-/bin/sh}\" -l"), "{line}");
        // `export` is a POSIX builtin; fish rejects it.
        assert!(!line.contains("export"), "{line}");
    }
}
