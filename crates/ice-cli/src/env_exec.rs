// SPDX-License-Identifier: GPL-3.0-or-later

//! Session-scoped proxy environment helpers (`env` and `exec`) for automation
//! and Agent command execution across macOS, Windows, and Linux.

use crate::exit::{EXIT_ERROR, EXIT_USAGE};
use crate::output::{emit_error, print_json, OutputMode};
use ice_config::AppPaths;
use ice_runtime::mcp::call_tool;
use ice_runtime::orchestrate::current_settings;
use ice_runtime::proxy_terminal::{proxy_env_pairs, shell_proxy_host, validate_shell_proxy_host};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellFlavor {
    Auto,
    Posix,
    Fish,
    PowerShell,
    Cmd,
}

impl ShellFlavor {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "sh" | "bash" | "zsh" | "posix" => Ok(Self::Posix),
            "fish" => Ok(Self::Fish),
            "ps" | "pwsh" | "powershell" => Ok(Self::PowerShell),
            "cmd" | "batch" => Ok(Self::Cmd),
            other => Err(format!(
                "unknown shell '{other}' (expected auto | sh | bash | zsh | fish | pwsh | cmd)"
            )),
        }
    }

    pub fn resolve(self) -> Self {
        match self {
            Self::Auto => {
                #[cfg(windows)]
                {
                    if std::env::var_os("PSModulePath").is_some() {
                        Self::PowerShell
                    } else {
                        Self::Cmd
                    }
                }
                #[cfg(not(windows))]
                {
                    let shell = std::env::var("SHELL").unwrap_or_default();
                    let name = Path::new(&shell)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");
                    if name.eq_ignore_ascii_case("fish") {
                        Self::Fish
                    } else {
                        Self::Posix
                    }
                }
            }
            other => other,
        }
    }
}

/// Resolve `(host, port)` from the live runtime if reachable, falling back to
/// persisted `settings.json` in `data_dir` (matching desktop's terminal helper
/// behavior so commands can be prepared even before starting the core).
pub fn resolve_mixed_endpoint(
    data_dir: &Path,
    timeout: Option<Duration>,
) -> Result<(String, u16, bool), String> {
    if let Ok(output) = call_tool(data_dir, "get_status", json!({}), timeout) {
        let host = output
            .value
            .pointer("/core/inbound_host")
            .and_then(Value::as_str);
        let port = output
            .value
            .pointer("/core/inbound_port")
            .and_then(Value::as_u64)
            .and_then(|p| u16::try_from(p).ok());
        if let (Some(host), Some(port)) = (host, port) {
            let safe_host = shell_proxy_host(host);
            validate_shell_proxy_host(&safe_host).map_err(|e| e.message)?;
            return Ok((safe_host, port, true));
        }
    }
    if let Ok(output) = call_tool(data_dir, "get_settings", json!({}), timeout) {
        let listen = output
            .value
            .get("mixed_listen")
            .and_then(Value::as_str)
            .unwrap_or("127.0.0.1");
        let port = output
            .value
            .get("mixed_port")
            .and_then(Value::as_u64)
            .and_then(|p| u16::try_from(p).ok())
            .unwrap_or(17890);
        let safe_host = shell_proxy_host(listen);
        validate_shell_proxy_host(&safe_host).map_err(|e| e.message)?;
        return Ok((safe_host, port, false));
    }
    let paths = AppPaths::new(data_dir);
    let settings = current_settings(&paths).unwrap_or_default();
    let safe_host = shell_proxy_host(&settings.mixed_listen);
    validate_shell_proxy_host(&safe_host).map_err(|e| e.message)?;
    Ok((safe_host, settings.mixed_port, false))
}

/// Format shell commands setting the session proxy env vars.
pub fn format_env_script(flavor: ShellFlavor, host: &str, port: u16) -> String {
    let pairs = proxy_env_pairs(host, port);
    match flavor.resolve() {
        ShellFlavor::Posix | ShellFlavor::Auto => pairs
            .into_iter()
            .map(|(k, v)| format!("export {k}='{v}'"))
            .collect::<Vec<_>>()
            .join("\n"),
        ShellFlavor::Fish => pairs
            .into_iter()
            .map(|(k, v)| format!("set -gx {k} '{v}'"))
            .collect::<Vec<_>>()
            .join("\n"),
        ShellFlavor::PowerShell => pairs
            .into_iter()
            .map(|(k, v)| format!("$env:{k} = '{v}'"))
            .collect::<Vec<_>>()
            .join("\n"),
        ShellFlavor::Cmd => pairs
            .into_iter()
            .map(|(k, v)| format!("set {k}={v}"))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub fn run_env_command(
    data_dir: &Path,
    flavor: ShellFlavor,
    mode: OutputMode,
    timeout: Option<Duration>,
) -> i32 {
    let (host, port, live) = match resolve_mixed_endpoint(data_dir, timeout) {
        Ok(res) => res,
        Err(err) => {
            emit_error(mode, "config.invalid", &err);
            return EXIT_ERROR;
        }
    };
    let script = format_env_script(flavor, &host, port);
    if mode.wants_json_for_raw() {
        let mut env_map = serde_json::Map::new();
        for (k, v) in proxy_env_pairs(&host, port) {
            env_map.insert(k, Value::String(v));
        }
        print_json(&json!({
            "host": host,
            "port": port,
            "live": live,
            "env": env_map,
            "script": script,
        }));
    } else {
        println!("{script}");
    }
    0
}

/// Execute `program` with `args` under the Mixed proxy environment and propagate
/// its exact exit status.
pub fn run_exec_command(
    data_dir: &Path,
    command_argv: &[String],
    mode: OutputMode,
    timeout: Option<Duration>,
) -> i32 {
    let Some((program, args)) = command_argv.split_first() else {
        emit_error(
            mode,
            "cli.usage",
            "exec requires a command after `--` (example: ice-box-cli exec -- curl https://ifconfig.me)",
        );
        return EXIT_USAGE;
    };
    let (host, port, _live) = match resolve_mixed_endpoint(data_dir, timeout) {
        Ok(res) => res,
        Err(err) => {
            emit_error(mode, "config.invalid", &err);
            return EXIT_ERROR;
        }
    };
    let mut cmd = Command::new(program);
    cmd.args(args);
    for (k, v) in proxy_env_pairs(&host, port) {
        cmd.env(k, v);
    }
    match cmd.status() {
        Ok(status) => {
            if let Some(code) = status.code() {
                code
            } else {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    status.signal().map_or(EXIT_ERROR, |sig| 128 + sig)
                }
                #[cfg(not(unix))]
                {
                    EXIT_ERROR
                }
            }
        }
        Err(err) => {
            emit_error(
                mode,
                "cli.exec_failed",
                &format!("spawn '{program}': {err}"),
            );
            EXIT_ERROR
        }
    }
}
