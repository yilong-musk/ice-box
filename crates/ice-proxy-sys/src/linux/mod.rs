// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux system proxy backend supporting GNOME/GTK (`gsettings`) and KDE Plasma
//! (`kreadconfig`/`kwriteconfig` + DBus). Falls back cleanly to unavailable on
//! headless hosts or unsupported environments without a desktop session.

mod gnome;
mod kde;

use std::path::Path;
use std::process::Command;

use crate::{ProxyBackup, ProxyEndpoints, ProxySysError, SystemProxy};

pub use gnome::{
    format_gvariant_string, format_gvariant_string_list, parse_gvariant_string,
    parse_gvariant_string_list, GnomeProxyBackup,
};
pub use kde::{parse_kde_proxy_endpoint, KdeProxyBackup, KdeTools};

/// Result of running an external helper command on Linux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmdOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Abstracts command execution and environment inspection for offline unit tests.
pub trait LinuxCmdRunner: Send {
    fn run(&self, program: &str, args: &[&str]) -> Result<CmdOutput, std::io::Error>;
    fn env(&self, key: &str) -> Option<String>;
    fn has_program(&self, program: &str) -> bool;
}

/// Real command runner backed by `std::process::Command` and `std::env`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RealLinuxCmdRunner;

impl LinuxCmdRunner for RealLinuxCmdRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CmdOutput, std::io::Error> {
        let output = Command::new(program).args(args).output()?;
        Ok(CmdOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    fn env(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn has_program(&self, program: &str) -> bool {
        let candidate = Path::new(program);
        if candidate.is_absolute() {
            return is_executable_file(candidate);
        }
        if let Some(paths) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&paths) {
                if is_executable_file(&dir.join(program)) {
                    return true;
                }
            }
        }
        for dir in ["/usr/bin", "/bin", "/usr/local/bin"] {
            if is_executable_file(&Path::new(dir).join(program)) {
                return true;
            }
        }
        false
    }
}

fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    meta.is_file() && (meta.permissions().mode() & 0o111 != 0)
}

/// Detected desktop environment proxy mechanism.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinuxProxyBackend {
    Gnome,
    Kde(KdeTools),
    Unsupported,
}

/// Detect the active Linux desktop proxy backend.
///
/// Requires a graphical or D-Bus session (`DISPLAY`, `WAYLAND_DISPLAY`, or
/// `DBUS_SESSION_BUS_ADDRESS`) so headless CI runners and SSH sessions report
/// `Unsupported` instead of probing or mutating absent desktop state.
pub fn detect_backend(runner: &dyn LinuxCmdRunner) -> LinuxProxyBackend {
    let has_session = ["DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS"]
        .iter()
        .any(|key| runner.env(key).is_some_and(|v| !v.trim().is_empty()));
    if !has_session {
        return LinuxProxyBackend::Unsupported;
    }

    if is_kde_session(runner) {
        if let Some(tools) = kde::detect_tools(runner) {
            return LinuxProxyBackend::Kde(tools);
        }
        return LinuxProxyBackend::Unsupported;
    }

    if gnome::is_available(runner) {
        return LinuxProxyBackend::Gnome;
    }

    if let Some(tools) = kde::detect_tools(runner) {
        return LinuxProxyBackend::Kde(tools);
    }

    LinuxProxyBackend::Unsupported
}

fn is_kde_session(runner: &dyn LinuxCmdRunner) -> bool {
    for key in ["XDG_CURRENT_DESKTOP", "DESKTOP_SESSION"] {
        if let Some(raw) = runner.env(key) {
            for token in raw.split(':') {
                let lower = token.trim().to_ascii_lowercase();
                if matches!(
                    lower.as_str(),
                    "kde" | "plasma" | "plasma5" | "plasma6" | "kubuntu"
                ) || lower.contains("plasma")
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Linux system proxy implementation dispatching to GNOME/GTK or KDE Plasma.
#[derive(Debug, Clone)]
pub struct LinuxSystemProxy<R: LinuxCmdRunner = RealLinuxCmdRunner> {
    runner: R,
    backend: LinuxProxyBackend,
}

impl LinuxSystemProxy<RealLinuxCmdRunner> {
    pub fn new() -> Self {
        Self::with_runner(RealLinuxCmdRunner)
    }
}

impl Default for LinuxSystemProxy<RealLinuxCmdRunner> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: LinuxCmdRunner> LinuxSystemProxy<R> {
    pub fn with_runner(runner: R) -> Self {
        let backend = detect_backend(&runner);
        Self { runner, backend }
    }

    pub fn with_backend(runner: R, backend: LinuxProxyBackend) -> Self {
        Self { runner, backend }
    }

    pub fn backend(&self) -> &LinuxProxyBackend {
        &self.backend
    }
}

impl<R: LinuxCmdRunner> SystemProxy for LinuxSystemProxy<R> {
    fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
        match &self.backend {
            LinuxProxyBackend::Gnome => gnome::backup(&self.runner),
            LinuxProxyBackend::Kde(tools) => kde::backup(&self.runner, tools),
            LinuxProxyBackend::Unsupported => Ok(ProxyBackup::default()),
        }
    }

    fn apply(&self, endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
        match &self.backend {
            LinuxProxyBackend::Gnome => gnome::apply(&self.runner, endpoints),
            LinuxProxyBackend::Kde(tools) => kde::apply(&self.runner, tools, endpoints),
            LinuxProxyBackend::Unsupported => Err(ProxySysError::NotImplemented("apply")),
        }
    }

    fn restore(&self, backup: &ProxyBackup) -> Result<(), ProxySysError> {
        match &self.backend {
            LinuxProxyBackend::Gnome => gnome::restore(&self.runner, backup),
            LinuxProxyBackend::Kde(tools) => kde::restore(&self.runner, tools, backup),
            LinuxProxyBackend::Unsupported => Ok(()),
        }
    }

    fn is_available(&self) -> bool {
        !matches!(self.backend, LinuxProxyBackend::Unsupported)
    }
}

pub(crate) fn format_host_port(host: &str, port: u16) -> Option<String> {
    let host = host.trim();
    if host.is_empty() || port == 0 {
        return None;
    }
    if host.contains(':') && !host.starts_with('[') {
        Some(format!("[{host}]:{port}"))
    } else {
        Some(format!("{host}:{port}"))
    }
}

pub(crate) fn run_checked(
    runner: &dyn LinuxCmdRunner,
    program: &str,
    args: &[&str],
) -> Result<String, String> {
    let out = runner
        .run(program, args)
        .map_err(|e| format!("spawn {program}: {e}"))?;
    if !out.success {
        let detail = if !out.stderr.trim().is_empty() {
            out.stderr.trim()
        } else {
            out.stdout.trim()
        };
        return Err(format!("{program} {args:?} failed: {detail}"));
    }
    Ok(out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};

    #[derive(Default, Clone)]
    struct MockLinuxRunner {
        env: Arc<Mutex<HashMap<String, String>>>,
        programs: Arc<Mutex<HashSet<String>>>,
        responses: Arc<Mutex<HashMap<String, Result<CmdOutput, String>>>>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl MockLinuxRunner {
        fn set_env(&self, key: &str, val: &str) {
            self.env
                .lock()
                .unwrap()
                .insert(key.to_string(), val.to_string());
        }

        fn add_program(&self, name: &str) {
            self.programs.lock().unwrap().insert(name.to_string());
        }

        fn set_ok(&self, program: &str, args: &[&str], stdout: &str) {
            let key = format!("{program} {}", args.join(" "));
            self.responses.lock().unwrap().insert(
                key,
                Ok(CmdOutput {
                    success: true,
                    stdout: stdout.to_string(),
                    stderr: String::new(),
                }),
            );
        }

        fn set_fail(&self, program: &str, args: &[&str], stderr: &str) {
            let key = format!("{program} {}", args.join(" "));
            self.responses.lock().unwrap().insert(
                key,
                Ok(CmdOutput {
                    success: false,
                    stdout: String::new(),
                    stderr: stderr.to_string(),
                }),
            );
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl LinuxCmdRunner for MockLinuxRunner {
        fn run(&self, program: &str, args: &[&str]) -> Result<CmdOutput, std::io::Error> {
            let key = format!("{program} {}", args.join(" "));
            self.calls.lock().unwrap().push(key.clone());
            let map = self.responses.lock().unwrap();
            if let Some(res) = map.get(&key) {
                return match res {
                    Ok(out) => Ok(out.clone()),
                    Err(msg) => Err(std::io::Error::other(msg.clone())),
                };
            }
            if program == "gsettings" && args.first() == Some(&"set") {
                return Ok(CmdOutput {
                    success: true,
                    stdout: String::new(),
                    stderr: String::new(),
                });
            }
            if program.starts_with("kwriteconfig") || program == "dbus-send" {
                return Ok(CmdOutput {
                    success: true,
                    stdout: String::new(),
                    stderr: String::new(),
                });
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("unexpected command: {key}"),
            ))
        }

        fn env(&self, key: &str) -> Option<String> {
            self.env.lock().unwrap().get(key).cloned()
        }

        fn has_program(&self, program: &str) -> bool {
            self.programs.lock().unwrap().contains(program)
        }
    }

    fn seed_gnome_defaults(runner: &MockLinuxRunner) {
        runner.set_env("DISPLAY", ":0");
        runner.set_env("XDG_CURRENT_DESKTOP", "ubuntu:GNOME");
        runner.add_program("gsettings");
        runner.set_ok(
            "gsettings",
            &["get", "org.gnome.system.proxy", "mode"],
            "'none'\n",
        );
        runner.set_ok(
            "gsettings",
            &["get", "org.gnome.system.proxy", "autoconfig-url"],
            "''\n",
        );
        runner.set_ok(
            "gsettings",
            &["get", "org.gnome.system.proxy", "ignore-hosts"],
            "['localhost', '127.0.0.0/8', '::1']\n",
        );
        runner.set_ok(
            "gsettings",
            &["get", "org.gnome.system.proxy", "use-same-proxy"],
            "true\n",
        );
        runner.set_ok(
            "gsettings",
            &["get", "org.gnome.system.proxy.http", "host"],
            "''\n",
        );
        runner.set_ok(
            "gsettings",
            &["get", "org.gnome.system.proxy.http", "port"],
            "8080\n",
        );
        runner.set_ok(
            "gsettings",
            &["get", "org.gnome.system.proxy.https", "host"],
            "''\n",
        );
        runner.set_ok(
            "gsettings",
            &["get", "org.gnome.system.proxy.https", "port"],
            "0\n",
        );
        runner.set_ok(
            "gsettings",
            &["get", "org.gnome.system.proxy.socks", "host"],
            "''\n",
        );
        runner.set_ok(
            "gsettings",
            &["get", "org.gnome.system.proxy.socks", "port"],
            "0\n",
        );
    }

    fn seed_kde6(runner: &MockLinuxRunner, proxy_type: &str) {
        runner.set_env("WAYLAND_DISPLAY", "wayland-0");
        runner.set_env("XDG_CURRENT_DESKTOP", "KDE");
        runner.add_program("kreadconfig6");
        runner.add_program("kwriteconfig6");
        runner.add_program("dbus-send");
        runner.set_ok(
            "kreadconfig6",
            &[
                "--file",
                "kioslaverc",
                "--group",
                "Proxy Settings",
                "--key",
                "ProxyType",
            ],
            &format!("{proxy_type}\n"),
        );
        for key in [
            "httpProxy",
            "httpsProxy",
            "socksProxy",
            "NoProxyFor",
            "ReversedException",
            "ProxyConfigScript",
        ] {
            runner.set_ok(
                "kreadconfig6",
                &[
                    "--file",
                    "kioslaverc",
                    "--group",
                    "Proxy Settings",
                    "--key",
                    key,
                ],
                "",
            );
        }
    }

    #[test]
    fn headless_session_is_unsupported_and_does_not_poison_restore() {
        let runner = MockLinuxRunner::default();
        runner.add_program("gsettings");
        let proxy = LinuxSystemProxy::with_runner(runner);
        assert_eq!(proxy.backend(), &LinuxProxyBackend::Unsupported);
        assert!(!proxy.is_available());
        assert_eq!(proxy.backup().unwrap(), ProxyBackup::default());
        proxy.restore(&ProxyBackup::default()).unwrap();
        let err = proxy
            .apply(&ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: None,
                socks_port: None,
            })
            .expect_err("unsupported apply");
        assert!(matches!(err, ProxySysError::NotImplemented("apply")));
    }

    #[test]
    fn kde_takes_precedence_over_gsettings_when_kde_desktop_is_active() {
        let runner = MockLinuxRunner::default();
        seed_kde6(&runner, "0");
        runner.add_program("gsettings");
        runner.set_ok(
            "gsettings",
            &["get", "org.gnome.system.proxy", "mode"],
            "'none'\n",
        );

        let proxy = LinuxSystemProxy::with_runner(runner);
        assert_eq!(
            proxy.backend(),
            &LinuxProxyBackend::Kde(KdeTools {
                read_cmd: "kreadconfig6",
                write_cmd: "kwriteconfig6",
            })
        );
        assert!(proxy.is_available());
    }

    #[test]
    fn gnome_backup_apply_and_restore_preserves_pac_and_prior_proxy() {
        let runner = MockLinuxRunner::default();
        seed_gnome_defaults(&runner);
        runner.set_ok(
            "gsettings",
            &["get", "org.gnome.system.proxy", "mode"],
            "'auto'\n",
        );
        runner.set_ok(
            "gsettings",
            &["get", "org.gnome.system.proxy", "autoconfig-url"],
            "'http://corp.local/proxy.pac'\n",
        );
        runner.set_ok(
            "gsettings",
            &["get", "org.gnome.system.proxy.http", "host"],
            "'10.0.0.2'\n",
        );
        runner.set_ok(
            "gsettings",
            &["get", "org.gnome.system.proxy.http", "port"],
            "3128\n",
        );

        let proxy = LinuxSystemProxy::with_runner(runner.clone());
        assert_eq!(proxy.backend(), &LinuxProxyBackend::Gnome);

        let backup = proxy.backup().unwrap();
        assert!(backup.enabled, "PAC auto mode counts as enabled");
        assert_eq!(backup.http, None, "auto mode does not publish manual http");
        assert_eq!(
            backup.extra["gnome"]["autoconfig_url"],
            "http://corp.local/proxy.pac"
        );

        let endpoints = ProxyEndpoints {
            http_host: "127.0.0.1".into(),
            http_port: 17890,
            socks_host: Some("127.0.0.1".into()),
            socks_port: Some(17890),
        };
        proxy.apply(&endpoints).unwrap();
        let calls = runner.calls();
        assert!(calls
            .contains(&"gsettings set org.gnome.system.proxy.http host '127.0.0.1'".to_string()));
        assert!(calls.contains(&"gsettings set org.gnome.system.proxy.http port 17890".to_string()));
        assert!(calls
            .contains(&"gsettings set org.gnome.system.proxy.socks host '127.0.0.1'".to_string()));
        assert!(calls.contains(&"gsettings set org.gnome.system.proxy mode 'manual'".to_string()));

        proxy.restore(&backup).unwrap();
        let calls = runner.calls();
        assert!(calls.contains(
            &"gsettings set org.gnome.system.proxy autoconfig-url 'http://corp.local/proxy.pac'"
                .to_string()
        ));
        assert!(calls.contains(&"gsettings set org.gnome.system.proxy mode 'auto'".to_string()));
    }

    #[test]
    fn gnome_apply_failure_rolls_back_prior_state() {
        let runner = MockLinuxRunner::default();
        seed_gnome_defaults(&runner);
        runner.set_fail(
            "gsettings",
            &["set", "org.gnome.system.proxy", "mode", "'manual'"],
            "dconf read-only",
        );

        let proxy = LinuxSystemProxy::with_runner(runner.clone());
        let err = proxy
            .apply(&ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: Some("127.0.0.1".into()),
                socks_port: Some(17890),
            })
            .expect_err("apply should fail");
        assert!(matches!(err, ProxySysError::ApplyFailed(_)));
        let calls = runner.calls();
        assert!(
            calls
                .iter()
                .any(|c| c == "gsettings set org.gnome.system.proxy mode 'none'"),
            "rollback must restore prior mode: {calls:?}"
        );
    }

    #[test]
    fn kde_backup_apply_and_restore_roundtrip_notifies_kio() {
        let runner = MockLinuxRunner::default();
        seed_kde6(&runner, "1");
        runner.set_ok(
            "kreadconfig6",
            &[
                "--file",
                "kioslaverc",
                "--group",
                "Proxy Settings",
                "--key",
                "httpProxy",
            ],
            "http://10.0.0.5 8080\n",
        );
        runner.set_ok(
            "kreadconfig6",
            &[
                "--file",
                "kioslaverc",
                "--group",
                "Proxy Settings",
                "--key",
                "httpsProxy",
            ],
            "http://10.0.0.5:8080\n",
        );

        let proxy = LinuxSystemProxy::with_runner(runner.clone());
        let backup = proxy.backup().unwrap();
        assert!(backup.enabled);
        assert_eq!(backup.http.as_deref(), Some("10.0.0.5:8080"));
        assert_eq!(backup.https.as_deref(), Some("10.0.0.5:8080"));

        proxy
            .apply(&ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: Some("127.0.0.1".into()),
                socks_port: Some(17890),
            })
            .unwrap();
        let calls = runner.calls();
        assert!(calls.iter().any(|c| {
            c == "kwriteconfig6 --file kioslaverc --group Proxy Settings --key httpProxy http://127.0.0.1 17890"
        }));
        assert!(calls.iter().any(|c| {
            c == "kwriteconfig6 --file kioslaverc --group Proxy Settings --key socksProxy socks://127.0.0.1 17890"
        }));
        assert!(calls.iter().any(|c| {
            c == "kwriteconfig6 --file kioslaverc --group Proxy Settings --key ProxyType 1"
        }));
        assert!(calls
            .iter()
            .any(|c| c.starts_with("dbus-send --type=signal /KIO/Scheduler")));

        proxy.restore(&backup).unwrap();
        let calls = runner.calls();
        assert!(calls.iter().any(|c| {
            c == "kwriteconfig6 --file kioslaverc --group Proxy Settings --key httpProxy http://10.0.0.5 8080"
        }));
    }

    #[test]
    fn gnome_and_kde_corrupt_default_restore_disables_proxy() {
        let gnome_runner = MockLinuxRunner::default();
        seed_gnome_defaults(&gnome_runner);
        let gnome_proxy = LinuxSystemProxy::with_runner(gnome_runner.clone());
        gnome_proxy.restore(&ProxyBackup::default()).unwrap();
        assert!(gnome_runner
            .calls()
            .contains(&"gsettings set org.gnome.system.proxy mode 'none'".to_string()));

        let kde_runner = MockLinuxRunner::default();
        seed_kde6(&kde_runner, "1");
        let kde_proxy = LinuxSystemProxy::with_runner(kde_runner.clone());
        kde_proxy.restore(&ProxyBackup::default()).unwrap();
        assert!(kde_runner.calls().contains(
            &"kwriteconfig6 --file kioslaverc --group Proxy Settings --key ProxyType 0".to_string()
        ));
    }
}
