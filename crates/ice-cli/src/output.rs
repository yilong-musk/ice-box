// SPDX-License-Identifier: GPL-3.0-or-later

//! Machine-readable JSON and concise human-readable output formatting.

use serde_json::{json, Value};
use std::fmt::Write as _;
use std::io::IsTerminal;

/// Explicit or auto-detected output mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Emit JSON when stdout is piped (non-TTY), human summary on TTY.
    Auto,
    /// Always emit structured JSON on stdout.
    Json,
    /// Always emit human-readable text on stdout.
    Human,
}

impl OutputMode {
    /// Whether structured JSON should be emitted for regular commands.
    pub fn wants_json(self) -> bool {
        match self {
            Self::Json => true,
            Self::Human => false,
            Self::Auto => !std::io::stdout().is_terminal(),
        }
    }

    /// Whether raw-text commands (`env`, `config`) should switch to JSON.
    pub fn wants_json_for_raw(self) -> bool {
        matches!(self, Self::Json)
    }
}

/// Print a JSON value cleanly to stdout.
pub fn print_json(value: &Value) {
    match serde_json::to_string_pretty(value) {
        Ok(text) => println!("{text}"),
        Err(_) => println!("{value}"),
    }
}

/// Print a structured error to stderr (as JSON when JSON mode is active so
/// Agent pipelines parsing stderr receive structured error details without
/// corrupting stdout).
pub fn emit_error(mode: OutputMode, code: &str, message: &str) {
    if mode.wants_json() {
        let payload = json!({
            "error": {
                "code": code,
                "message": message,
            }
        });
        if let Ok(text) = serde_json::to_string_pretty(&payload) {
            eprintln!("{text}");
            return;
        }
    }
    eprintln!("error [{code}]: {message}");
}

/// Print a tool result in either JSON or a compact human-readable form.
pub fn emit_tool_result(mode: OutputMode, command: &str, value: &Value) {
    if mode.wants_json() {
        print_json(value);
        return;
    }
    match format_tool_result_human(command, value) {
        Some(text) => print!("{text}"),
        None => print_json(value),
    }
}

/// Format a tool result for human-readable terminal display. Returns `None`
/// when the command should fall back to pretty-printed JSON.
pub(crate) fn format_tool_result_human(command: &str, value: &Value) -> Option<String> {
    match command {
        "status" => Some(format_status_human(value)),
        "sub_list" => format_subscriptions_human(value),
        "node_list" => format_nodes_human(value),
        "conn_list" => Some(format_connections_human(value)),
        "ruleset_list" => format_rule_sets_human(value),
        "logs" => format_logs_human(value),
        _ => None,
    }
}

fn format_status_human(v: &Value) -> String {
    let core = v
        .pointer("/core/status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let capture = v
        .get("traffic_capture")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let subs = v
        .get("subscription_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let host = v
        .pointer("/core/inbound_host")
        .and_then(Value::as_str)
        .unwrap_or("-");
    let port = v
        .pointer("/core/inbound_port")
        .and_then(Value::as_u64)
        .map(|p| p.to_string())
        .unwrap_or_else(|| "-".into());
    let mut out = String::new();
    let _ = writeln!(out, "core:          {core}");
    let _ = writeln!(out, "mixed inbound: {host}:{port}");
    let _ = writeln!(out, "capture:       {capture}");
    let _ = writeln!(out, "subscriptions: {subs}");
    if let Some(tag) = v.pointer("/selected_outbound/tag").and_then(Value::as_str) {
        let _ = writeln!(out, "outbound:      {tag}");
    }
    out
}

fn format_subscriptions_human(v: &Value) -> Option<String> {
    let items = v.as_array()?;
    if items.is_empty() {
        return Some("(no subscriptions)\n".into());
    }
    let mut out = String::new();
    for item in items {
        let active = if item.get("active").and_then(Value::as_bool).unwrap_or(false) {
            "*"
        } else {
            " "
        };
        let id = item.get("id").and_then(Value::as_str).unwrap_or("-");
        let name = item.get("name").and_then(Value::as_str).unwrap_or("-");
        let nodes = item.get("node_count").and_then(Value::as_u64).unwrap_or(0);
        let _ = writeln!(out, "{active} {id}  {name}  ({nodes} nodes)");
    }
    Some(out)
}

fn format_nodes_human(v: &Value) -> Option<String> {
    let items = v.as_array()?;
    if items.is_empty() {
        return Some("(no nodes)\n".into());
    }
    let mut out = String::new();
    for item in items {
        let tag = item.get("tag").and_then(Value::as_str).unwrap_or("-");
        let kind = item
            .get("outbound_type")
            .and_then(Value::as_str)
            .unwrap_or("-");
        if let Some(now) = item.get("group_now").and_then(Value::as_str) {
            let _ = writeln!(out, "{tag}  [{kind}] -> {now}");
        } else {
            let _ = writeln!(out, "{tag}  [{kind}]");
        }
    }
    Some(out)
}

fn format_connections_human(v: &Value) -> String {
    let total = v.get("total").and_then(Value::as_u64).unwrap_or(0);
    let matched = v.get("matched").and_then(Value::as_u64).unwrap_or(0);
    let mut out = String::new();
    let _ = writeln!(out, "connections: {matched}/{total}");
    if let Some(items) = v.get("connections").and_then(Value::as_array) {
        for item in items {
            let id = item.get("id").and_then(Value::as_str).unwrap_or("-");
            let host = item
                .get("host")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .or_else(|| item.get("destination_ip").and_then(Value::as_str))
                .unwrap_or("-");
            let port = item
                .get("destination_port")
                .and_then(|p| {
                    p.as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .or_else(|| p.as_u64().map(|n| n.to_string()))
                })
                .unwrap_or_else(|| "-".into());
            let chains = item
                .get("chains")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" -> ")
                })
                .unwrap_or_default();
            let _ = writeln!(out, "  {id}  {host}:{port}  [{chains}]");
        }
    }
    out
}

fn format_rule_sets_human(v: &Value) -> Option<String> {
    let items = v.as_array()?;
    if items.is_empty() {
        return Some("(no rule-sets)\n".into());
    }
    let mut out = String::new();
    for item in items {
        let tag = item.get("tag").and_then(Value::as_str).unwrap_or("-");
        let state = item.get("state").and_then(Value::as_str).unwrap_or("-");
        let _ = writeln!(out, "{tag}  ({state})");
    }
    Some(out)
}

fn format_logs_human(v: &Value) -> Option<String> {
    let lines = v
        .as_array()
        .or_else(|| v.get("lines").and_then(Value::as_array))?;
    if lines.is_empty() {
        return Some("(no logs)\n".into());
    }
    let mut out = String::new();
    for entry in lines {
        if let Some(line) = entry.as_str() {
            let _ = writeln!(out, "{line}");
        } else if let Some(raw) = entry.get("raw").and_then(Value::as_str) {
            let _ = writeln!(out, "{raw}");
        } else if let Some(msg) = entry.get("message").and_then(Value::as_str) {
            let ts = entry.get("ts").and_then(Value::as_str).unwrap_or("");
            let level = entry.get("level").and_then(Value::as_str).unwrap_or("");
            let _ = writeln!(out, "{ts} {level} {msg}");
        }
    }
    Some(out)
}
