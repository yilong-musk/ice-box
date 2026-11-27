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
    use crate::daemon::resolve_resource_dir_from_candidates;
    use crate::env_exec::{format_env_script, ShellFlavor};
    use crate::exit::{
        exit_code_for_bridge_error, exit_code_for_error_code, EXIT_AUTH, EXIT_ERROR,
        EXIT_NOT_RUNNING, EXIT_PERMISSION, EXIT_TIMEOUT,
    };
    use crate::signal::ShutdownSignal;
    use ice_runtime::mcp::BridgeError;
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
        let nested = root.join("resources");
        fs::create_dir_all(&nested).unwrap();
        let bin = if cfg!(windows) {
            "sing-box.exe"
        } else {
            "sing-box"
        };
        fs::write(nested.join(bin), b"core").unwrap();
        assert_eq!(
            resolve_resource_dir_from_candidates(Some(&root)),
            Some(nested)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shutdown_signal_wakes_waiter() {
        let signal = ShutdownSignal::new();
        assert!(!signal.is_triggered());
        assert!(!signal.wait_timeout(Duration::from_millis(10)));
        let trigger = signal.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            trigger.trigger();
        });
        assert!(signal.wait_timeout(Duration::from_secs(2)));
        assert!(signal.is_triggered());
        handle.join().unwrap();
    }
}
