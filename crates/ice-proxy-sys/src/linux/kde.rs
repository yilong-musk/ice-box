// SPDX-License-Identifier: GPL-3.0-or-later

//! KDE Plasma 5/6 system proxy backend via `kreadconfig`/`kwriteconfig` and
//! `kioslaverc` + DBus KIO scheduler reload.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::bypass::bypass_domains;
use crate::linux::{format_host_port, run_checked, LinuxCmdRunner};
use crate::{ProxyBackup, ProxyEndpoints, ProxySysError};

const KIO_FILE: &str = "kioslaverc";
const KIO_GROUP: &str = "Proxy Settings";

/// Resolved KDE config CLI pair (Plasma 6 preferred, Plasma 5 fallback).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdeTools {
    pub read_cmd: &'static str,
    pub write_cmd: &'static str,
}

/// Full snapshot of `kioslaverc` `[Proxy Settings]` for lossless restore.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KdeProxyBackup {
    pub proxy_type: String,
    pub http_proxy: String,
    pub https_proxy: String,
    pub socks_proxy: String,
    pub no_proxy_for: String,
    pub reversed_exception: String,
    pub proxy_config_script: String,
}

pub fn detect_tools(runner: &dyn LinuxCmdRunner) -> Option<KdeTools> {
    for (read_cmd, write_cmd) in [
        ("kreadconfig6", "kwriteconfig6"),
        ("kreadconfig5", "kwriteconfig5"),
    ] {
        if runner.has_program(read_cmd) && runner.has_program(write_cmd) {
            return Some(KdeTools {
                read_cmd,
                write_cmd,
            });
        }
    }
    None
}

fn read_key(runner: &dyn LinuxCmdRunner, tools: &KdeTools, key: &str) -> Result<String, String> {
    let out = run_checked(
        runner,
        tools.read_cmd,
        &["--file", KIO_FILE, "--group", KIO_GROUP, "--key", key],
    )?;
    Ok(out.trim().to_string())
}

fn write_key(
    runner: &dyn LinuxCmdRunner,
    tools: &KdeTools,
    key: &str,
    value: &str,
) -> Result<(), String> {
    if value.is_empty() {
        run_checked(
            runner,
            tools.write_cmd,
            &[
                "--file", KIO_FILE, "--group", KIO_GROUP, "--key", key, "--delete",
            ],
        )
        .map(|_| ())
    } else {
        run_checked(
            runner,
            tools.write_cmd,
            &[
                "--file", KIO_FILE, "--group", KIO_GROUP, "--key", key, value,
            ],
        )
        .map(|_| ())
    }
}

fn notify_kio_reload(runner: &dyn LinuxCmdRunner) {
    if !runner.has_program("dbus-send") {
        return;
    }
    let _ = runner.run(
        "dbus-send",
        &[
            "--type=signal",
            "/KIO/Scheduler",
            "org.kde.KIO.Scheduler.reparseSlaveConfiguration",
            "string:",
        ],
    );
}

fn format_kde_proxy_value(scheme: &str, host: &str, port: u16) -> String {
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    // KDE KCM (`kcm_proxy`) writes `scheme://host port` (space-separated),
    // which KIO, Qt, and Chromium's Linux proxy service all parse.
    format!("{scheme}://{host} {port}")
}

/// Parse a `kioslaverc` proxy entry (`http://host port`, `http://host:port`, or
/// bare `host:port`) into normalized `host:port`.
pub fn parse_kde_proxy_endpoint(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let no_scheme = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(trimmed);

    // Space-separated (`host port`)
    if let Some((host, port_str)) = no_scheme.rsplit_once(char::is_whitespace) {
        let port = port_str.trim().parse::<u16>().ok()?;
        let host = host.trim().trim_end_matches('/');
        return format_host_port(host, port);
    }

    // Colon-separated (`[ipv6]:port` or `host:port`)
    let no_path = no_scheme.trim_end_matches('/');
    if let Some(rest) = no_path.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        let port = after.strip_prefix(':')?.parse::<u16>().ok()?;
        return format_host_port(host, port);
    }
    let (host, port_str) = no_path.rsplit_once(':')?;
    let port = port_str.parse::<u16>().ok()?;
    format_host_port(host, port)
}

pub fn backup(runner: &dyn LinuxCmdRunner, tools: &KdeTools) -> Result<ProxyBackup, ProxySysError> {
    let proxy_type = read_key(runner, tools, "ProxyType").map_err(ProxySysError::ApplyFailed)?;
    let http_proxy = read_key(runner, tools, "httpProxy").map_err(ProxySysError::ApplyFailed)?;
    let https_proxy = read_key(runner, tools, "httpsProxy").map_err(ProxySysError::ApplyFailed)?;
    let socks_proxy = read_key(runner, tools, "socksProxy").map_err(ProxySysError::ApplyFailed)?;
    let no_proxy_for = read_key(runner, tools, "NoProxyFor").map_err(ProxySysError::ApplyFailed)?;
    let reversed_exception =
        read_key(runner, tools, "ReversedException").map_err(ProxySysError::ApplyFailed)?;
    let proxy_config_script =
        read_key(runner, tools, "ProxyConfigScript").map_err(ProxySysError::ApplyFailed)?;

    let enabled = matches!(proxy_type.as_str(), "1" | "2" | "3");
    let (http, https, socks) = if proxy_type == "1" {
        (
            parse_kde_proxy_endpoint(&http_proxy),
            parse_kde_proxy_endpoint(&https_proxy),
            parse_kde_proxy_endpoint(&socks_proxy),
        )
    } else {
        (None, None, None)
    };

    let state = KdeProxyBackup {
        proxy_type,
        http_proxy,
        https_proxy,
        socks_proxy,
        no_proxy_for,
        reversed_exception,
        proxy_config_script,
    };

    Ok(ProxyBackup {
        enabled,
        http,
        https,
        socks,
        extra: json!({
            "backend": "kde",
            "kde": state,
        }),
    })
}

pub fn apply(
    runner: &dyn LinuxCmdRunner,
    tools: &KdeTools,
    endpoints: &ProxyEndpoints,
) -> Result<(), ProxySysError> {
    let pre_backup = backup(runner, tools)?;

    let write = || -> Result<(), String> {
        let http_val = format_kde_proxy_value("http", &endpoints.http_host, endpoints.http_port);
        let socks_host = endpoints
            .socks_host
            .as_deref()
            .unwrap_or(&endpoints.http_host);
        let socks_port = endpoints.socks_port.unwrap_or(endpoints.http_port);
        let socks_val = format_kde_proxy_value("socks", socks_host, socks_port);
        let bypass = bypass_domains().join(",");

        write_key(runner, tools, "httpProxy", &http_val)?;
        write_key(runner, tools, "httpsProxy", &http_val)?;
        write_key(runner, tools, "socksProxy", &socks_val)?;
        write_key(runner, tools, "NoProxyFor", &bypass)?;
        write_key(runner, tools, "ReversedException", "false")?;
        write_key(runner, tools, "ProxyType", "1")?;
        Ok(())
    };

    if let Err(err) = write() {
        if let Err(restore_err) = restore(runner, tools, &pre_backup) {
            tracing::error!(error = %restore_err, "rollback partial KDE proxy apply");
        }
        return Err(ProxySysError::ApplyFailed(err));
    }

    notify_kio_reload(runner);
    Ok(())
}

pub fn restore(
    runner: &dyn LinuxCmdRunner,
    tools: &KdeTools,
    backup: &ProxyBackup,
) -> Result<(), ProxySysError> {
    let Some(raw_state) = backup.extra.get("kde") else {
        // Corrupt-backup or empty fallback: disable KDE proxy.
        write_key(runner, tools, "ProxyType", "0").map_err(ProxySysError::RestoreFailed)?;
        notify_kio_reload(runner);
        return Ok(());
    };
    let state: KdeProxyBackup = serde_json::from_value(raw_state.clone())
        .map_err(|e| ProxySysError::RestoreFailed(format!("parse backup.extra.kde: {e}")))?;

    let proxy_type = if state.proxy_type.is_empty() {
        "0"
    } else {
        state.proxy_type.as_str()
    };
    if proxy_type == "0" {
        write_key(runner, tools, "ProxyType", "0").map_err(ProxySysError::RestoreFailed)?;
    }

    write_key(runner, tools, "httpProxy", &state.http_proxy)
        .map_err(ProxySysError::RestoreFailed)?;
    write_key(runner, tools, "httpsProxy", &state.https_proxy)
        .map_err(ProxySysError::RestoreFailed)?;
    write_key(runner, tools, "socksProxy", &state.socks_proxy)
        .map_err(ProxySysError::RestoreFailed)?;
    write_key(runner, tools, "NoProxyFor", &state.no_proxy_for)
        .map_err(ProxySysError::RestoreFailed)?;
    write_key(
        runner,
        tools,
        "ReversedException",
        &state.reversed_exception,
    )
    .map_err(ProxySysError::RestoreFailed)?;
    write_key(
        runner,
        tools,
        "ProxyConfigScript",
        &state.proxy_config_script,
    )
    .map_err(ProxySysError::RestoreFailed)?;

    if proxy_type != "0" {
        write_key(runner, tools, "ProxyType", proxy_type).map_err(ProxySysError::RestoreFailed)?;
    }

    notify_kio_reload(runner);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_space_and_colon_kde_proxy_entries() {
        assert_eq!(
            parse_kde_proxy_endpoint("http://127.0.0.1 17890").as_deref(),
            Some("127.0.0.1:17890")
        );
        assert_eq!(
            parse_kde_proxy_endpoint("http://127.0.0.1:17890").as_deref(),
            Some("127.0.0.1:17890")
        );
        assert_eq!(
            parse_kde_proxy_endpoint("socks://[::1] 1080").as_deref(),
            Some("[::1]:1080")
        );
        assert_eq!(
            parse_kde_proxy_endpoint("http://[::1]:8080/").as_deref(),
            Some("[::1]:8080")
        );
        assert_eq!(parse_kde_proxy_endpoint(""), None);
    }
}
