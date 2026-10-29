// SPDX-License-Identifier: GPL-3.0-or-later

use crate::application::{
    add_custom_rule_use_case, add_subscription_use_case, broadcast_state_change, collect_nodes,
    collect_status, current_settings, disable_active_backend_inner, ensure_tun_elevation_inner,
    get_log_view_use_case, get_runtime_config_use_case, import_subscription_file_use_case,
    list_subscriptions_use_case, lock_orchestrate, query_rules, remove_custom_rule_use_case,
    remove_subscription_use_case, remove_tun_elevation_inner, replace_recovery_warnings,
    require_running_core, reset_helper_probe_cache, rule_overview, save_settings_inner,
    set_active_subscription_use_case, set_auto_update_subscription_use_case,
    set_group_selection_use_case, set_proxy_mode_inner, set_rule_disabled_use_case,
    set_selected_node_use_case, start_core_inner, start_service, stop_core_use_case,
    subscription_share_use_case, test_node_delay_use_case, update_custom_rule_use_case,
    update_subscription_use_case, AddCustomRuleRequest, AddSubscriptionRequest,
    GroupSelectionRequest, IdRequest, ImportSubscriptionFileRequest, ListRulesRequest,
    LogViewRequest, RemoveCustomRuleRequest, SetActiveRequest, SetAutoUpdateRequest,
    SetProxyModeRequest, SetRuleDisabledRequest, SettingsPatch, TagRequest,
    UpdateCustomRuleRequest,
};
use crate::AppState;
use ice_config::{AppError, ErrorCode};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

const PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_LOG_LINES: usize = 10_000;

pub fn tools() -> Vec<Value> {
    vec![
        read_tool(
            "get_status",
            "Read the current core, proxy, TUN, subscription, and runtime status.",
            no_args(),
        ),
        read_tool(
            "get_settings",
            "Read the shared application settings.",
            no_args(),
        ),
        write_tool(
            "update_settings",
            "Persist and apply a partial settings patch. Use the proxy service tools to change whether capture is enabled.",
            json!({
                "type": "object",
                "properties": {
                    "patch": settings_patch_schema()
                },
                "required": ["patch"],
                "additionalProperties": false
            }),
        ),
        write_tool(
            "start_proxy_service",
            "Start the effective system-proxy or TUN capture backend.",
            no_args(),
        ),
        write_tool(
            "stop_proxy_service",
            "Stop system-proxy or TUN capture while leaving the core available.",
            no_args(),
        ),
        write_tool(
            "start_core",
            "Start the sing-box core without taking over OS traffic.",
            no_args(),
        ),
        write_tool(
            "stop_core",
            "Stop OS capture and the sing-box core.",
            no_args(),
        ),
        write_tool(
            "set_proxy_mode",
            "Switch routing between rule, global, and direct modes.",
            json!({
                "type": "object",
                "properties": {
                    "mode": {"type": "string", "enum": ["rule", "global", "direct"]}
                },
                "required": ["mode"],
                "additionalProperties": false
            }),
        ),
        read_tool(
            "get_traffic_snapshot",
            "Read the current traffic sample while the core is running.",
            no_args(),
        ),
        read_tool(
            "get_traffic_since",
            "Read traffic samples after an optional cursor.",
            json!({
                "type": "object",
                "properties": {"cursor": {"type": "integer", "minimum": 0}},
                "additionalProperties": false
            }),
        ),
        read_tool(
            "list_subscriptions",
            "List subscriptions with sensitive URLs redacted.",
            no_args(),
        ),
        open_world_write_tool(
            "add_subscription",
            "Fetch and add a remote subscription URL.",
            json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string"},
                    "name": {"type": ["string", "null"]},
                    "auto_update": {"type": "boolean"},
                    "auto_update_interval": {
                        "type": ["string", "null"],
                        "enum": [
                            "one_hour",
                            "three_hours",
                            "six_hours",
                            "twelve_hours",
                            "twenty_four_hours",
                            null
                        ]
                    }
                },
                "required": ["url"],
                "additionalProperties": false
            }),
        ),
        write_tool(
            "import_subscription_document",
            "Import a subscription from a sing-box JSON document or URI list.",
            json!({
                "type": "object",
                "properties": {
                    "content": {"type": "string"},
                    "name": {"type": ["string", "null"]}
                },
                "required": ["content"],
                "additionalProperties": false
            }),
        ),
        write_tool(
            "remove_subscription",
            "Remove a subscription by ID.",
            id_schema(),
        ),
        open_world_write_tool(
            "update_subscription",
            "Fetch and apply an update for one subscription.",
            id_schema(),
        ),
        write_tool(
            "set_subscription_active",
            "Activate or deactivate a subscription.",
            json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "format": "uuid"},
                    "active": {"type": "boolean"}
                },
                "required": ["id", "active"],
                "additionalProperties": false
            }),
        ),
        write_tool(
            "set_subscription_auto_update",
            "Configure automatic updates for one subscription.",
            json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "format": "uuid"},
                    "auto_update": {"type": "boolean"},
                    "auto_update_interval": {
                        "type": ["string", "null"],
                        "enum": [
                            "one_hour",
                            "three_hours",
                            "six_hours",
                            "twelve_hours",
                            "twenty_four_hours",
                            null
                        ]
                    }
                },
                "required": ["id", "auto_update"],
                "additionalProperties": false
            }),
        ),
        read_tool(
            "export_subscription",
            "Return the full subscription URL or portable sing-box JSON. The result can contain credentials.",
            json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "format": "uuid"},
                    "kind": {"type": "string", "enum": ["url", "singbox"]}
                },
                "required": ["id", "kind"],
                "additionalProperties": false
            }),
        ),
        read_tool(
            "list_nodes",
            "List outbounds and strategy groups from the active subscription.",
            no_args(),
        ),
        write_tool(
            "select_node",
            "Select a node for the active profile.",
            json!({
                "type": "object",
                "properties": {"tag": {"type": "string"}},
                "required": ["tag"],
                "additionalProperties": false
            }),
        ),
        write_tool(
            "select_group_member",
            "Select a member of a strategy group.",
            json!({
                "type": "object",
                "properties": {
                    "group": {"type": "string"},
                    "member": {"type": "string"}
                },
                "required": ["group", "member"],
                "additionalProperties": false
            }),
        ),
        open_world_read_tool(
            "test_node_delay",
            "Measure delay for a node through the running core.",
            json!({
                "type": "object",
                "properties": {"tag": {"type": "string"}},
                "required": ["tag"],
                "additionalProperties": false
            }),
        ),
        read_tool(
            "get_rule_overview",
            "Read aggregate rule counts and types.",
            no_args(),
        ),
        read_tool(
            "list_rules",
            "List rules with the same filters and pagination as the Rules page.",
            json!({
                "type": "object",
                "properties": {
                    "keyword": {"type": ["string", "null"]},
                    "type": {"type": ["string", "null"]},
                    "disabled": {"type": ["string", "null"], "enum": ["all", "disabled", "enabled", null]},
                    "custom": {"type": ["boolean", "null"]},
                    "offset": {"type": "integer", "minimum": 0},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 200}
                },
                "additionalProperties": false
            }),
        ),
        write_tool(
            "set_rule_disabled",
            "Enable or disable a subscription rule by fingerprint.",
            json!({
                "type": "object",
                "properties": {
                    "fingerprint": {"type": "string"},
                    "disabled": {"type": "boolean"}
                },
                "required": ["fingerprint", "disabled"],
                "additionalProperties": false
            }),
        ),
        write_tool(
            "add_custom_rule",
            "Add a custom sing-box route rule.",
            json!({
                "type": "object",
                "properties": {"rule": {"type": "object"}},
                "required": ["rule"],
                "additionalProperties": false
            }),
        ),
        write_tool(
            "update_custom_rule",
            "Replace a custom sing-box route rule in place.",
            json!({
                "type": "object",
                "properties": {
                    "fingerprint": {"type": "string"},
                    "rule": {"type": "object"}
                },
                "required": ["fingerprint", "rule"],
                "additionalProperties": false
            }),
        ),
        write_tool(
            "remove_custom_rule",
            "Remove a custom rule by fingerprint.",
            json!({
                "type": "object",
                "properties": {"fingerprint": {"type": "string"}},
                "required": ["fingerprint"],
                "additionalProperties": false
            }),
        ),
        read_tool(
            "get_logs",
            "Read a bounded tail of the application and core logs.",
            json!({
                "type": "object",
                "properties": {"n": {"type": "integer", "minimum": 1, "maximum": MAX_LOG_LINES}},
                "additionalProperties": false
            }),
        ),
        read_tool(
            "get_runtime_config",
            "Read the generated sing-box configuration with known secrets redacted.",
            no_args(),
        ),
        write_tool(
            "recover_tun",
            "Retry guarded TUN cleanup without enabling capture.",
            no_args(),
        ),
        write_tool(
            "install_tun_helper",
            "Install the macOS privileged TUN helper. The operating system asks the user to authorize this action.",
            no_args(),
        ),
        write_tool(
            "uninstall_tun_helper",
            "Uninstall the macOS privileged TUN helper. The operating system asks the user to authorize this action.",
            no_args(),
        ),
        write_tool(
            "ensure_tun_elevation",
            "Install the one-time Windows scheduled-task elevation used by TUN.",
            no_args(),
        ),
        write_tool(
            "remove_tun_elevation",
            "Remove the Windows scheduled-task elevation used by TUN.",
            no_args(),
        ),
    ]
}

fn no_args() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

fn settings_patch_schema() -> Value {
    json!({
        "type": "object",
        "description": "Partial AppSettings object. Omitted fields are unchanged. proxy_service_enabled is managed by start_proxy_service and stop_proxy_service.",
        "properties": {
            "mixed_listen": {"type": "string"},
            "mixed_port": {"type": "integer", "minimum": 1024, "maximum": 65535},
            "clash_api_listen": {"type": "string"},
            "clash_api_port": {"type": "integer", "minimum": 1024, "maximum": 65535},
            "selected_tag": {"type": ["string", "null"]},
            "auto_set_system_proxy": {"type": "boolean"},
            "allow_lan": {"type": "boolean"},
            "ipv6": {
                "type": "boolean",
                "description": "Query AAAA records; false resolves IPv4 only. Windows TUN stays IPv4-only."
            },
            "prefer_ipv6": {
                "type": "boolean",
                "description": "Prefer AAAA over A when both exist; implies ipv6=true."
            },
            "proxy_mode": {"type": "string", "enum": ["rule", "global", "direct"]},
            "tun": {
                "type": "object",
                "properties": {
                    "enabled": {"type": "boolean"},
                    "interface_name": {"type": ["string", "null"]},
                    "ipv4_address": {"type": "string"},
                    "ipv6_address": {"type": "string"},
                    "mtu": {"type": "integer", "minimum": 1280, "maximum": 9000},
                    "auto_route": {"type": "boolean"},
                    "strict_route": {"type": "boolean"},
                    "stack": {"type": "string", "enum": ["gvisor", "system", "mixed"]},
                    "dns_hijack": {"type": "boolean"}
                },
                "additionalProperties": false
            },
            "auto_default_rules": {"type": "boolean"},
            "language": {"type": "string", "enum": ["system", "zh", "en"]},
            "check_app_updates": {"type": "boolean"},
            "probe_exit_ip": {"type": "boolean"},
            "log_debug": {"type": "boolean"},
            "tray_display_mode": {"type": "string", "enum": ["icon_and_speed", "icon", "speed"]},
            "launch_at_login": {"type": "boolean"}
        },
        "additionalProperties": false
    })
}

fn id_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"id": {"type": "string", "format": "uuid"}},
        "required": ["id"],
        "additionalProperties": false
    })
}

fn read_tool(name: &str, description: &str, input_schema: Value) -> Value {
    tool(name, description, input_schema, true, false)
}

fn write_tool(name: &str, description: &str, input_schema: Value) -> Value {
    tool(name, description, input_schema, false, false)
}

fn open_world_read_tool(name: &str, description: &str, input_schema: Value) -> Value {
    tool(name, description, input_schema, true, true)
}

fn open_world_write_tool(name: &str, description: &str, input_schema: Value) -> Value {
    tool(name, description, input_schema, false, true)
}

fn tool(
    name: &str,
    description: &str,
    input_schema: Value,
    read_only: bool,
    open_world: bool,
) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": !read_only,
            "idempotentHint": false,
            "openWorldHint": open_world
        }
    })
}

pub(super) fn respond(app: &AppHandle, text: &str) -> Option<Value> {
    respond_with(Some(app), text)
}

fn respond_with(app: Option<&AppHandle>, text: &str) -> Option<Value> {
    if text.trim().is_empty() {
        return None;
    }
    let request: Value = match serde_json::from_str(text) {
        Ok(request) => request,
        Err(error) => {
            return Some(protocol_error_value(
                None,
                -32700,
                format!("parse error: {error}"),
            ));
        }
    };
    let id = request.get("id").cloned();
    id.as_ref()?;
    if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(protocol_error_value(id, -32600, "invalid JSON-RPC version"));
    }
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return Some(protocol_error_value(id, -32600, "missing JSON-RPC method"));
    };
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    let result = match method {
        "initialize" => {
            let requested_version = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(PROTOCOL_VERSION);
            let protocol_version = if requested_version == PROTOCOL_VERSION {
                requested_version
            } else {
                PROTOCOL_VERSION
            };
            Ok(json!({
                "protocolVersion": protocol_version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {
                    "name": "ice-box",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "instructions": "Operate the running ice-box desktop runtime. GUI and MCP actions share one AppState, one data directory, and one proxy session."
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools()})),
        "tools/call" => match app {
            Some(app) => call_tool(app, params),
            None => Err((-32603, "application state is unavailable".to_string())),
        },
        "resources/list" => Ok(json!({"resources": []})),
        "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
        "prompts/list" => Ok(json!({"prompts": []})),
        "logging/setLevel" => Ok(json!({})),
        _ => {
            return Some(protocol_error_value(
                id,
                -32601,
                format!("method not found: {method}"),
            ));
        }
    };
    Some(match result {
        Ok(result) => json!({
            "jsonrpc": "2.0",
            "id": id.unwrap_or(Value::Null),
            "result": result
        }),
        Err((code, message)) => protocol_error_value(id, code, message),
    })
}

fn call_tool(app: &AppHandle, params: Value) -> Result<Value, (i32, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| (-32602, "tools/call requires a tool name".to_string()))?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    match dispatch_tool(app, name, args) {
        Ok(value) => {
            let text = serde_json::to_string_pretty(&value)
                .unwrap_or_else(|_| "{\"ok\":true}".to_string());
            let structured_content = if value.is_object() {
                value
            } else {
                json!({"result": value})
            };
            Ok(json!({
                "content": [{"type": "text", "text": text}],
                "structuredContent": structured_content,
                "isError": false
            }))
        }
        Err(error) => Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("{}: {}", error.code, error.message)
            }],
            "structuredContent": {
                "error": {
                    "code": error.code,
                    "message": error.message
                }
            },
            "isError": true
        })),
    }
}

pub(super) fn protocol_error_value(
    id: Option<Value>,
    code: i32,
    message: impl Into<String>,
) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": {"code": code, "message": message.into()}
    })
}

fn dispatch_tool(app: &AppHandle, name: &str, args: Value) -> Result<Value, AppError> {
    ensure_object(&args)?;
    validate_tool_arguments(name, &args)?;
    let state = app.state::<AppState>();
    match name {
        "get_status" => {
            crate::application::refresh_runtime_probes(state.inner());
            to_value(collect_status(state.inner())?)
        }
        "get_settings" => to_value(current_settings(&state.paths)?),
        "update_settings" => {
            let patch = parse_update_settings_args(&args)?;
            save_settings_inner(app, state.inner(), patch)?;
            broadcast_state_change(app);
            Ok(json!({"ok": true}))
        }
        "start_proxy_service" => {
            start_service(app, state.inner())?;
            Ok(json!({"ok": true}))
        }
        "stop_proxy_service" => {
            disable_active_backend_inner(app, state.inner())?;
            Ok(json!({"ok": true}))
        }
        "start_core" => {
            let orchestrate = lock_orchestrate(state.inner())?;
            start_core_inner(app, state.inner())?;
            drop(orchestrate);
            broadcast_state_change(app);
            Ok(json!({"ok": true}))
        }
        "stop_core" => {
            stop_core_use_case(app, state.inner())?;
            Ok(json!({"ok": true}))
        }
        "set_proxy_mode" => {
            let mode = args
                .get("mode")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid_args("mode must be rule, global, or direct"))?;
            set_proxy_mode_inner(
                app,
                state.inner(),
                SetProxyModeRequest {
                    mode: mode.to_string(),
                },
            )?;
            Ok(json!({"ok": true}))
        }
        "get_traffic_snapshot" => {
            require_running_core(state.inner())?;
            to_value(state.traffic.snapshot())
        }
        "get_traffic_since" => {
            require_running_core(state.inner())?;
            let cursor = args.get("cursor").and_then(Value::as_u64);
            to_value(state.traffic.snapshot_since(cursor))
        }
        "list_subscriptions" => list_subscriptions_use_case(state.inner()),
        "add_subscription" => {
            let request: AddSubscriptionRequest = parse_args(args)?;
            add_subscription_use_case(app, state.inner(), request)
        }
        "import_subscription_document" => {
            let request: ImportSubscriptionFileRequest = parse_args(args)?;
            import_subscription_file_use_case(app, state.inner(), request)
        }
        "remove_subscription" => {
            let request: IdRequest = parse_args(args)?;
            remove_subscription_use_case(app, state.inner(), request)
        }
        "update_subscription" => {
            let request: IdRequest = parse_args(args)?;
            update_subscription_use_case(app, state.inner(), request)
        }
        "set_subscription_active" => {
            let request: SetActiveRequest = parse_args(args)?;
            set_active_subscription_use_case(app, state.inner(), request)
        }
        "set_subscription_auto_update" => {
            let request: SetAutoUpdateRequest = parse_args(args)?;
            set_auto_update_subscription_use_case(app, state.inner(), request)
        }
        "export_subscription" => {
            let request = parse_args(args)?;
            let text = subscription_share_use_case(state.inner(), request)?;
            Ok(json!({"content": text}))
        }
        "list_nodes" => to_value(collect_nodes(state.inner())?),
        "select_node" => {
            let request: TagRequest = parse_args(args)?;
            set_selected_node_use_case(app, state.inner(), request)?;
            Ok(json!({"ok": true}))
        }
        "select_group_member" => {
            let request: GroupSelectionRequest = parse_args(args)?;
            set_group_selection_use_case(app, state.inner(), request)?;
            Ok(json!({"ok": true}))
        }
        "test_node_delay" => {
            let request: TagRequest = parse_args(args)?;
            to_value(test_node_delay_use_case(state.inner(), request)?)
        }
        "get_rule_overview" => to_value(rule_overview(state.inner())?),
        "list_rules" => {
            let request: ListRulesRequest = parse_args(args)?;
            to_value(query_rules(state.inner(), &request)?)
        }
        "set_rule_disabled" => {
            let request: SetRuleDisabledRequest = parse_args(args)?;
            set_rule_disabled_use_case(app, state.inner(), request)
        }
        "add_custom_rule" => {
            let request: AddCustomRuleRequest = parse_args(args)?;
            add_custom_rule_use_case(app, state.inner(), request)
        }
        "update_custom_rule" => {
            let request: UpdateCustomRuleRequest = parse_args(args)?;
            update_custom_rule_use_case(app, state.inner(), request)
        }
        "remove_custom_rule" => {
            let request: RemoveCustomRuleRequest = parse_args(args)?;
            remove_custom_rule_use_case(app, state.inner(), request)
        }
        "get_logs" => {
            let n = args
                .get("n")
                .and_then(Value::as_u64)
                .unwrap_or(200)
                .clamp(1, MAX_LOG_LINES as u64) as usize;
            to_value(get_log_view_use_case(state.inner(), LogViewRequest { n })?)
        }
        "get_runtime_config" => Ok(json!({
            "config": get_runtime_config_use_case(state.inner())?
        })),
        "recover_tun" => {
            let _orchestrate = lock_orchestrate(state.inner())?;
            state.capture.refresh_backend()?;
            let mut core = state
                .core
                .lock()
                .map_err(|_| AppError::new(ErrorCode::LockPoisoned, "core lock poisoned"))?;
            let warnings = state.capture.recover(&mut **core)?;
            replace_recovery_warnings(state.inner(), warnings.clone());
            drop(core);
            drop(_orchestrate);
            broadcast_state_change(app);
            to_value(warnings)
        }
        "install_tun_helper" => {
            crate::helper_install::install_helper_inner(app)?;
            reset_helper_probe_cache(state.inner());
            broadcast_state_change(app);
            Ok(json!({"ok": true}))
        }
        "uninstall_tun_helper" => {
            crate::helper_install::uninstall_helper_inner(app)?;
            reset_helper_probe_cache(state.inner());
            broadcast_state_change(app);
            Ok(json!({"ok": true}))
        }
        "ensure_tun_elevation" => {
            ensure_tun_elevation_inner(app, state.inner())?;
            broadcast_state_change(app);
            Ok(json!({"ok": true}))
        }
        "remove_tun_elevation" => {
            remove_tun_elevation_inner(app, state.inner())?;
            broadcast_state_change(app);
            Ok(json!({"ok": true}))
        }
        _ => Err(AppError::new(
            ErrorCode::ConfigInvalid,
            format!("unknown MCP tool: {name}"),
        )),
    }
}

fn ensure_object(value: &Value) -> Result<(), AppError> {
    if value.is_object() {
        Ok(())
    } else {
        Err(invalid_args("tool arguments must be a JSON object"))
    }
}

fn validate_tool_arguments(name: &str, args: &Value) -> Result<(), AppError> {
    if name == "update_settings" {
        if let Some(patch) = args.get("patch") {
            reject_proxy_service_flag(patch)?;
        }
    }
    let Some(tool) = tools()
        .into_iter()
        .find(|tool| tool.get("name").and_then(Value::as_str) == Some(name))
    else {
        return Ok(());
    };
    let schema = tool
        .get("inputSchema")
        .ok_or_else(|| invalid_args(format!("{name} has no input schema")))?;
    validate_schema_value(args, schema, "$")
        .map_err(|message| invalid_args(format!("invalid tool arguments: {message}")))
}

fn validate_schema_value(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
    if let Some(expected) = schema.get("type") {
        let matches = match expected {
            Value::String(kind) => schema_type_matches(value, kind),
            Value::Array(kinds) => kinds
                .iter()
                .filter_map(Value::as_str)
                .any(|kind| schema_type_matches(value, kind)),
            _ => false,
        };
        if !matches {
            return Err(format!("{path} must be {}", schema_type_name(expected)));
        }
    }

    if let Some(options) = schema.get("enum").and_then(Value::as_array) {
        if !options.iter().any(|option| option == value) {
            return Err(format!("{path} must be one of {options:?}"));
        }
    }

    if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64) {
        if value.as_f64().is_none_or(|number| number < minimum) {
            return Err(format!("{path} must be at least {minimum}"));
        }
    }
    if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64) {
        if value.as_f64().is_none_or(|number| number > maximum) {
            return Err(format!("{path} must be at most {maximum}"));
        }
    }

    let Some(object) = value.as_object() else {
        return Ok(());
    };
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for key in required.iter().filter_map(Value::as_str) {
            if !object.contains_key(key) {
                return Err(format!("{path}.{key} is required"));
            }
        }
    }

    let properties = schema.get("properties").and_then(Value::as_object);
    if let Some(properties) = properties {
        for (key, child) in object {
            if let Some(child_schema) = properties.get(key) {
                validate_schema_value(child, child_schema, &format!("{path}.{key}"))?;
                continue;
            }
            match schema.get("additionalProperties") {
                Some(Value::Bool(false)) => {
                    return Err(format!("{path}.{key} is not allowed"));
                }
                Some(additional @ Value::Object(_)) => {
                    validate_schema_value(child, additional, &format!("{path}.{key}"))?;
                }
                _ => {}
            }
        }
    } else if matches!(schema.get("additionalProperties"), Some(Value::Bool(false)))
        && !object.is_empty()
    {
        return Err(format!("{path} does not allow properties"));
    }
    Ok(())
}

fn schema_type_matches(value: &Value, kind: &str) -> bool {
    match kind {
        "array" => value.is_array(),
        "boolean" => value.is_boolean(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "null" => value.is_null(),
        "number" => value.is_number(),
        "object" => value.is_object(),
        "string" => value.is_string(),
        _ => false,
    }
}

fn schema_type_name(expected: &Value) -> String {
    match expected {
        Value::String(kind) => kind.clone(),
        Value::Array(kinds) => kinds
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" or "),
        _ => "valid".to_string(),
    }
}

fn parse_args<T: DeserializeOwned>(value: Value) -> Result<T, AppError> {
    serde_json::from_value(value)
        .map_err(|error| invalid_args(format!("invalid tool arguments: {error}")))
}

fn parse_update_settings_args(args: &Value) -> Result<SettingsPatch, AppError> {
    let patch = args
        .get("patch")
        .ok_or_else(|| invalid_args("update_settings requires a patch object"))?;
    ensure_object(patch)?;
    reject_proxy_service_flag(patch)?;
    parse_args(patch.clone())
}

fn reject_proxy_service_flag(patch: &Value) -> Result<(), AppError> {
    if patch.get("proxy_service_enabled").is_some() {
        return Err(invalid_args(
            "proxy_service_enabled is managed by start_proxy_service and stop_proxy_service",
        ));
    }
    Ok(())
}

fn invalid_args(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::ConfigInvalid, message)
}

fn to_value<T: serde::Serialize>(value: T) -> Result<Value, AppError> {
    serde_json::to_value(value).map_err(|error| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("serialize MCP result: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_are_unique_and_have_object_schemas() {
        let tools = tools();
        let mut names = std::collections::HashSet::new();
        for tool in &tools {
            let name = tool.get("name").and_then(Value::as_str).expect("name");
            assert!(names.insert(name.to_string()), "duplicate tool {name}");
            assert_eq!(
                tool.pointer("/inputSchema/type").and_then(Value::as_str),
                Some("object"),
                "{name} input schema"
            );
            let read_only = tool
                .pointer("/annotations/readOnlyHint")
                .and_then(Value::as_bool)
                .expect("readOnlyHint");
            let destructive = tool
                .pointer("/annotations/destructiveHint")
                .and_then(Value::as_bool)
                .expect("destructiveHint");
            assert_eq!(destructive, !read_only, "{name} annotation");
            let open_world = tool
                .pointer("/annotations/openWorldHint")
                .and_then(Value::as_bool)
                .expect("openWorldHint");
            assert_eq!(
                open_world,
                matches!(
                    name,
                    "add_subscription" | "update_subscription" | "test_node_delay"
                ),
                "{name} openWorldHint"
            );
        }
        assert!(tools.len() >= 25);
    }

    #[test]
    fn initialize_and_tool_list_are_protocol_shaped() {
        let initialized = respond_with(
            None,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {"protocolVersion": PROTOCOL_VERSION}
            })
            .to_string(),
        )
        .expect("initialize response");
        assert_eq!(initialized["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(
            initialized["result"]["capabilities"]["tools"]["listChanged"],
            false
        );

        let listed = respond_with(
            None,
            r#"{"jsonrpc":"2.0","id":"tools","method":"tools/list"}"#,
        )
        .expect("tool list response");
        assert_eq!(
            listed["result"]["tools"].as_array().map(Vec::len),
            Some(tools().len())
        );
    }

    #[test]
    fn initialize_negotiates_a_supported_protocol_version() {
        let initialized = respond_with(
            None,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {"protocolVersion": "2099-01-01"}
            })
            .to_string(),
        )
        .expect("initialize response");
        assert_eq!(initialized["result"]["protocolVersion"], PROTOCOL_VERSION);
    }

    #[test]
    fn update_settings_rejects_the_proxy_service_desire_flag() {
        let error = parse_update_settings_args(&json!({
            "patch": {"proxy_service_enabled": false}
        }))
        .expect_err("proxy service flag must be rejected");
        assert_eq!(error.code, ErrorCode::ConfigInvalid.as_str());
        assert!(error.message.contains("start_proxy_service"));

        let patch = parse_update_settings_args(&json!({
            "patch": {"mixed_port": 18080}
        }))
        .expect("ordinary setting");
        assert_eq!(patch.mixed_port, Some(18080));
    }

    #[test]
    fn tool_arguments_are_validated_against_the_advertised_schema() {
        assert!(validate_tool_arguments("start_proxy_service", &json!({"dry_run": true})).is_err());
        assert!(validate_tool_arguments("set_proxy_mode", &json!({"mode": "bogus"})).is_err());
        assert!(validate_tool_arguments("get_logs", &json!({"n": 0})).is_err());
        assert!(validate_tool_arguments(
            "update_settings",
            &json!({"patch": {"mixedPort": 18080}})
        )
        .is_err());
        assert!(validate_tool_arguments(
            "update_settings",
            &json!({"patch": {"mixed_port": 18080}})
        )
        .is_ok());
    }

    #[test]
    fn update_custom_rule_is_a_validated_write_tool() {
        let tool = tools()
            .into_iter()
            .find(|tool| tool["name"] == "update_custom_rule")
            .expect("update_custom_rule tool");
        assert_eq!(tool["annotations"]["readOnlyHint"], false);
        assert_eq!(tool["annotations"]["destructiveHint"], true);
        assert_eq!(
            tool["inputSchema"]["required"],
            json!(["fingerprint", "rule"])
        );

        let rule = json!({ "domain": ["a.com"], "outbound": "direct" });
        assert!(validate_tool_arguments(
            "update_custom_rule",
            &json!({ "fingerprint": "sha256:abc", "rule": rule })
        )
        .is_ok());
        assert!(validate_tool_arguments("update_custom_rule", &json!({ "rule": rule })).is_err());
        assert!(validate_tool_arguments(
            "update_custom_rule",
            &json!({ "fingerprint": "sha256:abc" })
        )
        .is_err());
        assert!(validate_tool_arguments(
            "update_custom_rule",
            &json!({ "fingerprint": "sha256:abc", "rule": "not-an-object" })
        )
        .is_err());
        assert!(validate_tool_arguments(
            "update_custom_rule",
            &json!({ "fingerprint": "sha256:abc", "rule": rule, "extra": true })
        )
        .is_err());
    }

    #[test]
    fn update_settings_schema_lists_managed_patch_fields() {
        let tool = tools()
            .into_iter()
            .find(|tool| tool["name"] == "update_settings")
            .expect("update_settings tool");
        let patch = &tool["inputSchema"]["properties"]["patch"];
        assert_eq!(patch["additionalProperties"], false);
        assert!(patch["properties"]["mixed_port"].is_object());
        assert!(patch["properties"]["ipv6"].is_object());
        assert!(patch["properties"]["prefer_ipv6"].is_object());
        assert!(patch["properties"]["tun"]["properties"]["enabled"].is_object());
        assert!(patch["properties"].get("proxy_service_enabled").is_none());
    }

    #[test]
    fn protocol_errors_keep_the_request_id() {
        let response = protocol_error_value(Some(Value::from("request-1")), -32601, "not found");
        assert_eq!(response["id"], "request-1");
        assert_eq!(response["error"]["code"], -32601);
    }

    #[test]
    fn notification_is_ignored_and_invalid_requests_return_json_rpc_errors() {
        assert!(respond_with(
            None,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        )
        .is_none());

        let malformed = respond_with(None, "{").expect("parse error");
        assert_eq!(malformed["id"], Value::Null);
        assert_eq!(malformed["error"]["code"], -32700);

        let wrong_version = respond_with(None, r#"{"jsonrpc":"1.0","id":7,"method":"ping"}"#)
            .expect("version error");
        assert_eq!(wrong_version["id"], 7);
        assert_eq!(wrong_version["error"]["code"], -32600);

        let unknown = respond_with(
            None,
            r#"{"jsonrpc":"2.0","id":"missing","method":"missing"}"#,
        )
        .expect("method error");
        assert_eq!(unknown["id"], "missing");
        assert_eq!(unknown["error"]["code"], -32601);
    }
}
