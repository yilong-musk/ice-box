// SPDX-License-Identifier: GPL-3.0-or-later

//! Machine-readable JSON and concise human-readable output formatting.

use serde_json::{json, Value};
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

/// Print a structured error to stderr (or stdout as JSON when `--json` is active
/// so Agent pipelines reading stdout still receive valid JSON on failure).
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
    match command {
        "status" => print_status_human(value),
        "sub_list" => print_subscriptions_human(value),
        "node_list" => print_nodes_human(value),
        "conn_list" => print_connections_human(value),
        "ruleset_list" => print_rule_sets_human(value),
        "logs" => print_logs_human(value),
        _ => print_json(value),
    }
}

fn print_status_human(v: &Value) {
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
    println!("core:          {core}");
    println!("mixed inbound: {host}:{port}");
    println!("capture:       {capture}");
    println!("subscriptions: {subs}");
    if let Some(tag) = v.pointer("/selected_outbound/tag").and_then(Value::as_str) {
        println!("outbound:      {tag}");
    }
}

fn print_subscriptions_human(v: &Value) {
    let Some(items) = v.as_array() else {
        print_json(v);
        return;
    };
    if items.is_empty() {
        println!("(no subscriptions)");
        return;
    }
    for item in items {
        let active = if item.get("active").and_then(Value::as_bool).unwrap_or(false) {
            "*"
        } else {
            " "
        };
        let id = item.get("id").and_then(Value::as_str).unwrap_or("-");
        let name = item.get("name").and_then(Value::as_str).unwrap_or("-");
        let nodes = item.get("node_count").and_then(Value::as_u64).unwrap_or(0);
        println!("{active} {id}  {name}  ({nodes} nodes)");
    }
}

fn print_nodes_human(v: &Value) {
    let Some(items) = v.as_array() else {
        print_json(v);
        return;
    };
    if items.is_empty() {
        println!("(no nodes)");
        return;
    }
    for item in items {
        let tag = item.get("tag").and_then(Value::as_str).unwrap_or("-");
        let kind = item
            .get("outbound_type")
            .and_then(Value::as_str)
            .unwrap_or("-");
        if let Some(now) = item.get("group_now").and_then(Value::as_str) {
            println!("{tag}  [{kind}] -> {now}");
        } else {
            println!("{tag}  [{kind}]");
        }
    }
}

fn print_connections_human(v: &Value) {
    let total = v.get("total").and_then(Value::as_u64).unwrap_or(0);
    let matched = v.get("matched").and_then(Value::as_u64).unwrap_or(0);
    println!("connections: {matched}/{total}");
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
                .and_then(Value::as_u64)
                .unwrap_or(0);
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
            println!("  {id}  {host}:{port}  [{chains}]");
        }
    }
}

fn print_rule_sets_human(v: &Value) {
    let Some(items) = v.as_array() else {
        print_json(v);
        return;
    };
    if items.is_empty() {
        println!("(no rule-sets)");
        return;
    }
    for item in items {
        let tag = item.get("tag").and_then(Value::as_str).unwrap_or("-");
        let state = item.get("state").and_then(Value::as_str).unwrap_or("-");
        println!("{tag}  ({state})");
    }
}

fn print_logs_human(v: &Value) {
    if let Some(lines) = v.get("lines").and_then(Value::as_array) {
        for entry in lines {
            if let Some(raw) = entry.get("raw").and_then(Value::as_str) {
                println!("{raw}");
            } else if let Some(msg) = entry.get("message").and_then(Value::as_str) {
                let ts = entry.get("ts").and_then(Value::as_str).unwrap_or("");
                let level = entry.get("level").and_then(Value::as_str).unwrap_or("");
                println!("{ts} {level} {msg}");
            }
        }
    } else {
        print_json(v);
    }
}
