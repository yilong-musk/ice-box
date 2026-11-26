// SPDX-License-Identifier: GPL-3.0-or-later

//! GNOME / GTK system proxy backend via `gsettings org.gnome.system.proxy`.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::bypass::bypass_domains;
use crate::linux::{bracket_ipv6_host, format_host_port, run_checked, LinuxCmdRunner};
use crate::{ProxyBackup, ProxyEndpoints, ProxySysError};

const SCHEMA_ROOT: &str = "org.gnome.system.proxy";
const SCHEMA_HTTP: &str = "org.gnome.system.proxy.http";
const SCHEMA_HTTPS: &str = "org.gnome.system.proxy.https";
const SCHEMA_SOCKS: &str = "org.gnome.system.proxy.socks";
const MEMORY_BACKEND_WARNING: &str = "Using the 'memory' GSettings backend";

/// Full snapshot of `org.gnome.system.proxy` keys for lossless restore.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GnomeProxyBackup {
    pub mode: String,
    pub autoconfig_url: String,
    pub ignore_hosts: Vec<String>,
    #[serde(default)]
    pub use_same_proxy: Option<bool>,
    pub http_host: String,
    pub http_port: u16,
    pub https_host: String,
    pub https_port: u16,
    pub socks_host: String,
    pub socks_port: u16,
}

pub fn is_available(runner: &dyn LinuxCmdRunner) -> bool {
    if !runner.has_program("gsettings") {
        return false;
    }
    runner
        .run("gsettings", &["get", SCHEMA_ROOT, "mode"])
        .is_ok_and(|out| out.success && !out.stdout.trim().is_empty())
}

fn get_string(runner: &dyn LinuxCmdRunner, schema: &str, key: &str) -> Result<String, String> {
    let raw = run_checked(runner, "gsettings", &["get", schema, key])?;
    Ok(parse_gvariant_string(&raw))
}

fn get_port(runner: &dyn LinuxCmdRunner, schema: &str, key: &str) -> Result<u16, String> {
    let raw = run_checked(runner, "gsettings", &["get", schema, key])?;
    let trimmed = raw.trim();
    // Strip optional GVariant type prefix such as `uint16 ` or `int32 `.
    let token = trimmed.split_whitespace().last().unwrap_or(trimmed);
    Ok(token.parse::<u16>().unwrap_or(0))
}

fn get_optional_bool(runner: &dyn LinuxCmdRunner, schema: &str, key: &str) -> Option<bool> {
    let raw = run_checked(runner, "gsettings", &["get", schema, key]).ok()?;
    match raw.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn set_value(
    runner: &dyn LinuxCmdRunner,
    schema: &str,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let args = ["set", schema, key, value];
    let out = runner
        .run("gsettings", &args)
        .map_err(|e| format!("spawn gsettings: {e}"))?;
    if !out.success {
        let detail = if !out.stderr.trim().is_empty() {
            out.stderr.trim()
        } else {
            out.stdout.trim()
        };
        return Err(format!("gsettings {args:?} failed: {detail}"));
    }
    if out.stderr.contains(MEMORY_BACKEND_WARNING) {
        return Err(format!(
            "gsettings {args:?} wrote to the in-memory GSettings backend: {}",
            out.stderr.trim()
        ));
    }
    Ok(())
}

pub fn backup(runner: &dyn LinuxCmdRunner) -> Result<ProxyBackup, ProxySysError> {
    let mode = get_string(runner, SCHEMA_ROOT, "mode").map_err(ProxySysError::ApplyFailed)?;
    let autoconfig_url =
        get_string(runner, SCHEMA_ROOT, "autoconfig-url").map_err(ProxySysError::ApplyFailed)?;
    let ignore_hosts_raw = run_checked(runner, "gsettings", &["get", SCHEMA_ROOT, "ignore-hosts"])
        .map_err(ProxySysError::ApplyFailed)?;
    let ignore_hosts = parse_gvariant_string_list(&ignore_hosts_raw);
    let use_same_proxy = get_optional_bool(runner, SCHEMA_ROOT, "use-same-proxy");

    let http_host = get_string(runner, SCHEMA_HTTP, "host").map_err(ProxySysError::ApplyFailed)?;
    let http_port = get_port(runner, SCHEMA_HTTP, "port").map_err(ProxySysError::ApplyFailed)?;
    let https_host =
        get_string(runner, SCHEMA_HTTPS, "host").map_err(ProxySysError::ApplyFailed)?;
    let https_port = get_port(runner, SCHEMA_HTTPS, "port").map_err(ProxySysError::ApplyFailed)?;
    let socks_host =
        get_string(runner, SCHEMA_SOCKS, "host").map_err(ProxySysError::ApplyFailed)?;
    let socks_port = get_port(runner, SCHEMA_SOCKS, "port").map_err(ProxySysError::ApplyFailed)?;

    let enabled = matches!(mode.as_str(), "manual" | "auto");
    let (http, https, socks) = if mode == "manual" {
        (
            format_host_port(&http_host, http_port),
            format_host_port(&https_host, https_port),
            format_host_port(&socks_host, socks_port),
        )
    } else {
        (None, None, None)
    };

    let state = GnomeProxyBackup {
        mode,
        autoconfig_url,
        ignore_hosts,
        use_same_proxy,
        http_host,
        http_port,
        https_host,
        https_port,
        socks_host,
        socks_port,
    };

    Ok(ProxyBackup {
        enabled,
        http,
        https,
        socks,
        extra: json!({
            "backend": "gnome",
            "gnome": state,
        }),
    })
}

pub fn apply(runner: &dyn LinuxCmdRunner, endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
    let pre_backup = backup(runner)?;

    let write = || -> Result<(), String> {
        let http_host_val = bracket_ipv6_host(&endpoints.http_host);
        let http_host = format_gvariant_string(&http_host_val);
        let http_port = endpoints.http_port.to_string();
        let socks_host_val = bracket_ipv6_host(
            endpoints
                .socks_host
                .as_deref()
                .unwrap_or(&endpoints.http_host),
        );
        let socks_host = format_gvariant_string(&socks_host_val);
        let socks_port = endpoints
            .socks_port
            .unwrap_or(endpoints.http_port)
            .to_string();
        let bypass = bypass_domains();
        let ignore_hosts = format_gvariant_string_list(&bypass);

        set_value(runner, SCHEMA_HTTP, "host", &http_host)?;
        set_value(runner, SCHEMA_HTTP, "port", &http_port)?;
        set_value(runner, SCHEMA_HTTPS, "host", &http_host)?;
        set_value(runner, SCHEMA_HTTPS, "port", &http_port)?;
        set_value(runner, SCHEMA_SOCKS, "host", &socks_host)?;
        set_value(runner, SCHEMA_SOCKS, "port", &socks_port)?;
        set_value(runner, SCHEMA_ROOT, "ignore-hosts", &ignore_hosts)?;
        // Best-effort: `use-same-proxy` is ignored when a schema omits it.
        let _ = set_value(runner, SCHEMA_ROOT, "use-same-proxy", "false");
        set_value(runner, SCHEMA_ROOT, "mode", "'manual'")?;

        // Read back the persisted values to catch silent writes to an
        // in-memory GSettings backend (e.g. broken GIO/dconf environment).
        let actual_mode = get_string(runner, SCHEMA_ROOT, "mode")?;
        if actual_mode != "manual" {
            return Err(format!(
                "gsettings read-back mismatch for {SCHEMA_ROOT} mode: expected 'manual', got '{actual_mode}'"
            ));
        }
        if !http_host_val.is_empty() && endpoints.http_port != 0 {
            let actual_http_host = get_string(runner, SCHEMA_HTTP, "host")?;
            if actual_http_host != http_host_val {
                return Err(format!(
                    "gsettings read-back mismatch for {SCHEMA_HTTP} host: expected '{http_host_val}', got '{actual_http_host}'"
                ));
            }
        } else if !socks_host_val.is_empty() {
            let actual_socks_host = get_string(runner, SCHEMA_SOCKS, "host")?;
            if actual_socks_host != socks_host_val {
                return Err(format!(
                    "gsettings read-back mismatch for {SCHEMA_SOCKS} host: expected '{socks_host_val}', got '{actual_socks_host}'"
                ));
            }
        }
        Ok(())
    };

    if let Err(err) = write() {
        if let Err(restore_err) = restore(runner, &pre_backup) {
            tracing::error!(error = %restore_err, "rollback partial GNOME proxy apply");
        }
        return Err(ProxySysError::ApplyFailed(err));
    }
    Ok(())
}

pub fn restore(runner: &dyn LinuxCmdRunner, backup: &ProxyBackup) -> Result<(), ProxySysError> {
    let Some(raw_state) = backup.extra.get("gnome") else {
        // Corrupt-backup or empty fallback: switch GNOME proxy off.
        set_value(runner, SCHEMA_ROOT, "mode", "'none'").map_err(ProxySysError::RestoreFailed)?;
        return Ok(());
    };
    let state: GnomeProxyBackup = serde_json::from_value(raw_state.clone())
        .map_err(|e| ProxySysError::RestoreFailed(format!("parse backup.extra.gnome: {e}")))?;

    let mode = format_gvariant_string(if state.mode.is_empty() {
        "none"
    } else {
        &state.mode
    });
    if state.mode == "none" || state.mode.is_empty() {
        set_value(runner, SCHEMA_ROOT, "mode", &mode).map_err(ProxySysError::RestoreFailed)?;
    }

    set_value(
        runner,
        SCHEMA_HTTP,
        "host",
        &format_gvariant_string(&state.http_host),
    )
    .map_err(ProxySysError::RestoreFailed)?;
    set_value(runner, SCHEMA_HTTP, "port", &state.http_port.to_string())
        .map_err(ProxySysError::RestoreFailed)?;
    set_value(
        runner,
        SCHEMA_HTTPS,
        "host",
        &format_gvariant_string(&state.https_host),
    )
    .map_err(ProxySysError::RestoreFailed)?;
    set_value(runner, SCHEMA_HTTPS, "port", &state.https_port.to_string())
        .map_err(ProxySysError::RestoreFailed)?;
    set_value(
        runner,
        SCHEMA_SOCKS,
        "host",
        &format_gvariant_string(&state.socks_host),
    )
    .map_err(ProxySysError::RestoreFailed)?;
    set_value(runner, SCHEMA_SOCKS, "port", &state.socks_port.to_string())
        .map_err(ProxySysError::RestoreFailed)?;
    set_value(
        runner,
        SCHEMA_ROOT,
        "autoconfig-url",
        &format_gvariant_string(&state.autoconfig_url),
    )
    .map_err(ProxySysError::RestoreFailed)?;

    let ignore_refs: Vec<&str> = state.ignore_hosts.iter().map(String::as_str).collect();
    set_value(
        runner,
        SCHEMA_ROOT,
        "ignore-hosts",
        &format_gvariant_string_list(&ignore_refs),
    )
    .map_err(ProxySysError::RestoreFailed)?;

    if let Some(same) = state.use_same_proxy {
        let _ = set_value(
            runner,
            SCHEMA_ROOT,
            "use-same-proxy",
            if same { "true" } else { "false" },
        );
    }

    if state.mode != "none" && !state.mode.is_empty() {
        set_value(runner, SCHEMA_ROOT, "mode", &mode).map_err(ProxySysError::RestoreFailed)?;
    }
    Ok(())
}

/// Format a Rust string as a single-quoted GVariant string literal.
pub fn format_gvariant_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('\'');
    out
}

/// Format a list of strings as a GVariant `as` literal.
pub fn format_gvariant_string_list(items: &[&str]) -> String {
    if items.is_empty() {
        return "@as []".to_string();
    }
    let inner: Vec<String> = items.iter().map(|s| format_gvariant_string(s)).collect();
    format!("[{}]", inner.join(", "))
}

/// Parse a GVariant string output from `gsettings get`.
pub fn parse_gvariant_string(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('\'') && trimmed.ends_with('\''))
            || (trimmed.starts_with('"') && trimmed.ends_with('"')))
    {
        return unescape_gvariant_inner(&trimmed[1..trimmed.len() - 1]);
    }
    trimmed.to_string()
}

/// Parse a GVariant `as` list output from `gsettings get`.
pub fn parse_gvariant_string_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    let body = trimmed
        .strip_prefix("@as")
        .map(str::trim)
        .unwrap_or(trimmed);
    let Some(inner) = body.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
        return Vec::new();
    };
    let inner = inner.trim();
    if inner.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut chars = inner.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_whitespace() || ch == ',' {
            continue;
        }
        if ch == '\'' || ch == '"' {
            let quote = ch;
            let mut item = String::new();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    if let Some(escaped) = chars.next() {
                        match escaped {
                            'n' => item.push('\n'),
                            'r' => item.push('\r'),
                            't' => item.push('\t'),
                            other => item.push(other),
                        }
                    }
                } else if c == quote {
                    break;
                } else {
                    item.push(c);
                }
            }
            out.push(item);
        }
    }
    out
}

fn unescape_gvariant_inner(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                match escaped {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    other => out.push(other),
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gvariant_string_roundtrips_quotes_and_backslashes() {
        for sample in [
            "",
            "127.0.0.1",
            "http://example.com/a'b\\c.pac",
            "line1\nline2",
        ] {
            let encoded = format_gvariant_string(sample);
            assert_eq!(parse_gvariant_string(&encoded), sample);
        }
        assert_eq!(parse_gvariant_string("\"double'quoted\""), "double'quoted");
    }

    #[test]
    fn gvariant_list_parses_empty_and_escaped_entries() {
        assert_eq!(parse_gvariant_string_list("@as []\n"), Vec::<String>::new());
        assert_eq!(parse_gvariant_string_list("[]"), Vec::<String>::new());
        assert_eq!(format_gvariant_string_list(&[]), "@as []");

        let list = ["localhost", "127.0.0.1", "::1", "a'b,c"];
        let encoded = format_gvariant_string_list(&list);
        assert_eq!(
            parse_gvariant_string_list(&encoded),
            vec!["localhost", "127.0.0.1", "::1", "a'b,c"]
        );
    }
}
