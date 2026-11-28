// SPDX-License-Identifier: GPL-3.0-or-later

//! Cross-platform headless CLI and daemon library for ice-box.

pub mod commands;
pub mod daemon;
pub mod env_exec;
pub mod exit;
pub mod output;
pub mod signal;

pub use commands::run;

#[cfg(test)]
mod tests {
    use crate::daemon::{
        resolve_resource_dir_from_dirs_with_preference,
        resolve_resource_dir_from_exe_path_with_preference,
    };
    use crate::env_exec::{format_env_script, ShellFlavor};
    use crate::exit::{
        exit_code_for_bridge_error, exit_code_for_error_code, EXIT_AUTH, EXIT_ERROR,
        EXIT_NOT_RUNNING, EXIT_PERMISSION, EXIT_TIMEOUT,
    };
    use crate::output::format_tool_result_human;
    use crate::signal::ShutdownSignal;
    use ice_runtime::application::{ConnectionRow, ConnectionsPage};
    use ice_runtime::mcp::{BridgeError, MCP_POLICY_DENIED};
    use serde_json::json;
    use std::fs;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ice-cli-unit-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn manifest_has_no_tauri_or_gui_deps() {
        let manifest = include_str!("../Cargo.toml");
        for forbidden in ["tauri", "objc2", "webkit", "gtk"] {
            assert!(
                !manifest.contains(forbidden),
                "{forbidden} must not appear in ice-cli"
            );
        }
    }

    #[test]
    fn exit_codes_classify_bridge_and_application_errors() {
        assert_eq!(
            exit_code_for_bridge_error(&BridgeError::NotRunning("down".into())),
            EXIT_NOT_RUNNING
        );
        assert_eq!(
            exit_code_for_bridge_error(&BridgeError::AuthOrConfig("bad token".into())),
            EXIT_AUTH
        );
        assert_eq!(
            exit_code_for_bridge_error(&BridgeError::Transport("read timed out".into())),
            EXIT_TIMEOUT
        );
        assert_eq!(
            exit_code_for_bridge_error(&BridgeError::ToolError {
                code: "tun.permission_required".into(),
                message: "helper missing".into(),
            }),
            EXIT_PERMISSION
        );
        assert_eq!(
            exit_code_for_bridge_error(&BridgeError::ToolError {
                code: MCP_POLICY_DENIED.into(),
                message: "blocked by --read-only".into(),
            }),
            EXIT_PERMISSION
        );
        assert_eq!(
            exit_code_for_error_code("core.health_timeout"),
            EXIT_TIMEOUT
        );
        assert_eq!(exit_code_for_error_code("config.invalid"), EXIT_ERROR);
    }

    #[test]
    fn env_script_formats_every_supported_shell_and_ipv6() {
        let posix = format_env_script(ShellFlavor::Posix, "127.0.0.1", 17890);
        assert!(posix.contains("export HTTP_PROXY='http://127.0.0.1:17890'"));
        assert!(posix.contains("export ALL_PROXY='socks5://127.0.0.1:17890'"));

        let fish = format_env_script(ShellFlavor::Fish, "::1", 17890);
        assert!(fish.contains("set -gx HTTP_PROXY 'http://[::1]:17890'"));

        let pwsh = format_env_script(ShellFlavor::PowerShell, "127.0.0.1", 17890);
        assert!(pwsh.contains("$env:HTTP_PROXY = 'http://127.0.0.1:17890'"));

        let cmd = format_env_script(ShellFlavor::Cmd, "127.0.0.1", 17890);
        assert!(cmd.contains("set HTTP_PROXY=http://127.0.0.1:17890"));

        assert_eq!(ShellFlavor::parse("bash").unwrap(), ShellFlavor::Posix);
        assert_eq!(ShellFlavor::parse("pwsh").unwrap(), ShellFlavor::PowerShell);
        assert!(ShellFlavor::parse("bogus").is_err());
    }

    #[test]
    fn resource_dir_prefers_sibling_resources_dir_over_exe_dir() {
        let root = temp_dir("res");
        let empty = temp_dir("res-empty");
        let (_, empty_prefer) =
            resolve_resource_dir_from_dirs_with_preference(std::slice::from_ref(&empty));
        assert!(!empty_prefer);

        let nested = root.join("resources");
        fs::create_dir_all(&nested).unwrap();
        let bin = if cfg!(windows) {
            "sing-box.exe"
        } else {
            "sing-box"
        };
        fs::write(nested.join(bin), b"core").unwrap();
        assert_eq!(
            resolve_resource_dir_from_dirs_with_preference(std::slice::from_ref(&root)).0,
            Some(nested.clone())
        );
        let (dir, prefer) =
            resolve_resource_dir_from_dirs_with_preference(std::slice::from_ref(&root));
        assert_eq!(dir, Some(nested));
        assert!(prefer);

        let exe_path = root.join(if cfg!(windows) {
            "ice-box-cli.exe"
        } else {
            "ice-box-cli"
        });
        fs::write(&exe_path, b"cli").unwrap();
        let (exe_resolved, exe_prefer) =
            resolve_resource_dir_from_exe_path_with_preference(Some(&exe_path));
        // Unix returns the canonical directory so a symlink install finds
        // bundled resources. Windows returns the invocation directory unchanged.
        let expected_resources = if cfg!(unix) {
            fs::canonicalize(root.join("resources")).unwrap()
        } else {
            root.join("resources")
        };
        assert_eq!(exe_resolved, Some(expected_resources));
        assert!(exe_prefer);

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(empty);
    }

    #[cfg(unix)]
    #[test]
    fn resource_dir_follows_symlinked_executable_to_bundled_resources() {
        use std::os::unix::fs::symlink;

        let root = temp_dir("symlink");
        let real_dir = root.join("install").join("current");
        let resources_dir = real_dir.join("resources");
        let bin_dir = root.join("bin");
        fs::create_dir_all(&resources_dir).unwrap();
        fs::create_dir_all(&bin_dir).unwrap();

        let real_cli = real_dir.join("ice-box-cli");
        fs::write(&real_cli, b"cli").unwrap();
        fs::write(resources_dir.join("sing-box"), b"bundled").unwrap();
        // Put an unrelated sing-box in bin_dir to verify canonical target resources win.
        fs::write(bin_dir.join("sing-box"), b"other").unwrap();

        let mid_link = root.join("mid-link");
        let bin_link = bin_dir.join("ice-box-cli");
        symlink(&real_cli, &mid_link).unwrap();
        symlink(&mid_link, &bin_link).unwrap();

        let (dir, prefer) = resolve_resource_dir_from_exe_path_with_preference(Some(&bin_link));
        let expected = fs::canonicalize(&resources_dir).unwrap();
        assert_eq!(dir, Some(expected));
        assert!(prefer);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shutdown_signal_wakes_waiter_and_done_waiter() {
        let signal = ShutdownSignal::new();
        assert!(!signal.is_triggered());
        assert!(!signal.wait_timeout(Duration::from_millis(10)));
        assert!(!signal.wait_done(Duration::from_millis(10)));
        let trigger = signal.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            trigger.trigger();
            std::thread::sleep(Duration::from_millis(20));
            trigger.mark_done();
        });
        assert!(signal.wait_timeout(Duration::from_secs(2)));
        assert!(signal.is_triggered());
        assert!(signal.wait_done(Duration::from_secs(2)));
        handle.join().unwrap();
    }

    #[test]
    fn human_formatters_match_serialized_connections_and_logs_schemas() {
        let page = ConnectionsPage {
            running: true,
            upload_total: 100,
            download_total: 200,
            total: 1,
            matched: 1,
            connections: vec![ConnectionRow {
                id: "conn-1".into(),
                network: "tcp".into(),
                inbound: "mixed/mixed-in".into(),
                source: "127.0.0.1:50000".into(),
                host: "example.com".into(),
                destination_ip: String::new(),
                destination_port: "443".into(),
                process_path: String::new(),
                upload: 10,
                download: 20,
                start_ms: Some(1_700_000_000_000),
                chains: vec!["node-a".into(), "proxy".into()],
                rule: "final".into(),
            }],
        };
        let rendered_conns =
            format_tool_result_human("conn_list", &serde_json::to_value(page).unwrap())
                .expect("connections human output");
        assert!(
            rendered_conns.contains("example.com:443"),
            "expected string port 443, got:\n{rendered_conns}"
        );
        assert!(rendered_conns.contains("[node-a -> proxy]"));

        let logs = serde_json::to_value(vec![
            "2026-01-01T00:00:00Z INFO core started",
            "2026-01-01T00:00:01Z WARN slow peer",
        ])
        .unwrap();
        let rendered_logs = format_tool_result_human("logs", &logs).expect("logs human output");
        assert_eq!(
            rendered_logs,
            "2026-01-01T00:00:00Z INFO core started\n2026-01-01T00:00:01Z WARN slow peer\n"
        );

        let empty_logs = format_tool_result_human("logs", &json!([])).expect("empty logs");
        assert_eq!(empty_logs, "(no logs)\n");
    }
}
