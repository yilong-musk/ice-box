// SPDX-License-Identifier: GPL-3.0-or-later

//! Command-line parser and dispatcher for `ice-box-cli`.

use crate::daemon::{run_daemon, DaemonOptions};
use crate::env_exec::{run_env_command, run_exec_command, ShellFlavor};
use crate::exit::{
    exit_code_for_bridge_error, EXIT_ERROR, EXIT_NOT_RUNNING, EXIT_OK, EXIT_TIMEOUT, EXIT_USAGE,
};
use crate::output::{emit_error, emit_tool_result, print_json, OutputMode};
use ice_runtime::mcp::{
    self, call_tool, default_data_dir, list_tools, ping, BridgeError, McpPolicy,
};
use serde_json::{json, Value};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
struct GlobalOpts {
    data_dir: Option<PathBuf>,
    resource_dir: Option<PathBuf>,
    output: OutputMode,
    timeout: Option<Duration>,
}

impl Default for GlobalOpts {
    fn default() -> Self {
        Self {
            data_dir: None,
            resource_dir: None,
            output: OutputMode::Auto,
            timeout: None,
        }
    }
}

/// Main CLI entry point returning a process exit code.
pub fn run(args: impl IntoIterator<Item = OsString>) -> i32 {
    let raw: Vec<OsString> = args.into_iter().collect();
    // Direct compatibility with the desktop executable's `--mcp-stdio` mode.
    if raw.first().is_some_and(|a| a == "--mcp-stdio") {
        return ice_runtime::run_mcp_stdio_bridge(raw.into_iter().skip(1));
    }

    let mut global = GlobalOpts::default();
    let mut rest: Vec<String> = Vec::new();
    let mut iter = raw.into_iter();
    let mut passthrough = false;

    while let Some(arg) = iter.next() {
        let s = arg.to_string_lossy().into_owned();
        if passthrough {
            rest.push(s);
            continue;
        }
        if s == "--" {
            passthrough = true;
            rest.push(s);
            continue;
        }
        match s.as_str() {
            "--data-dir" => {
                let Some(val) = iter.next() else {
                    emit_error(global.output, "cli.usage", "--data-dir requires a path");
                    return EXIT_USAGE;
                };
                global.data_dir = Some(PathBuf::from(val));
            }
            "--resource-dir" => {
                let Some(val) = iter.next() else {
                    emit_error(global.output, "cli.usage", "--resource-dir requires a path");
                    return EXIT_USAGE;
                };
                global.resource_dir = Some(PathBuf::from(val));
            }
            "--json" => global.output = OutputMode::Json,
            "--human" => global.output = OutputMode::Human,
            "--timeout" => {
                let Some(val) = iter.next() else {
                    emit_error(
                        global.output,
                        "cli.usage",
                        "--timeout requires a number of seconds",
                    );
                    return EXIT_USAGE;
                };
                match val.to_string_lossy().parse::<f64>() {
                    Ok(secs) if secs.is_finite() && secs > 0.0 => {
                        global.timeout = Some(Duration::from_secs_f64(secs));
                    }
                    _ => {
                        emit_error(
                            global.output,
                            "cli.usage",
                            "--timeout must be a positive number of seconds",
                        );
                        return EXIT_USAGE;
                    }
                }
            }
            _ => rest.push(s),
        }
    }

    if rest.is_empty() || matches!(rest[0].as_str(), "-h" | "--help" | "help") {
        print_root_help();
        return EXIT_OK;
    }
    if matches!(rest[0].as_str(), "-V" | "--version" | "version") {
        if global.output.wants_json_for_raw() {
            print_json(&json!({
                "name": "ice-box-cli",
                "version": VERSION,
                "core_version": ice_core::BUNDLED_SINGBOX_VERSION,
                "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            }));
        } else {
            println!(
                "ice-box-cli {VERSION} (sing-box {})",
                ice_core::BUNDLED_SINGBOX_VERSION
            );
        }
        return EXIT_OK;
    }

    let cmd = rest[0].as_str();
    let sub = &rest[1..];

    // Offline commands that do not require resolving a running service.
    match cmd {
        "tools" => return handle_tools(&global, sub),
        "schema" => return handle_schema(&global, sub),
        _ => {}
    }

    let data_dir = match resolve_data_dir(global.data_dir.clone()) {
        Ok(dir) => dir,
        Err(msg) => {
            emit_error(global.output, "cli.usage", &msg);
            return EXIT_USAGE;
        }
    };

    match cmd {
        "daemon" | "serve" => handle_daemon(data_dir, &global, sub),
        "mcp" => {
            let mut bridge_args = vec![OsString::from("--data-dir"), data_dir.into_os_string()];
            bridge_args.extend(sub.iter().map(OsString::from));
            ice_runtime::run_mcp_stdio_bridge(bridge_args)
        }
        "ping" => handle_ping(&data_dir, &global),
        "wait-ready" => handle_wait_ready(&data_dir, &global, sub),
        "status" => invoke_and_print(&data_dir, &global, "status", "get_status", json!({})),
        "start" => handle_start(&data_dir, &global, sub),
        "stop" => invoke_and_print(&data_dir, &global, "stop", "stop_proxy_service", json!({})),
        "core" => handle_core(&data_dir, &global, sub),
        "mode" => handle_mode(&data_dir, &global, sub),
        "env" => handle_env(&data_dir, &global, sub),
        "exec" => handle_exec(&data_dir, &global, sub),
        "sub" | "subscription" | "subscriptions" => handle_sub(&data_dir, &global, sub),
        "node" | "nodes" => handle_node(&data_dir, &global, sub),
        "rule" | "rules" => handle_rule(&data_dir, &global, sub),
        "ruleset" | "rulesets" => handle_ruleset(&data_dir, &global, sub),
        "conn" | "connections" => handle_conn(&data_dir, &global, sub),
        "traffic" => handle_traffic(&data_dir, &global, sub),
        "logs" => handle_logs(&data_dir, &global, sub),
        "config" => handle_config(&data_dir, &global),
        "settings" => handle_settings(&data_dir, &global, sub),
        "tun" => handle_tun(&data_dir, &global, sub),
        "call" => handle_call(&data_dir, &global, sub),
        unknown => {
            emit_error(
                global.output,
                "cli.usage",
                &format!("unknown command '{unknown}'; run `ice-box-cli --help`"),
            );
            EXIT_USAGE
        }
    }
}

pub fn resolve_data_dir(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(dir) = explicit {
        return Ok(dir);
    }
    if let Some(dir) = std::env::var_os("ICE_BOX_DATA_DIR").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    default_data_dir().ok_or_else(|| {
        "could not resolve ice-box data directory; pass --data-dir or set ICE_BOX_DATA_DIR".into()
    })
}

fn invoke_and_print(
    data_dir: &Path,
    global: &GlobalOpts,
    display_cmd: &str,
    tool_name: &str,
    args: Value,
) -> i32 {
    match call_tool(data_dir, tool_name, args, global.timeout) {
        Ok(out) => {
            emit_tool_result(global.output, display_cmd, &out.value);
            EXIT_OK
        }
        Err(err) => {
            emit_error(global.output, err.code(), err.message());
            exit_code_for_bridge_error(&err)
        }
    }
}

fn handle_daemon(data_dir: PathBuf, global: &GlobalOpts, args: &[String]) -> i32 {
    let mut auto_start_core = true;
    let mut start_service = false;
    let mut read_only = false;
    let mut allow_secrets = true;
    let mut allow_interactive_elevation = false;
    let mut print_ready = false;

    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!(
                    "Usage: ice-box-cli daemon [OPTIONS]\n\n\
                     Run the headless ice-box runtime in the foreground.\n\n\
                     Options:\n\
                       --data-dir <PATH>              Override data directory\n\
                       --resource-dir <PATH>          Override bundled sing-box/geoip directory\n\
                       --no-auto-start                Do not start sing-box automatically on boot\n\
                       --start-service                Enable proxy capture automatically after boot\n\
                       --read-only                    Allow read-only control/MCP tools only\n\
                       --no-secrets                   Disable credential export tools (export_subscription)\n\
                       --allow-interactive-elevation  Allow GUI OS elevation prompts\n\
                       --print-ready                  Print one JSON line once the control listener is ready"
                );
                return EXIT_OK;
            }
            "--no-auto-start" => auto_start_core = false,
            "--start-service" => start_service = true,
            "--read-only" => read_only = true,
            "--no-secrets" => allow_secrets = false,
            "--allow-interactive-elevation" => allow_interactive_elevation = true,
            "--print-ready" => print_ready = true,
            other => {
                emit_error(
                    global.output,
                    "cli.usage",
                    &format!("unknown daemon option '{other}'"),
                );
                return EXIT_USAGE;
            }
        }
    }

    run_daemon(DaemonOptions {
        data_dir,
        resource_dir: global.resource_dir.clone(),
        auto_start_core,
        start_service,
        policy: McpPolicy {
            read_only,
            allow_secrets,
        },
        allow_interactive_elevation,
        print_ready,
    })
}

fn handle_ping(data_dir: &Path, global: &GlobalOpts) -> i32 {
    match ping(data_dir, global.timeout) {
        Ok(()) => {
            if global.output.wants_json() {
                print_json(&json!({"ok": true}));
            } else {
                println!("ok");
            }
            EXIT_OK
        }
        Err(err) => {
            emit_error(global.output, err.code(), err.message());
            exit_code_for_bridge_error(&err)
        }
    }
}

fn handle_wait_ready(data_dir: &Path, global: &GlobalOpts, args: &[String]) -> i32 {
    let mut require_core = false;
    let mut require_service = false;
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!(
                    "Usage: ice-box-cli wait-ready [--core] [--service] [--timeout <SECS>]\n\n\
                     Poll until the local control service is reachable (and optionally the core is running)."
                );
                return EXIT_OK;
            }
            "--core" => require_core = true,
            "--service" => require_service = true,
            other => {
                emit_error(
                    global.output,
                    "cli.usage",
                    &format!("unknown wait-ready option '{other}'"),
                );
                return EXIT_USAGE;
            }
        }
    }
    let deadline = Instant::now() + global.timeout.unwrap_or(Duration::from_secs(30));
    let mut last_err: Option<BridgeError> = None;
    loop {
        match call_tool(
            data_dir,
            "get_status",
            json!({}),
            Some(Duration::from_secs(3)),
        ) {
            Ok(out) => {
                let core_running =
                    out.value.pointer("/core/status").and_then(Value::as_str) == Some("running");
                let capture_active = out
                    .value
                    .get("traffic_capture")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s != "inactive");
                if (!require_core || core_running) && (!require_service || capture_active) {
                    emit_tool_result(global.output, "status", &out.value);
                    return EXIT_OK;
                }
            }
            Err(err) => {
                last_err = Some(err);
            }
        }
        if Instant::now() >= deadline {
            let detail = last_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| "condition not satisfied before timeout".into());
            emit_error(
                global.output,
                "cli.timeout",
                &format!("wait-ready timed out: {detail}"),
            );
            return EXIT_TIMEOUT;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn handle_start(data_dir: &Path, global: &GlobalOpts, args: &[String]) -> i32 {
    let mut wait = false;
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Usage: ice-box-cli start [--wait] [--timeout <SECS>]");
                return EXIT_OK;
            }
            "--wait" => wait = true,
            other => {
                emit_error(
                    global.output,
                    "cli.usage",
                    &format!("unknown start option '{other}'"),
                );
                return EXIT_USAGE;
            }
        }
    }
    match call_tool(data_dir, "start_proxy_service", json!({}), global.timeout) {
        Ok(out) => {
            if wait {
                return handle_wait_ready(data_dir, global, &["--core".to_string()]);
            }
            emit_tool_result(global.output, "start", &out.value);
            EXIT_OK
        }
        Err(err) => {
            emit_error(global.output, err.code(), err.message());
            exit_code_for_bridge_error(&err)
        }
    }
}

fn handle_core(data_dir: &Path, global: &GlobalOpts, args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("start") => invoke_and_print(data_dir, global, "core_start", "start_core", json!({})),
        Some("stop") => invoke_and_print(data_dir, global, "core_stop", "stop_core", json!({})),
        Some("-h" | "--help") | None => {
            println!("Usage: ice-box-cli core <start|stop>");
            if args.is_empty() {
                EXIT_USAGE
            } else {
                EXIT_OK
            }
        }
        Some(other) => {
            emit_error(
                global.output,
                "cli.usage",
                &format!("unknown core subcommand '{other}' (expected start | stop)"),
            );
            EXIT_USAGE
        }
    }
}

fn handle_mode(data_dir: &Path, global: &GlobalOpts, args: &[String]) -> i32 {
    let Some(mode) = args.first().map(String::as_str) else {
        emit_error(
            global.output,
            "cli.usage",
            "usage: ice-box-cli mode <rule|global|direct>",
        );
        return EXIT_USAGE;
    };
    if matches!(mode, "-h" | "--help") {
        println!("Usage: ice-box-cli mode <rule|global|direct>");
        return EXIT_OK;
    }
    if !matches!(mode, "rule" | "global" | "direct") {
        emit_error(
            global.output,
            "cli.usage",
            &format!("invalid mode '{mode}' (expected rule | global | direct)"),
        );
        return EXIT_USAGE;
    }
    invoke_and_print(
        data_dir,
        global,
        "mode",
        "set_proxy_mode",
        json!({ "mode": mode }),
    )
}

fn handle_env(data_dir: &Path, global: &GlobalOpts, args: &[String]) -> i32 {
    let mut flavor = ShellFlavor::Auto;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!(
                    "Usage: ice-box-cli env [--shell auto|sh|bash|zsh|fish|pwsh|cmd] [--json]"
                );
                return EXIT_OK;
            }
            "--shell" => {
                let Some(val) = args.get(i + 1) else {
                    emit_error(global.output, "cli.usage", "--shell requires a value");
                    return EXIT_USAGE;
                };
                match ShellFlavor::parse(val) {
                    Ok(parsed) => flavor = parsed,
                    Err(msg) => {
                        emit_error(global.output, "cli.usage", &msg);
                        return EXIT_USAGE;
                    }
                }
                i += 2;
            }
            other => {
                emit_error(
                    global.output,
                    "cli.usage",
                    &format!("unknown env option '{other}'"),
                );
                return EXIT_USAGE;
            }
        }
    }
    run_env_command(data_dir, flavor, global.output, global.timeout)
}

fn handle_exec(data_dir: &Path, global: &GlobalOpts, args: &[String]) -> i32 {
    if args.first().is_some_and(|a| a == "-h" || a == "--help") {
        println!(
            "Usage: ice-box-cli exec -- <COMMAND> [ARGS...]\n\n\
             Run a command with session-only HTTP_PROXY/HTTPS_PROXY/ALL_PROXY variables pointing at ice-box."
        );
        return EXIT_OK;
    }
    let cmd_slice = if args.first().is_some_and(|a| a == "--") {
        &args[1..]
    } else {
        args
    };
    run_exec_command(data_dir, cmd_slice, global.output, global.timeout)
}

fn handle_sub(data_dir: &Path, global: &GlobalOpts, args: &[String]) -> i32 {
    let Some(action) = args.first().map(String::as_str) else {
        println!(
            "Usage: ice-box-cli sub <list|add|import|update|use|deactivate|rm|auto-update|export>"
        );
        return EXIT_USAGE;
    };
    let tail = &args[1..];
    match action {
        "-h" | "--help" => {
            println!(
                "Usage: ice-box-cli sub <SUBCOMMAND>\n\n\
                 Subcommands:\n\
                   list                                    List subscriptions\n\
                   add <URL> [--name <N>] [--activate]     Add remote subscription\n\
                   import <FILE|-> [--name <N>] [--activate]  Import local document\n\
                   update <ID>                             Refresh subscription\n\
                   use <ID>                                Activate subscription\n\
                   deactivate <ID>                         Deactivate subscription\n\
                   rm <ID>                                 Remove subscription\n\
                   auto-update <ID> <on|off> [--interval 1h|3h|6h|12h|24h]\n\
                   export <ID> [--kind url|singbox]        Export subscription"
            );
            EXIT_OK
        }
        "list" => invoke_and_print(
            data_dir,
            global,
            "sub_list",
            "list_subscriptions",
            json!({}),
        ),
        "add" => {
            let Some(url) = tail.first() else {
                emit_error(
                    global.output,
                    "cli.usage",
                    "usage: ice-box-cli sub add <URL> [--name <NAME>] [--auto-update] [--interval 1h|3h|6h|12h|24h] [--activate]",
                );
                return EXIT_USAGE;
            };
            let mut name: Option<String> = None;
            let mut auto_update = false;
            let mut interval: Option<String> = None;
            let mut activate = false;
            let mut i = 1;
            while i < tail.len() {
                match tail[i].as_str() {
                    "--name" => {
                        let Some(val) = tail.get(i + 1) else {
                            emit_error(global.output, "cli.usage", "--name requires a value");
                            return EXIT_USAGE;
                        };
                        name = Some(val.clone());
                        i += 2;
                    }
                    "--auto-update" => {
                        auto_update = true;
                        i += 1;
                    }
                    "--interval" => {
                        let Some(val) = tail.get(i + 1) else {
                            emit_error(global.output, "cli.usage", "--interval requires a value");
                            return EXIT_USAGE;
                        };
                        match normalize_auto_update_interval(val) {
                            Ok(mapped) => interval = Some(mapped.to_string()),
                            Err(msg) => {
                                emit_error(global.output, "cli.usage", &msg);
                                return EXIT_USAGE;
                            }
                        }
                        i += 2;
                    }
                    "--activate" => {
                        activate = true;
                        i += 1;
                    }
                    other => {
                        emit_error(
                            global.output,
                            "cli.usage",
                            &format!("unknown option '{other}'"),
                        );
                        return EXIT_USAGE;
                    }
                }
            }
            let mut payload = json!({ "url": url, "auto_update": auto_update });
            if let Some(name) = name {
                payload["name"] = Value::String(name);
            }
            if let Some(interval) = interval {
                payload["auto_update_interval"] = Value::String(interval);
            }
            invoke_sub_and_maybe_activate(
                data_dir,
                global,
                "sub_add",
                "add_subscription",
                payload,
                activate,
            )
        }
        "import" => {
            let Some(file) = tail.first() else {
                emit_error(
                    global.output,
                    "cli.usage",
                    "usage: ice-box-cli sub import <FILE|-> [--name <NAME>] [--activate]",
                );
                return EXIT_USAGE;
            };
            let mut name: Option<String> = None;
            let mut activate = false;
            let mut i = 1;
            while i < tail.len() {
                match tail[i].as_str() {
                    "--name" => {
                        let Some(val) = tail.get(i + 1) else {
                            emit_error(global.output, "cli.usage", "--name requires a value");
                            return EXIT_USAGE;
                        };
                        name = Some(val.clone());
                        i += 2;
                    }
                    "--activate" => {
                        activate = true;
                        i += 1;
                    }
                    other => {
                        emit_error(
                            global.output,
                            "cli.usage",
                            &format!("unknown option '{other}'"),
                        );
                        return EXIT_USAGE;
                    }
                }
            }
            let content = if file == "-" {
                let mut buf = String::new();
                if let Err(err) = std::io::stdin().read_to_string(&mut buf) {
                    emit_error(global.output, "cli.io", &format!("read stdin: {err}"));
                    return EXIT_ERROR;
                }
                buf
            } else {
                match std::fs::read_to_string(file) {
                    Ok(text) => text,
                    Err(err) => {
                        emit_error(global.output, "cli.io", &format!("read '{file}': {err}"));
                        return EXIT_ERROR;
                    }
                }
            };
            let mut payload = json!({ "content": content });
            if let Some(name) = name {
                payload["name"] = Value::String(name);
            }
            invoke_sub_and_maybe_activate(
                data_dir,
                global,
                "sub_import",
                "import_subscription_document",
                payload,
                activate,
            )
        }
        "update" => {
            let Some(id) = tail.first() else {
                emit_error(
                    global.output,
                    "cli.usage",
                    "usage: ice-box-cli sub update <ID>",
                );
                return EXIT_USAGE;
            };
            invoke_and_print(
                data_dir,
                global,
                "sub_update",
                "update_subscription",
                json!({ "id": id }),
            )
        }
        "use" => {
            let Some(id) = tail.first() else {
                emit_error(
                    global.output,
                    "cli.usage",
                    "usage: ice-box-cli sub use <ID>",
                );
                return EXIT_USAGE;
            };
            invoke_and_print(
                data_dir,
                global,
                "sub_use",
                "set_subscription_active",
                json!({ "id": id, "active": true }),
            )
        }
        "deactivate" => {
            let Some(id) = tail.first() else {
                emit_error(
                    global.output,
                    "cli.usage",
                    "usage: ice-box-cli sub deactivate <ID>",
                );
                return EXIT_USAGE;
            };
            invoke_and_print(
                data_dir,
                global,
                "sub_deactivate",
                "set_subscription_active",
                json!({ "id": id, "active": false }),
            )
        }
        "rm" | "remove" => {
            let Some(id) = tail.first() else {
                emit_error(global.output, "cli.usage", "usage: ice-box-cli sub rm <ID>");
                return EXIT_USAGE;
            };
            invoke_and_print(
                data_dir,
                global,
                "sub_rm",
                "remove_subscription",
                json!({ "id": id }),
            )
        }
        "auto-update" => {
            let (Some(id), Some(flag)) = (tail.first(), tail.get(1)) else {
                emit_error(
                    global.output,
                    "cli.usage",
                    "usage: ice-box-cli sub auto-update <ID> <on|off> [--interval 1h|3h|6h|12h|24h]",
                );
                return EXIT_USAGE;
            };
            let enabled = match flag.as_str() {
                "on" | "true" | "1" => true,
                "off" | "false" | "0" => false,
                other => {
                    emit_error(
                        global.output,
                        "cli.usage",
                        &format!("expected on|off, got '{other}'"),
                    );
                    return EXIT_USAGE;
                }
            };
            let mut interval: Option<String> = None;
            let mut i = 2;
            while i < tail.len() {
                if tail[i] == "--interval" {
                    let Some(val) = tail.get(i + 1) else {
                        emit_error(global.output, "cli.usage", "--interval requires a value");
                        return EXIT_USAGE;
                    };
                    match normalize_auto_update_interval(val) {
                        Ok(mapped) => interval = Some(mapped.to_string()),
                        Err(msg) => {
                            emit_error(global.output, "cli.usage", &msg);
                            return EXIT_USAGE;
                        }
                    }
                    i += 2;
                } else {
                    emit_error(
                        global.output,
                        "cli.usage",
                        &format!("unknown option '{}'", tail[i]),
                    );
                    return EXIT_USAGE;
                }
            }
            let mut payload = json!({ "id": id, "auto_update": enabled });
            if let Some(interval) = interval {
                payload["auto_update_interval"] = Value::String(interval);
            }
            invoke_and_print(
                data_dir,
                global,
                "sub_auto_update",
                "set_subscription_auto_update",
                payload,
            )
        }
        "export" => {
            let Some(id) = tail.first() else {
                emit_error(
                    global.output,
                    "cli.usage",
                    "usage: ice-box-cli sub export <ID> [--kind url|singbox]",
                );
                return EXIT_USAGE;
            };
            let mut kind = "singbox".to_string();
            let mut i = 1;
            while i < tail.len() {
                if tail[i] == "--kind" {
                    let Some(val) = tail.get(i + 1) else {
                        emit_error(global.output, "cli.usage", "--kind requires url|singbox");
                        return EXIT_USAGE;
                    };
                    kind = val.clone();
                    i += 2;
                } else {
                    emit_error(
                        global.output,
                        "cli.usage",
                        &format!("unknown option '{}'", tail[i]),
                    );
                    return EXIT_USAGE;
                }
            }
            invoke_and_print(
                data_dir,
                global,
                "sub_export",
                "export_subscription",
                json!({ "id": id, "kind": kind }),
            )
        }
        other => {
            emit_error(
                global.output,
                "cli.usage",
                &format!("unknown sub subcommand '{other}'"),
            );
            EXIT_USAGE
        }
    }
}

fn handle_node(data_dir: &Path, global: &GlobalOpts, args: &[String]) -> i32 {
    let Some(action) = args.first().map(String::as_str) else {
        println!("Usage: ice-box-cli node <list|use|group|test>");
        return EXIT_USAGE;
    };
    let tail = &args[1..];
    match action {
        "-h" | "--help" => {
            println!(
                "Usage: ice-box-cli node <SUBCOMMAND>\n\n\
                 Subcommands:\n\
                   list                     List nodes and strategy groups\n\
                   use <TAG>                Select outbound node\n\
                   group <GROUP> <MEMBER>   Select member within a strategy group\n\
                   test <TAG>               Probe node latency"
            );
            EXIT_OK
        }
        "list" => invoke_and_print(data_dir, global, "node_list", "list_nodes", json!({})),
        "use" => {
            let Some(tag) = tail.first() else {
                emit_error(
                    global.output,
                    "cli.usage",
                    "usage: ice-box-cli node use <TAG>",
                );
                return EXIT_USAGE;
            };
            invoke_and_print(
                data_dir,
                global,
                "node_use",
                "select_node",
                json!({ "tag": tag }),
            )
        }
        "group" => {
            let (Some(group), Some(member)) = (tail.first(), tail.get(1)) else {
                emit_error(
                    global.output,
                    "cli.usage",
                    "usage: ice-box-cli node group <GROUP> <MEMBER>",
                );
                return EXIT_USAGE;
            };
            invoke_and_print(
                data_dir,
                global,
                "node_group",
                "select_group_member",
                json!({ "group": group, "member": member }),
            )
        }
        "test" => {
            let Some(tag) = tail.first() else {
                emit_error(
                    global.output,
                    "cli.usage",
                    "usage: ice-box-cli node test <TAG>",
                );
                return EXIT_USAGE;
            };
            invoke_and_print(
                data_dir,
                global,
                "node_test",
                "test_node_delay",
                json!({ "tag": tag }),
            )
        }
        other => {
            emit_error(
                global.output,
                "cli.usage",
                &format!("unknown node subcommand '{other}'"),
            );
            EXIT_USAGE
        }
    }
}

fn handle_rule(data_dir: &Path, global: &GlobalOpts, args: &[String]) -> i32 {
    let Some(action) = args.first().map(String::as_str) else {
        println!("Usage: ice-box-cli rule <overview|list|enable|disable|add|update|rm>");
        return EXIT_USAGE;
    };
    let tail = &args[1..];
    match action {
        "-h" | "--help" => {
            println!(
                concat!(
                    "Usage: ice-box-cli rule <SUBCOMMAND>\n\n",
                    "Subcommands:\n",
                    "  overview\n",
                    "  list [--keyword <KW>] [--type <T>] [--disabled all|disabled|enabled] [--custom] [--limit <N>] [--offset <N>]\n",
                    "  enable <FINGERPRINT>\n",
                    "  disable <FINGERPRINT>\n",
                    "  add <JSON|--stdin>\n",
                    "  update <FINGERPRINT> <JSON|--stdin>\n",
                    "  rm <FINGERPRINT>"
                )
            );
            EXIT_OK
        }
        "overview" => invoke_and_print(
            data_dir,
            global,
            "rule_overview",
            "get_rule_overview",
            json!({}),
        ),
        "list" => {
            let mut payload = serde_json::Map::new();
            let mut i = 0;
            while i < tail.len() {
                match tail[i].as_str() {
                    "--keyword" => {
                        let Some(v) = tail.get(i + 1) else {
                            emit_error(global.output, "cli.usage", "--keyword requires a value");
                            return EXIT_USAGE;
                        };
                        payload.insert("keyword".into(), Value::String(v.clone()));
                        i += 2;
                    }
                    "--type" => {
                        let Some(v) = tail.get(i + 1) else {
                            emit_error(global.output, "cli.usage", "--type requires a value");
                            return EXIT_USAGE;
                        };
                        payload.insert("type".into(), Value::String(v.clone()));
                        i += 2;
                    }
                    "--disabled" => {
                        let Some(v) = tail.get(i + 1) else {
                            emit_error(
                                global.output,
                                "cli.usage",
                                "--disabled requires all|disabled|enabled",
                            );
                            return EXIT_USAGE;
                        };
                        payload.insert("disabled".into(), Value::String(v.clone()));
                        i += 2;
                    }
                    "--disabled-only" => {
                        payload.insert("disabled".into(), Value::String("disabled".into()));
                        i += 1;
                    }
                    "--custom" => {
                        payload.insert("custom".into(), Value::Bool(true));
                        i += 1;
                    }
                    "--limit" => {
                        let Some(v) = tail.get(i + 1).and_then(|s| s.parse::<u64>().ok()) else {
                            emit_error(global.output, "cli.usage", "--limit requires an integer");
                            return EXIT_USAGE;
                        };
                        payload.insert("limit".into(), json!(v));
                        i += 2;
                    }
                    "--offset" => {
                        let Some(v) = tail.get(i + 1).and_then(|s| s.parse::<u64>().ok()) else {
                            emit_error(global.output, "cli.usage", "--offset requires an integer");
                            return EXIT_USAGE;
                        };
                        payload.insert("offset".into(), json!(v));
                        i += 2;
                    }
                    other => {
                        emit_error(
                            global.output,
                            "cli.usage",
                            &format!("unknown option '{other}'"),
                        );
                        return EXIT_USAGE;
                    }
                }
            }
            invoke_and_print(
                data_dir,
                global,
                "rule_list",
                "list_rules",
                Value::Object(payload),
            )
        }
        "enable" | "disable" => {
            let Some(fp) = tail.first() else {
                emit_error(
                    global.output,
                    "cli.usage",
                    &format!("usage: ice-box-cli rule {action} <FINGERPRINT>"),
                );
                return EXIT_USAGE;
            };
            invoke_and_print(
                data_dir,
                global,
                "rule_toggle",
                "set_rule_disabled",
                json!({ "fingerprint": fp, "disabled": action == "disable" }),
            )
        }
        "add" => {
            let rule = match parse_json_arg(tail) {
                Ok(v) => v,
                Err(msg) => {
                    emit_error(global.output, "cli.usage", &msg);
                    return EXIT_USAGE;
                }
            };
            invoke_and_print(
                data_dir,
                global,
                "rule_add",
                "add_custom_rule",
                json!({ "rule": rule }),
            )
        }
        "update" => {
            let Some(fp) = tail.first() else {
                emit_error(
                    global.output,
                    "cli.usage",
                    "usage: ice-box-cli rule update <FINGERPRINT> <JSON|--stdin>",
                );
                return EXIT_USAGE;
            };
            let rule = match parse_json_arg(&tail[1..]) {
                Ok(v) => v,
                Err(msg) => {
                    emit_error(global.output, "cli.usage", &msg);
                    return EXIT_USAGE;
                }
            };
            invoke_and_print(
                data_dir,
                global,
                "rule_update",
                "update_custom_rule",
                json!({ "fingerprint": fp, "rule": rule }),
            )
        }
        "rm" | "remove" => {
            let Some(fp) = tail.first() else {
                emit_error(
                    global.output,
                    "cli.usage",
                    "usage: ice-box-cli rule rm <FINGERPRINT>",
                );
                return EXIT_USAGE;
            };
            invoke_and_print(
                data_dir,
                global,
                "rule_rm",
                "remove_custom_rule",
                json!({ "fingerprint": fp }),
            )
        }
        other => {
            emit_error(
                global.output,
                "cli.usage",
                &format!("unknown rule subcommand '{other}'"),
            );
            EXIT_USAGE
        }
    }
}

fn handle_ruleset(data_dir: &Path, global: &GlobalOpts, args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("list") => invoke_and_print(
            data_dir,
            global,
            "ruleset_list",
            "list_rule_sets",
            json!({}),
        ),
        Some("refresh") => invoke_and_print(
            data_dir,
            global,
            "ruleset_refresh",
            "refresh_rule_sets",
            json!({}),
        ),
        Some("-h" | "--help") | None => {
            println!("Usage: ice-box-cli ruleset <list|refresh>");
            if args.is_empty() {
                EXIT_USAGE
            } else {
                EXIT_OK
            }
        }
        Some(other) => {
            emit_error(
                global.output,
                "cli.usage",
                &format!("unknown ruleset subcommand '{other}'"),
            );
            EXIT_USAGE
        }
    }
}

fn handle_conn(data_dir: &Path, global: &GlobalOpts, args: &[String]) -> i32 {
    let Some(action) = args.first().map(String::as_str) else {
        println!("Usage: ice-box-cli conn <list|close|close-all>");
        return EXIT_USAGE;
    };
    let tail = &args[1..];
    match action {
        "-h" | "--help" => {
            println!(
                "Usage: ice-box-cli conn <list [--keyword <KW>] [--limit <N>] | close <ID> | close-all>"
            );
            EXIT_OK
        }
        "list" => {
            let mut payload = serde_json::Map::new();
            let mut i = 0;
            while i < tail.len() {
                match tail[i].as_str() {
                    "--keyword" => {
                        let Some(v) = tail.get(i + 1) else {
                            emit_error(global.output, "cli.usage", "--keyword requires a value");
                            return EXIT_USAGE;
                        };
                        payload.insert("keyword".into(), Value::String(v.clone()));
                        i += 2;
                    }
                    "--limit" => {
                        let Some(v) = tail.get(i + 1).and_then(|s| s.parse::<u64>().ok()) else {
                            emit_error(global.output, "cli.usage", "--limit requires an integer");
                            return EXIT_USAGE;
                        };
                        payload.insert("limit".into(), json!(v));
                        i += 2;
                    }
                    other => {
                        emit_error(
                            global.output,
                            "cli.usage",
                            &format!("unknown option '{other}'"),
                        );
                        return EXIT_USAGE;
                    }
                }
            }
            invoke_and_print(
                data_dir,
                global,
                "conn_list",
                "list_connections",
                Value::Object(payload),
            )
        }
        "close" => {
            let Some(id) = tail.first() else {
                emit_error(
                    global.output,
                    "cli.usage",
                    "usage: ice-box-cli conn close <ID>",
                );
                return EXIT_USAGE;
            };
            invoke_and_print(
                data_dir,
                global,
                "conn_close",
                "close_connection",
                json!({ "id": id }),
            )
        }
        "close-all" => invoke_and_print(
            data_dir,
            global,
            "conn_close_all",
            "close_all_connections",
            json!({}),
        ),
        other => {
            emit_error(
                global.output,
                "cli.usage",
                &format!("unknown conn subcommand '{other}'"),
            );
            EXIT_USAGE
        }
    }
}

fn handle_traffic(data_dir: &Path, global: &GlobalOpts, args: &[String]) -> i32 {
    if args.is_empty() {
        return invoke_and_print(
            data_dir,
            global,
            "traffic",
            "get_traffic_snapshot",
            json!({}),
        );
    }
    if args[0] == "-h" || args[0] == "--help" {
        println!("Usage: ice-box-cli traffic [--since <CURSOR>]");
        return EXIT_OK;
    }
    if args[0] == "--since" {
        let Some(cursor) = args.get(1).and_then(|s| s.parse::<u64>().ok()) else {
            emit_error(
                global.output,
                "cli.usage",
                "--since requires a non-negative integer cursor",
            );
            return EXIT_USAGE;
        };
        return invoke_and_print(
            data_dir,
            global,
            "traffic",
            "get_traffic_since",
            json!({ "cursor": cursor }),
        );
    }
    emit_error(
        global.output,
        "cli.usage",
        &format!("unknown traffic option '{}'", args[0]),
    );
    EXIT_USAGE
}

fn handle_logs(data_dir: &Path, global: &GlobalOpts, args: &[String]) -> i32 {
    let mut payload = serde_json::Map::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: ice-box-cli logs [-n <LINES>] [--all]");
                return EXIT_OK;
            }
            "-n" | "--lines" => {
                let Some(n) = args.get(i + 1).and_then(|s| s.parse::<u64>().ok()) else {
                    emit_error(global.output, "cli.usage", "-n requires a positive integer");
                    return EXIT_USAGE;
                };
                payload.insert("n".into(), json!(n));
                i += 2;
            }
            "--all" => {
                payload.insert("all".into(), Value::Bool(true));
                i += 1;
            }
            other => {
                emit_error(
                    global.output,
                    "cli.usage",
                    &format!("unknown logs option '{other}'"),
                );
                return EXIT_USAGE;
            }
        }
    }
    invoke_and_print(data_dir, global, "logs", "get_logs", Value::Object(payload))
}

fn handle_config(data_dir: &Path, global: &GlobalOpts) -> i32 {
    match call_tool(data_dir, "get_runtime_config", json!({}), global.timeout) {
        Ok(out) => {
            if global.output.wants_json_for_raw() {
                print_json(&out.value);
            } else if let Some(cfg) = out.value.get("config").and_then(Value::as_str) {
                println!("{cfg}");
            } else {
                print_json(&out.value);
            }
            EXIT_OK
        }
        Err(err) => {
            emit_error(global.output, err.code(), err.message());
            exit_code_for_bridge_error(&err)
        }
    }
}

fn handle_settings(data_dir: &Path, global: &GlobalOpts, args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("get") | None => {
            invoke_and_print(data_dir, global, "settings_get", "get_settings", json!({}))
        }
        Some("set") => {
            let patch = match parse_json_arg(&args[1..]) {
                Ok(v) => v,
                Err(msg) => {
                    emit_error(global.output, "cli.usage", &msg);
                    return EXIT_USAGE;
                }
            };
            invoke_and_print(
                data_dir,
                global,
                "settings_set",
                "update_settings",
                json!({ "patch": patch }),
            )
        }
        Some("-h" | "--help") => {
            println!("Usage: ice-box-cli settings <get | set <JSON|--stdin>>");
            EXIT_OK
        }
        Some(other) => {
            emit_error(
                global.output,
                "cli.usage",
                &format!("unknown settings subcommand '{other}'"),
            );
            EXIT_USAGE
        }
    }
}

fn handle_tun(data_dir: &Path, global: &GlobalOpts, args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("recover") => {
            invoke_and_print(data_dir, global, "tun_recover", "recover_tun", json!({}))
        }
        Some("install-helper") => invoke_and_print(
            data_dir,
            global,
            "tun_install",
            "install_tun_helper",
            json!({}),
        ),
        Some("uninstall-helper") => invoke_and_print(
            data_dir,
            global,
            "tun_uninstall",
            "uninstall_tun_helper",
            json!({}),
        ),
        Some("ensure-elevation") => invoke_and_print(
            data_dir,
            global,
            "tun_ensure",
            "ensure_tun_elevation",
            json!({}),
        ),
        Some("remove-elevation") => invoke_and_print(
            data_dir,
            global,
            "tun_remove",
            "remove_tun_elevation",
            json!({}),
        ),
        Some("-h" | "--help") | None => {
            println!(
                "Usage: ice-box-cli tun <recover|install-helper|uninstall-helper|ensure-elevation|remove-elevation>"
            );
            if args.is_empty() {
                EXIT_USAGE
            } else {
                EXIT_OK
            }
        }
        Some(other) => {
            emit_error(
                global.output,
                "cli.usage",
                &format!("unknown tun subcommand '{other}'"),
            );
            EXIT_USAGE
        }
    }
}

fn handle_tools(global: &GlobalOpts, args: &[String]) -> i32 {
    let mut live = false;
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!(
                    "Usage: ice-box-cli tools [--live]\n\n\
                     List available control/MCP tools. Uses the compiled tool catalog by default\n\
                     (works offline), or queries a running service with --live."
                );
                return EXIT_OK;
            }
            "--live" => live = true,
            other => {
                emit_error(
                    global.output,
                    "cli.usage",
                    &format!("unknown tools option '{other}'"),
                );
                return EXIT_USAGE;
            }
        }
    }
    let catalog = if live {
        let data_dir = match resolve_data_dir(global.data_dir.clone()) {
            Ok(dir) => dir,
            Err(msg) => {
                emit_error(global.output, "cli.usage", &msg);
                return EXIT_USAGE;
            }
        };
        match list_tools(&data_dir, global.timeout) {
            Ok(list) => list,
            Err(err) => {
                emit_error(global.output, err.code(), err.message());
                return exit_code_for_bridge_error(&err);
            }
        }
    } else {
        mcp::tools()
    };

    if matches!(global.output, OutputMode::Human) {
        for tool in &catalog {
            let name = tool.get("name").and_then(Value::as_str).unwrap_or("-");
            let ro = tool
                .pointer("/annotations/readOnlyHint")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let tag = if ro { "read " } else { "write" };
            let desc = tool
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            println!("{tag}  {name:<30} {desc}");
        }
    } else {
        let summary: Vec<Value> = catalog
            .into_iter()
            .map(|t| {
                json!({
                    "name": t["name"],
                    "description": t["description"],
                    "read_only": t["annotations"]["readOnlyHint"],
                })
            })
            .collect();
        print_json(&Value::Array(summary));
    }
    EXIT_OK
}

fn handle_schema(global: &GlobalOpts, args: &[String]) -> i32 {
    let Some(name) = args.first().map(String::as_str) else {
        emit_error(
            global.output,
            "cli.usage",
            "usage: ice-box-cli schema <TOOL_NAME>",
        );
        return EXIT_USAGE;
    };
    if matches!(name, "-h" | "--help") {
        println!(
            "Usage: ice-box-cli schema <TOOL_NAME>\n\n\
             Print the JSON input schema and annotations for one control/MCP tool."
        );
        return EXIT_OK;
    }
    let Some(tool) = mcp::tools()
        .into_iter()
        .find(|t| t.get("name").and_then(Value::as_str) == Some(name))
    else {
        emit_error(
            global.output,
            "cli.usage",
            &format!("unknown tool '{name}'; run `ice-box-cli tools` to list all tools"),
        );
        return EXIT_USAGE;
    };
    print_json(&tool);
    EXIT_OK
}

fn handle_call(data_dir: &Path, global: &GlobalOpts, args: &[String]) -> i32 {
    let Some(name) = args.first().map(String::as_str) else {
        emit_error(
            global.output,
            "cli.usage",
            "usage: ice-box-cli call <TOOL_NAME> [--input <JSON> | --stdin]",
        );
        return EXIT_USAGE;
    };
    if matches!(name, "-h" | "--help") {
        println!(
            "Usage: ice-box-cli call <TOOL_NAME> [--input <JSON> | --stdin]\n\n\
             Invoke any control/MCP tool directly against the running desktop app or daemon."
        );
        return EXIT_OK;
    }
    let payload = if args.len() == 1 {
        json!({})
    } else {
        let slice = if args.get(1).is_some_and(|a| a == "--input") {
            &args[2..]
        } else {
            &args[1..]
        };
        match parse_json_arg(slice) {
            Ok(v) => v,
            Err(msg) => {
                emit_error(global.output, "cli.usage", &msg);
                return EXIT_USAGE;
            }
        }
    };
    match call_tool(data_dir, name, payload, global.timeout) {
        Ok(out) => {
            print_json(&out.value);
            EXIT_OK
        }
        Err(err) => {
            emit_error(global.output, err.code(), err.message());
            exit_code_for_bridge_error(&err)
        }
    }
}

fn normalize_auto_update_interval(raw: &str) -> Result<&'static str, String> {
    match raw {
        "1h" | "one_hour" => Ok("one_hour"),
        "3h" | "three_hours" => Ok("three_hours"),
        "6h" | "six_hours" => Ok("six_hours"),
        "12h" | "twelve_hours" => Ok("twelve_hours"),
        "24h" | "twenty_four_hours" => Ok("twenty_four_hours"),
        other => Err(format!(
            "invalid interval '{other}' (expected 1h | 3h | 6h | 12h | 24h)"
        )),
    }
}

fn invoke_sub_and_maybe_activate(
    data_dir: &Path,
    global: &GlobalOpts,
    display_cmd: &str,
    tool_name: &str,
    payload: Value,
    activate: bool,
) -> i32 {
    match call_tool(data_dir, tool_name, payload, global.timeout) {
        Ok(out) => {
            if activate {
                let maybe_id = out
                    .value
                    .pointer("/meta/id")
                    .or_else(|| out.value.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if let Some(id) = maybe_id {
                    match call_tool(
                        data_dir,
                        "set_subscription_active",
                        json!({ "id": id, "active": true }),
                        global.timeout,
                    ) {
                        Ok(activated) => {
                            emit_tool_result(global.output, display_cmd, &activated.value);
                            return EXIT_OK;
                        }
                        Err(err) => {
                            emit_error(global.output, err.code(), err.message());
                            return exit_code_for_bridge_error(&err);
                        }
                    }
                }
            }
            emit_tool_result(global.output, display_cmd, &out.value);
            EXIT_OK
        }
        Err(err) => {
            emit_error(global.output, err.code(), err.message());
            exit_code_for_bridge_error(&err)
        }
    }
}

fn parse_json_arg(args: &[String]) -> Result<Value, String> {
    let Some(first) = args.first() else {
        return Err("expected a JSON argument or --stdin".into());
    };
    let text = if first == "--stdin" || first == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("read JSON from stdin: {e}"))?;
        buf
    } else {
        first.clone()
    };
    serde_json::from_str(&text).map_err(|e| format!("invalid JSON input: {e}"))
}

fn print_root_help() {
    println!(
        concat!(
            "ice-box-cli {version} — Cross-platform headless CLI & daemon for ice-box\n\n",
            "Usage:\n",
            "  ice-box-cli [GLOBAL OPTIONS] <COMMAND> [ARGS...]\n\n",
            "Global Options:\n",
            "  --data-dir <PATH>       Data directory (env: ICE_BOX_DATA_DIR)\n",
            "  --resource-dir <PATH>   Bundled sing-box/geoip directory (env: ICE_BOX_RESOURCE_DIR)\n",
            "  --json                  Force machine-readable JSON output\n",
            "  --human                 Force human-readable output\n",
            "  --timeout <SECS>        RPC or wait timeout in seconds\n",
            "  -V, --version           Print version information\n",
            "  -h, --help              Print help\n\n",
            "Runtime & Bridge:\n",
            "  daemon                  Run the headless runtime in the foreground\n",
            "  mcp                     Run the MCP stdio bridge (also --mcp-stdio)\n",
            "  ping                    Check whether the local control service is reachable\n",
            "  wait-ready              Wait until the control service (and core) are ready\n\n",
            "Control & Automation:\n",
            "  status                  Inspect core, capture, subscription, and exit status\n",
            "  start [--wait]          Start proxy capture service (system proxy or TUN)\n",
            "  stop                    Stop proxy capture while keeping core running\n",
            "  core <start|stop>       Start or stop the sing-box core directly\n",
            "  mode <rule|global|direct>  Switch routing mode\n",
            "  env [--shell ...]       Print session-only proxy environment commands\n",
            "  exec -- <CMD...>        Execute a command with session-only proxy env vars\n",
            "  sub <...>               Manage subscriptions (list/add/import/update/use/rm/...)\n",
            "  node <...>              Manage nodes & groups (list/use/group/test)\n",
            "  rule <...>              Manage rules (overview/list/enable/disable/add/update/rm)\n",
            "  ruleset <list|refresh>  Inspect or refresh cached rule-sets\n",
            "  conn <...>              Inspect or close live connections\n",
            "  traffic [--since <N>]   Read live traffic counters\n",
            "  logs [-n <N>] [--all]   Read merged application and core logs\n",
            "  config                  Print the redacted runtime sing-box config\n",
            "  settings <get|set>      Read or patch application settings\n",
            "  tun <...>               Manage TUN recovery and elevation\n\n",
            "Agent Discovery & Generic RPC:\n",
            "  tools [--live]          List all control/MCP tools\n",
            "  schema <TOOL>           Print the JSON input schema for one tool\n",
            "  call <TOOL> [JSON]      Invoke any tool by name over local RPC\n\n",
            "Exit Codes:\n",
            "  {ok} ok  {err} error  {usage} usage  {nr} not running  4 auth  5 permission  {to} timeout"
        ),
        version = VERSION,
        ok = EXIT_OK,
        err = EXIT_ERROR,
        usage = EXIT_USAGE,
        nr = EXIT_NOT_RUNNING,
        to = EXIT_TIMEOUT,
    );
}
