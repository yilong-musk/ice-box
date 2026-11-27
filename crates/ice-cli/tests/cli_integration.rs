// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end integration tests for the cross-platform headless CLI & daemon.

use ice_cli::daemon::{run_daemon_with_signal, DaemonOptions};
#[cfg(unix)]
use ice_cli::exit::EXIT_PERMISSION;
use ice_cli::exit::{EXIT_NOT_RUNNING, EXIT_OK, EXIT_USAGE};
use ice_cli::signal::ShutdownSignal;
use ice_runtime::mcp::{call_tool, ping, McpPolicy};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ice-cli-it-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn cli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ice-box-cli"))
}

fn wait_for_daemon(data_dir: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if ping(data_dir, Some(Duration::from_millis(250))).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("daemon did not become reachable at {}", data_dir.display());
}

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(cli_bin())
        .args(args)
        .output()
        .expect("spawn ice-box-cli")
}

#[test]
fn offline_discovery_and_not_running_exit_codes() {
    let version = run_cli(&["--json", "--version"]);
    assert_eq!(version.status.code(), Some(EXIT_OK));
    let ver_json: Value = serde_json::from_slice(&version.stdout).expect("version json");
    assert_eq!(ver_json["name"], "ice-box-cli");
    assert_eq!(ver_json["core_version"], ice_core::BUNDLED_SINGBOX_VERSION);

    let tools = run_cli(&["tools"]);
    assert_eq!(tools.status.code(), Some(EXIT_OK));
    let list: Vec<Value> = serde_json::from_slice(&tools.stdout).expect("tools json");
    assert!(list.len() >= 40, "expected >= 40 tools, got {}", list.len());

    let schema = run_cli(&["schema", "set_proxy_mode"]);
    assert_eq!(schema.status.code(), Some(EXIT_OK));
    let schema_json: Value = serde_json::from_slice(&schema.stdout).expect("schema json");
    assert_eq!(schema_json["name"], "set_proxy_mode");

    let missing_schema = run_cli(&["schema", "does_not_exist"]);
    assert_eq!(missing_schema.status.code(), Some(EXIT_USAGE));

    let empty_dir = temp_dir("not-running");
    let dir_str = empty_dir.to_string_lossy().to_string();
    let status = run_cli(&["--data-dir", &dir_str, "--json", "status"]);
    assert_eq!(status.status.code(), Some(EXIT_NOT_RUNNING));
    let err_json: Value = serde_json::from_slice(&status.stderr).expect("error json");
    assert_eq!(err_json["error"]["code"], "cli.not_running");
    let _ = fs::remove_dir_all(empty_dir);
}

#[test]
fn headless_daemon_full_automation_round_trip() {
    let dir = temp_dir("roundtrip");
    let dir_str = dir.to_string_lossy().to_string();
    let signal = ShutdownSignal::new();

    let daemon_dir = dir.clone();
    let daemon_signal = signal.clone();
    let daemon_handle = std::thread::spawn(move || {
        run_daemon_with_signal(
            DaemonOptions {
                data_dir: daemon_dir,
                resource_dir: None,
                auto_start_core: false,
                start_service: false,
                policy: McpPolicy::default(),
                allow_interactive_elevation: false,
                print_ready: false,
            },
            daemon_signal,
        )
    });

    wait_for_daemon(&dir);

    // 1. ping + wait-ready + status
    let ping_out = run_cli(&["--data-dir", &dir_str, "--json", "ping"]);
    assert_eq!(ping_out.status.code(), Some(EXIT_OK));

    let ready_out = run_cli(&["--data-dir", &dir_str, "--json", "wait-ready"]);
    assert_eq!(ready_out.status.code(), Some(EXIT_OK));
    let status: Value = serde_json::from_slice(&ready_out.stdout).expect("status json");
    assert_eq!(status["launch_at_login_supported"], false);
    assert_eq!(status["in_app_update_supported"], false);
    assert_eq!(status["tray_display_supported"], false);

    // 2. settings set + get + env + exec
    let set_port = run_cli(&[
        "--data-dir",
        &dir_str,
        "--json",
        "settings",
        "set",
        r#"{"mixed_port": 18991}"#,
    ]);
    assert_eq!(
        set_port.status.code(),
        Some(EXIT_OK),
        "stderr: {}",
        String::from_utf8_lossy(&set_port.stderr)
    );

    let env_out = run_cli(&["--data-dir", &dir_str, "--json", "env", "--shell", "bash"]);
    assert_eq!(env_out.status.code(), Some(EXIT_OK));
    let env_json: Value = serde_json::from_slice(&env_out.stdout).expect("env json");
    assert_eq!(env_json["port"], 18991);
    assert_eq!(env_json["env"]["HTTP_PROXY"], "http://127.0.0.1:18991");
    assert_eq!(env_json["env"]["ALL_PROXY"], "socks5://127.0.0.1:18991");

    // 3. exec injects proxy env vars and propagates exit codes
    #[cfg(unix)]
    {
        let exec_ok = run_cli(&[
            "--data-dir",
            &dir_str,
            "exec",
            "--",
            "sh",
            "-c",
            "printf '%s|%s' \"$HTTP_PROXY\" \"$ALL_PROXY\"",
        ]);
        assert_eq!(exec_ok.status.code(), Some(EXIT_OK));
        assert_eq!(
            String::from_utf8_lossy(&exec_ok.stdout),
            "http://127.0.0.1:18991|socks5://127.0.0.1:18991"
        );

        let exec_rc = run_cli(&["--data-dir", &dir_str, "exec", "--", "sh", "-c", "exit 42"]);
        assert_eq!(exec_rc.status.code(), Some(42));
    }
    #[cfg(windows)]
    {
        let exec_ok = run_cli(&[
            "--data-dir",
            &dir_str,
            "exec",
            "--",
            "cmd",
            "/C",
            "echo %HTTP_PROXY%^|%ALL_PROXY%",
        ]);
        assert_eq!(exec_ok.status.code(), Some(EXIT_OK));
        let text = String::from_utf8_lossy(&exec_ok.stdout);
        assert!(text.contains("http://127.0.0.1:18991|socks5://127.0.0.1:18991"));

        let exec_rc = run_cli(&["--data-dir", &dir_str, "exec", "--", "cmd", "/C", "exit 42"]);
        assert_eq!(exec_rc.status.code(), Some(42));
    }

    // 4. Import a local subscription, switch mode, list nodes, add/remove custom rule
    let sample_file = dir.join("sample.json");
    fs::write(
        &sample_file,
        r#"{
            "outbounds": [
                {"type": "socks", "tag": "node-alpha", "server": "1.1.1.1", "server_port": 1080},
                {"type": "direct", "tag": "direct"}
            ]
        }"#,
    )
    .unwrap();
    let sample_str = sample_file.to_string_lossy().to_string();
    let imported = run_cli(&[
        "--data-dir",
        &dir_str,
        "--json",
        "sub",
        "import",
        &sample_str,
        "--name",
        "cli-test",
        "--activate",
    ]);
    assert_eq!(
        imported.status.code(),
        Some(EXIT_OK),
        "stderr: {}",
        String::from_utf8_lossy(&imported.stderr)
    );

    let nodes = run_cli(&["--data-dir", &dir_str, "--json", "node", "list"]);
    assert_eq!(nodes.status.code(), Some(EXIT_OK));
    let nodes_json: Value = serde_json::from_slice(&nodes.stdout).expect("nodes json");
    assert!(nodes_json
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["tag"] == "node-alpha"));

    let mode_out = run_cli(&["--data-dir", &dir_str, "--json", "mode", "global"]);
    assert_eq!(mode_out.status.code(), Some(EXIT_OK));

    let rule_add = run_cli(&[
        "--data-dir",
        &dir_str,
        "--json",
        "rule",
        "add",
        r#"{"domain_suffix":["example.org"],"outbound":"direct"}"#,
    ]);
    assert_eq!(
        rule_add.status.code(),
        Some(EXIT_OK),
        "stderr: {}",
        String::from_utf8_lossy(&rule_add.stderr)
    );

    // 5. Generic `call` tool + rule list
    let call_out = run_cli(&["--data-dir", &dir_str, "call", "get_rule_overview"]);
    assert_eq!(call_out.status.code(), Some(EXIT_OK));
    let overview: Value = serde_json::from_slice(&call_out.stdout).expect("overview json");
    assert_eq!(overview["custom"], 1);

    let rules = run_cli(&[
        "--data-dir",
        &dir_str,
        "--json",
        "rule",
        "list",
        "--custom",
        "--keyword",
        "example.org",
    ]);
    assert_eq!(rules.status.code(), Some(EXIT_OK));
    let rules_json: Value = serde_json::from_slice(&rules.stdout).expect("rules json");
    assert_eq!(rules_json["total"], 1);

    // 6. Headless elevation guardrail fails fast with permission exit code on Unix
    #[cfg(unix)]
    {
        let helper_install = run_cli(&["--data-dir", &dir_str, "--json", "tun", "install-helper"]);
        assert_eq!(helper_install.status.code(), Some(EXIT_PERMISSION));
    }

    // 7. Graceful shutdown cleans up runtime files
    signal.trigger();
    assert_eq!(daemon_handle.join().unwrap(), EXIT_OK);
    assert!(!dir.join("mcp-endpoint.json").exists());
    assert!(!dir.join("mcp.secret").exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn read_only_and_no_secrets_policies_guard_agent_calls() {
    let dir = temp_dir("policy");
    let signal = ShutdownSignal::new();

    let daemon_dir = dir.clone();
    let daemon_signal = signal.clone();
    let daemon_handle = std::thread::spawn(move || {
        run_daemon_with_signal(
            DaemonOptions {
                data_dir: daemon_dir,
                resource_dir: None,
                auto_start_core: false,
                start_service: false,
                policy: McpPolicy {
                    read_only: true,
                    allow_secrets: false,
                },
                allow_interactive_elevation: false,
                print_ready: false,
            },
            daemon_signal,
        )
    });

    wait_for_daemon(&dir);

    // Read-only tool succeeds.
    assert!(call_tool(&dir, "get_status", json!({}), None).is_ok());
    assert!(call_tool(&dir, "get_settings", json!({}), None).is_ok());

    // Mutating tool is rejected by --read-only.
    let err = call_tool(&dir, "set_proxy_mode", json!({"mode": "global"}), None)
        .expect_err("read-only must reject mutating tool");
    assert!(err.message().contains("--read-only"), "got: {err}");

    // Secret export is rejected by --no-secrets.
    let export_err = call_tool(
        &dir,
        "export_subscription",
        json!({"id": "00000000-0000-0000-0000-000000000000", "kind": "url"}),
        None,
    )
    .expect_err("no-secrets must reject export_subscription");
    assert!(
        export_err.message().contains("--no-secrets"),
        "got: {export_err}"
    );

    signal.trigger();
    assert_eq!(daemon_handle.join().unwrap(), EXIT_OK);
    let _ = fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn daemon_subprocess_prints_ready_json_and_exits_cleanly_on_sigterm() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;

    let dir = temp_dir("subproc-daemon");
    let dir_str = dir.to_string_lossy().to_string();
    let mut child = Command::new(cli_bin())
        .args([
            "--data-dir",
            &dir_str,
            "daemon",
            "--no-auto-start",
            "--print-ready",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn daemon subprocess");

    let stdout = child.stdout.take().expect("daemon stdout");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read ready line");
    let ready: Value = serde_json::from_str(line.trim()).expect("ready json");
    assert_eq!(ready["ready"], true);
    assert_eq!(ready["pid"], u64::from(child.id()));
    assert!(ready["endpoint"]
        .as_str()
        .is_some_and(|s| s.starts_with("127.0.0.1:")));

    // Verify client can query the live subprocess daemon.
    let status = run_cli(&["--data-dir", &dir_str, "--json", "ping"]);
    assert_eq!(status.status.code(), Some(EXIT_OK));

    // Send SIGTERM and verify clean exit + runtime file removal.
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let exit_status = child.wait().expect("wait daemon");
    assert_eq!(exit_status.code(), Some(EXIT_OK));
    assert!(!dir.join("mcp-endpoint.json").exists());
    assert!(!dir.join("mcp.secret").exists());
    let _ = fs::remove_dir_all(dir);
}
