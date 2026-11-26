// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux system proxy backend supporting GNOME/GTK (`gsettings`) and KDE Plasma
//! (`kreadconfig`/`kwriteconfig` + DBus). Falls back cleanly to unavailable on
//! headless hosts or unsupported environments without a desktop session.

mod gnome;
mod kde;

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::{ProxyBackup, ProxyEndpoints, ProxySysError, SystemProxy};

pub use gnome::{
    format_gvariant_string, format_gvariant_string_list, parse_gvariant_string,
    parse_gvariant_string_list, GnomeProxyBackup,
};
pub use kde::{parse_kde_proxy_endpoint, KdeProxyBackup, KdeTools};

/// Maximum time a host proxy tool (`gsettings`, `kreadconfig*`, `kwriteconfig*`,
/// `dbus-send`) is allowed to run before it is killed and reaped.
const HOST_TOOL_TIMEOUT: Duration = Duration::from_secs(8);

/// Standard host binary directories searched before `PATH` so bundled AppImage
/// tools or user PATH entries cannot shadow host desktop utilities.
const SYSTEM_BIN_DIRS: &[&str] = &["/usr/bin", "/bin", "/usr/local/bin"];

/// Environment variables injected by AppImage runtimes that contaminate host
/// desktop tools (for example redirecting `gsettings` to an in-memory backend
/// or loading mismatched Qt/GTK libraries).
const APPIMAGE_CONTAMINATING_ENV_VARS: &[&str] = &[
    "GIO_MODULE_DIR",
    "GSETTINGS_SCHEMA_DIR",
    "GSETTINGS_BACKEND",
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "QT_PLUGIN_PATH",
    "QT_QPA_PLATFORM_PLUGIN_PATH",
    "GTK_PATH",
    "GTK_IM_MODULE_FILE",
    "GDK_PIXBUF_MODULE_FILE",
    "PYTHONHOME",
    "PYTHONPATH",
];

/// Strip AppImage-injected environment variables before spawning host tools.
///
/// When neither `APPIMAGE` nor `APPDIR` is set and non-empty, `vars` is
/// returned unchanged. Otherwise, variables in
/// [`APPIMAGE_CONTAMINATING_ENV_VARS`] are removed, and any `XDG_DATA_DIRS`
/// entries located under `$APPDIR` are filtered out (dropping `XDG_DATA_DIRS`
/// altogether if nothing remains so GLib/Qt fall back to the XDG defaults).
pub fn sanitized_host_tool_env<I, K, V>(vars: I) -> Vec<(OsString, OsString)>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<OsString>,
    V: Into<OsString>,
{
    let entries: Vec<(OsString, OsString)> = vars
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();

    let in_appimage = entries
        .iter()
        .any(|(k, v)| (k == OsStr::new("APPIMAGE") || k == OsStr::new("APPDIR")) && !v.is_empty());
    if !in_appimage {
        return entries;
    }

    let appdir = entries
        .iter()
        .find(|(k, v)| k == OsStr::new("APPDIR") && !v.is_empty())
        .map(|(_, v)| PathBuf::from(v));

    let mut out = Vec::with_capacity(entries.len());
    for (key, val) in entries {
        if key
            .to_str()
            .is_some_and(|name| APPIMAGE_CONTAMINATING_ENV_VARS.contains(&name))
        {
            continue;
        }
        if key == OsStr::new("XDG_DATA_DIRS") {
            if let Some(appdir_path) = appdir.as_deref() {
                let kept: Vec<PathBuf> = std::env::split_paths(&val)
                    .filter(|entry| {
                        !entry.as_os_str().is_empty() && !entry.starts_with(appdir_path)
                    })
                    .collect();
                if kept.is_empty() {
                    continue;
                }
                let joined = std::env::join_paths(&kept).unwrap_or(val);
                out.push((key, joined));
                continue;
            }
        }
        out.push((key, val));
    }
    out
}

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
///
/// Resolves each host tool's absolute path once (searching `/usr/bin`, `/bin`,
/// `/usr/local/bin`, then `PATH`), sanitizes AppImage environment variables,
/// and enforces a bounded execution timeout on every child process.
#[derive(Debug, Default, Clone)]
pub struct RealLinuxCmdRunner {
    resolved: Arc<Mutex<HashMap<String, PathBuf>>>,
}

impl RealLinuxCmdRunner {
    pub fn new() -> Self {
        Self::default()
    }

    fn resolve_cached(&self, program: &str) -> Option<PathBuf> {
        if let Ok(cache) = self.resolved.lock() {
            if let Some(path) = cache.get(program) {
                return Some(path.clone());
            }
        }
        let system_dirs: Vec<&Path> = SYSTEM_BIN_DIRS.iter().map(Path::new).collect();
        let found = resolve_program_in(program, &system_dirs, std::env::var_os("PATH").as_deref())?;
        if let Ok(mut cache) = self.resolved.lock() {
            cache.insert(program.to_string(), found.clone());
        }
        Some(found)
    }
}

impl LinuxCmdRunner for RealLinuxCmdRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CmdOutput, std::io::Error> {
        let resolved = self.resolve_cached(program).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("host proxy tool not found: {program}"),
            )
        })?;
        run_command_with_timeout(&resolved, args, HOST_TOOL_TIMEOUT)
    }

    fn env(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn has_program(&self, program: &str) -> bool {
        self.resolve_cached(program).is_some()
    }
}

fn resolve_program_in(
    program: &str,
    system_dirs: &[&Path],
    path_env: Option<&OsStr>,
) -> Option<PathBuf> {
    let candidate = Path::new(program);
    if candidate.is_absolute() {
        return is_executable_file(candidate).then(|| candidate.to_path_buf());
    }
    for dir in system_dirs {
        let full = dir.join(program);
        if is_executable_file(&full) {
            return Some(full);
        }
    }
    if let Some(paths) = path_env {
        for dir in std::env::split_paths(paths) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            let full = dir.join(program);
            if is_executable_file(&full) {
                return Some(full);
            }
        }
    }
    None
}

fn run_command_with_timeout(
    program: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<CmdOutput, std::io::Error> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let env = sanitized_host_tool_env(std::env::vars_os());
    cmd.env_clear().envs(env);

    let mut child = cmd.spawn()?;
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    // Drain stdout and stderr on dedicated threads so a chatty child cannot
    // block on a full OS pipe buffer while the parent waits for exit.
    let stdout_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stdout_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stderr_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None => {
                let now = Instant::now();
                if now >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!(
                            "{} timed out after {}ms",
                            program.display(),
                            timeout.as_millis()
                        ),
                    ));
                }
                let remaining = deadline.saturating_duration_since(now);
                std::thread::sleep(remaining.min(Duration::from_millis(10)));
            }
        }
    };

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    Ok(CmdOutput {
        success: status.success(),
        stdout: String::from_utf8_lossy(&stdout).to_string(),
        stderr: String::from_utf8_lossy(&stderr).to_string(),
    })
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
///
/// KDE (`kioslaverc`) is only selected in a KDE/Plasma session; non-KDE
/// sessions never fall through to KDE when `gsettings` fails or is missing so
/// a broken GNOME/GTK session cannot silently mutate unrelated KDE files.
///
/// Note on non-GNOME GTK/Wayland desktops (such as XFCE, Sway, and Hyprland):
/// when `gsettings` and the `org.gnome.system.proxy` schema are present, this
/// returns [`LinuxProxyBackend::Gnome`]. That has partial effect on those
/// desktops — GTK apps, Chromium/Electron, and Firefox honor GSettings, while
/// applications that read only environment variables or desktop-specific proxy
/// settings will not follow it.
pub fn detect_backend(runner: &dyn LinuxCmdRunner) -> LinuxProxyBackend {
    let has_session = ["DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS"]
        .iter()
        .any(|key| runner.env(key).is_some_and(|v| !v.trim().is_empty()));
    if !has_session {
        return LinuxProxyBackend::Unsupported;
    }

    if is_kde_session(runner) {
        return kde::detect_tools(runner)
            .map(LinuxProxyBackend::Kde)
            .unwrap_or(LinuxProxyBackend::Unsupported);
    }

    if gnome::is_available(runner) {
        return LinuxProxyBackend::Gnome;
    }

    LinuxProxyBackend::Unsupported
}

fn is_gnome_like_session(runner: &dyn LinuxCmdRunner) -> bool {
    for key in ["XDG_CURRENT_DESKTOP", "DESKTOP_SESSION"] {
        if let Some(raw) = runner.env(key) {
            for token in raw.split(':') {
                let lower = token.trim().to_ascii_lowercase();
                if matches!(
                    lower.as_str(),
                    "gnome"
                        | "ubuntu"
                        | "cinnamon"
                        | "x-cinnamon"
                        | "mate"
                        | "budgie"
                        | "pantheon"
                        | "cosmic"
                ) || lower.contains("gnome")
                    || lower.contains("cinnamon")
                    || lower.contains("budgie")
                {
                    return true;
                }
            }
        }
    }
    false
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
    if is_gnome_like_session(runner) {
        return false;
    }
    runner
        .env("KDE_FULL_SESSION")
        .is_some_and(|v| v.eq_ignore_ascii_case("true"))
}

/// Linux system proxy implementation dispatching to GNOME/GTK or KDE Plasma.
#[derive(Debug, Clone)]
pub struct LinuxSystemProxy<R: LinuxCmdRunner = RealLinuxCmdRunner> {
    runner: R,
    backend: LinuxProxyBackend,
}

impl LinuxSystemProxy<RealLinuxCmdRunner> {
    pub fn new() -> Self {
        Self::with_runner(RealLinuxCmdRunner::new())
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
        // Dispatch on the backend recorded when the backup was taken rather
        // than the currently detected session, so a desktop change or missing
        // session env at shutdown cannot clobber the wrong backend or silently
        // clear `applied=false` while the original proxy stays stuck on.
        match backup.extra.get("backend") {
            Some(value) if !value.is_null() => match value.as_str() {
                Some("gnome") => {
                    if !self.runner.has_program("gsettings") {
                        return Err(ProxySysError::RestoreFailed(
                            "gsettings is unavailable to restore recorded GNOME system proxy"
                                .into(),
                        ));
                    }
                    gnome::restore(&self.runner, backup)
                }
                Some("kde") => {
                    let Some(tools) = kde::detect_tools(&self.runner) else {
                        return Err(ProxySysError::RestoreFailed(
                            "kreadconfig/kwriteconfig unavailable to restore recorded KDE system proxy"
                                .into(),
                        ));
                    };
                    kde::restore(&self.runner, &tools, backup)
                }
                Some(other) => Err(ProxySysError::RestoreFailed(format!(
                    "unsupported recorded Linux proxy backend: {other}"
                ))),
                None => Err(ProxySysError::RestoreFailed(format!(
                    "invalid recorded Linux proxy backend metadata: {value}"
                ))),
            },
            _ => match &self.backend {
                LinuxProxyBackend::Gnome => gnome::restore(&self.runner, backup),
                LinuxProxyBackend::Kde(tools) => kde::restore(&self.runner, tools, backup),
                LinuxProxyBackend::Unsupported => Ok(()),
            },
        }
    }

    fn is_available(&self) -> bool {
        !matches!(self.backend, LinuxProxyBackend::Unsupported)
    }
}

pub(crate) fn bracket_ipv6_host(host: &str) -> String {
    let host = host.trim();
    if host.contains(':') && !host.starts_with('[') && !host.ends_with(']') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

pub(crate) fn format_host_port(host: &str, port: u16) -> Option<String> {
    let host = host.trim();
    if host.is_empty() || port == 0 {
        return None;
    }
    Some(format!("{}:{port}", bracket_ipv6_host(host)))
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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MockLinuxRunner {
        env: Arc<Mutex<HashMap<String, String>>>,
        programs: Arc<Mutex<HashSet<String>>>,
        responses: Arc<Mutex<HashMap<String, Result<CmdOutput, String>>>>,
        calls: Arc<Mutex<Vec<String>>>,
        persist_gsettings: Arc<AtomicBool>,
    }

    impl Default for MockLinuxRunner {
        fn default() -> Self {
            Self {
                env: Arc::new(Mutex::new(HashMap::new())),
                programs: Arc::new(Mutex::new(HashSet::new())),
                responses: Arc::new(Mutex::new(HashMap::new())),
                calls: Arc::new(Mutex::new(Vec::new())),
                persist_gsettings: Arc::new(AtomicBool::new(true)),
            }
        }
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

        fn set_persist_gsettings(&self, persist: bool) {
            self.persist_gsettings.store(persist, Ordering::Relaxed);
        }

        fn set_ok(&self, program: &str, args: &[&str], stdout: &str) {
            self.set_ok_with_stderr(program, args, stdout, "");
        }

        fn set_ok_with_stderr(&self, program: &str, args: &[&str], stdout: &str, stderr: &str) {
            let key = format!("{program} {}", args.join(" "));
            self.responses.lock().unwrap().insert(
                key,
                Ok(CmdOutput {
                    success: true,
                    stdout: stdout.to_string(),
                    stderr: stderr.to_string(),
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
            let mut map = self.responses.lock().unwrap();
            if let Some(res) = map.get(&key).cloned() {
                return match res {
                    Ok(out) => Ok(out),
                    Err(msg) => Err(std::io::Error::other(msg)),
                };
            }
            if program == "gsettings" && args.first() == Some(&"set") {
                if args.len() == 4 && self.persist_gsettings.load(Ordering::Relaxed) {
                    let get_key = format!("gsettings get {} {}", args[1], args[2]);
                    map.insert(
                        get_key,
                        Ok(CmdOutput {
                            success: true,
                            stdout: format!("{}\n", args[3]),
                            stderr: String::new(),
                        }),
                    );
                }
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
            "Proxy Config Script",
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
    fn detection_matrix_never_falls_through_to_kde_on_non_kde_desktops() {
        let kde_tools = KdeTools {
            read_cmd: "kreadconfig6",
            write_cmd: "kwriteconfig6",
        };

        // Explicit GNOME-family desktops must never fall back to KDE when
        // gsettings is missing or fails, even if KDE tools are installed.
        for desktop in [
            "GNOME",
            "ubuntu:GNOME",
            "Cinnamon",
            "X-Cinnamon",
            "MATE",
            "Budgie:GNOME",
            "Pantheon",
            "COSMIC",
        ] {
            let broken_gnome = MockLinuxRunner::default();
            broken_gnome.set_env("DISPLAY", ":0");
            broken_gnome.set_env("XDG_CURRENT_DESKTOP", desktop);
            broken_gnome.add_program("gsettings");
            broken_gnome.set_fail(
                "gsettings",
                &["get", "org.gnome.system.proxy", "mode"],
                "No such schema",
            );
            broken_gnome.add_program("kreadconfig6");
            broken_gnome.add_program("kwriteconfig6");
            assert_eq!(
                detect_backend(&broken_gnome),
                LinuxProxyBackend::Unsupported,
                "desktop={desktop} with broken gsettings must not fall back to KDE"
            );

            let working_gnome = MockLinuxRunner::default();
            working_gnome.set_env("DISPLAY", ":0");
            working_gnome.set_env("XDG_CURRENT_DESKTOP", desktop);
            working_gnome.add_program("gsettings");
            working_gnome.set_ok(
                "gsettings",
                &["get", "org.gnome.system.proxy", "mode"],
                "'none'\n",
            );
            working_gnome.add_program("kreadconfig6");
            working_gnome.add_program("kwriteconfig6");
            assert_eq!(
                detect_backend(&working_gnome),
                LinuxProxyBackend::Gnome,
                "desktop={desktop} with working gsettings should select GNOME"
            );
        }

        // Non-GNOME GTK / Wayland desktops (XFCE, Sway, Hyprland) use GNOME
        // gsettings when available (partial effect) and never fall back to KDE.
        for desktop in ["XFCE", "sway", "Hyprland"] {
            let with_gsettings = MockLinuxRunner::default();
            with_gsettings.set_env("WAYLAND_DISPLAY", "wayland-0");
            with_gsettings.set_env("XDG_CURRENT_DESKTOP", desktop);
            with_gsettings.add_program("gsettings");
            with_gsettings.set_ok(
                "gsettings",
                &["get", "org.gnome.system.proxy", "mode"],
                "'none'\n",
            );
            with_gsettings.add_program("kreadconfig6");
            with_gsettings.add_program("kwriteconfig6");
            assert_eq!(
                detect_backend(&with_gsettings),
                LinuxProxyBackend::Gnome,
                "desktop={desktop} with gsettings should select GNOME"
            );

            let without_gsettings = MockLinuxRunner::default();
            without_gsettings.set_env("WAYLAND_DISPLAY", "wayland-0");
            without_gsettings.set_env("XDG_CURRENT_DESKTOP", desktop);
            without_gsettings.add_program("kreadconfig6");
            without_gsettings.add_program("kwriteconfig6");
            assert_eq!(
                detect_backend(&without_gsettings),
                LinuxProxyBackend::Unsupported,
                "desktop={desktop} without gsettings must not fall back to KDE"
            );
        }

        // KDE sessions select KDE only when KDE tools exist and never fall back
        // to GNOME when KDE tools are absent.
        let kde_without_tools = MockLinuxRunner::default();
        kde_without_tools.set_env("WAYLAND_DISPLAY", "wayland-0");
        kde_without_tools.set_env("XDG_CURRENT_DESKTOP", "KDE");
        kde_without_tools.add_program("gsettings");
        kde_without_tools.set_ok(
            "gsettings",
            &["get", "org.gnome.system.proxy", "mode"],
            "'none'\n",
        );
        assert_eq!(
            detect_backend(&kde_without_tools),
            LinuxProxyBackend::Unsupported
        );

        let kde_full_session = MockLinuxRunner::default();
        kde_full_session.set_env("DISPLAY", ":0");
        kde_full_session.set_env("KDE_FULL_SESSION", "true");
        kde_full_session.add_program("kreadconfig6");
        kde_full_session.add_program("kwriteconfig6");
        assert_eq!(
            detect_backend(&kde_full_session),
            LinuxProxyBackend::Kde(kde_tools)
        );
    }

    #[test]
    fn restore_dispatches_on_recorded_backend_over_detected_backend() {
        // Backup recorded under GNOME, restored while detected backend is KDE.
        let runner = MockLinuxRunner::default();
        seed_kde6(&runner, "0");
        runner.add_program("gsettings");
        let proxy = LinuxSystemProxy::with_runner(runner.clone());
        assert!(matches!(proxy.backend(), LinuxProxyBackend::Kde(_)));

        let gnome_backup = ProxyBackup {
            enabled: false,
            http: None,
            https: None,
            socks: None,
            extra: serde_json::json!({
                "backend": "gnome",
                "gnome": {
                    "mode": "none",
                    "autoconfig_url": "",
                    "ignore_hosts": ["localhost"],
                    "use_same_proxy": true,
                    "http_host": "",
                    "http_port": 0,
                    "https_host": "",
                    "https_port": 0,
                    "socks_host": "",
                    "socks_port": 0
                }
            }),
        };
        proxy.restore(&gnome_backup).unwrap();
        let calls = runner.calls();
        assert!(
            calls
                .iter()
                .any(|c| c == "gsettings set org.gnome.system.proxy mode 'none'"),
            "must restore GNOME via gsettings: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| c.starts_with("kwriteconfig")),
            "must not mutate KDE config when restoring a GNOME backup: {calls:?}"
        );

        // Backup recorded under KDE, restored while detected backend is GNOME.
        let gnome_runner = MockLinuxRunner::default();
        seed_gnome_defaults(&gnome_runner);
        gnome_runner.add_program("kreadconfig6");
        gnome_runner.add_program("kwriteconfig6");
        gnome_runner.add_program("dbus-send");
        let gnome_proxy = LinuxSystemProxy::with_runner(gnome_runner.clone());
        assert_eq!(gnome_proxy.backend(), &LinuxProxyBackend::Gnome);

        let kde_backup = ProxyBackup {
            enabled: false,
            http: None,
            https: None,
            socks: None,
            extra: serde_json::json!({
                "backend": "kde",
                "kde": {
                    "proxy_type": "0",
                    "http_proxy": "",
                    "https_proxy": "",
                    "socks_proxy": "",
                    "no_proxy_for": "",
                    "reversed_exception": "false",
                    "proxy_config_script": ""
                }
            }),
        };
        gnome_proxy.restore(&kde_backup).unwrap();
        let calls = gnome_runner.calls();
        assert!(
            calls
                .iter()
                .any(|c| c
                    == "kwriteconfig6 --file kioslaverc --group Proxy Settings --key ProxyType 0"),
            "must restore KDE via kwriteconfig6: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| c.starts_with("gsettings set")),
            "must not mutate GNOME when restoring a KDE backup: {calls:?}"
        );
    }

    #[test]
    fn restore_works_without_session_env_and_fails_when_recorded_tools_missing() {
        // Headless (no DISPLAY/WAYLAND_DISPLAY/DBUS_SESSION_BUS_ADDRESS), so
        // detected backend is Unsupported, yet gsettings IS available: recorded
        // GNOME backup must still restore via gsettings.
        let headless_with_gsettings = MockLinuxRunner::default();
        headless_with_gsettings.add_program("gsettings");
        let proxy = LinuxSystemProxy::with_runner(headless_with_gsettings.clone());
        assert_eq!(proxy.backend(), &LinuxProxyBackend::Unsupported);

        let gnome_backup = ProxyBackup {
            extra: serde_json::json!({ "backend": "gnome" }),
            ..ProxyBackup::default()
        };
        proxy.restore(&gnome_backup).unwrap();
        assert!(headless_with_gsettings
            .calls()
            .contains(&"gsettings set org.gnome.system.proxy mode 'none'".to_string()));

        // Recorded GNOME backup when gsettings is missing must return Err,
        // never Ok(()) (which would clear `applied=false` while stuck on).
        let no_tools = MockLinuxRunner::default();
        let unavailable_proxy = LinuxSystemProxy::with_runner(no_tools);
        assert_eq!(unavailable_proxy.backend(), &LinuxProxyBackend::Unsupported);
        let err = unavailable_proxy
            .restore(&gnome_backup)
            .expect_err("missing gsettings must fail restore");
        assert!(matches!(err, ProxySysError::RestoreFailed(_)));

        // Recorded KDE backup when kreadconfig/kwriteconfig are missing must
        // also return RestoreFailed, even if currently in a GNOME session.
        let gnome_only = MockLinuxRunner::default();
        seed_gnome_defaults(&gnome_only);
        let gnome_proxy = LinuxSystemProxy::with_runner(gnome_only);
        let kde_backup = ProxyBackup {
            extra: serde_json::json!({ "backend": "kde" }),
            ..ProxyBackup::default()
        };
        let err = gnome_proxy
            .restore(&kde_backup)
            .expect_err("missing KDE tools must fail restore");
        assert!(matches!(err, ProxySysError::RestoreFailed(_)));

        // Unknown recorded backend must also fail closed.
        let unknown_backup = ProxyBackup {
            extra: serde_json::json!({ "backend": "unknown-de" }),
            ..ProxyBackup::default()
        };
        let err = gnome_proxy
            .restore(&unknown_backup)
            .expect_err("unknown recorded backend must fail restore");
        assert!(matches!(err, ProxySysError::RestoreFailed(_)));
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
    fn gnome_apply_brackets_ipv6_hosts() {
        let runner = MockLinuxRunner::default();
        seed_gnome_defaults(&runner);
        let proxy = LinuxSystemProxy::with_runner(runner.clone());

        proxy
            .apply(&ProxyEndpoints {
                http_host: "::1".into(),
                http_port: 17890,
                socks_host: Some("::1".into()),
                socks_port: Some(17890),
            })
            .unwrap();

        let calls = runner.calls();
        assert!(
            calls.contains(&"gsettings set org.gnome.system.proxy.http host '[::1]'".to_string())
        );
        assert!(
            calls.contains(&"gsettings set org.gnome.system.proxy.https host '[::1]'".to_string())
        );
        assert!(
            calls.contains(&"gsettings set org.gnome.system.proxy.socks host '[::1]'".to_string())
        );

        // Already-bracketed IPv6 must not be double-bracketed.
        assert_eq!(bracket_ipv6_host("[::1]"), "[::1]");
        assert_eq!(bracket_ipv6_host("::1"), "[::1]");
        assert_eq!(bracket_ipv6_host("127.0.0.1"), "127.0.0.1");
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
    fn gnome_apply_fails_and_rolls_back_on_readback_mismatch() {
        let runner = MockLinuxRunner::default();
        seed_gnome_defaults(&runner);
        // Simulate silent writes to an in-memory GSettings backend: `set`
        // exits 0 without changing what `get` returns.
        runner.set_persist_gsettings(false);

        let proxy = LinuxSystemProxy::with_runner(runner.clone());
        let err = proxy
            .apply(&ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: Some("127.0.0.1".into()),
                socks_port: Some(17890),
            })
            .expect_err("silent memory-backend write must fail read-back");
        assert!(matches!(err, ProxySysError::ApplyFailed(ref msg) if msg.contains("read-back")));
        let calls = runner.calls();
        assert!(
            calls
                .iter()
                .any(|c| c == "gsettings set org.gnome.system.proxy mode 'none'"),
            "read-back mismatch must roll back to pre-apply snapshot: {calls:?}"
        );
    }

    #[test]
    fn gnome_apply_fails_and_rolls_back_on_memory_backend_stderr() {
        let runner = MockLinuxRunner::default();
        seed_gnome_defaults(&runner);
        runner.set_ok_with_stderr(
            "gsettings",
            &["set", "org.gnome.system.proxy.http", "host", "'127.0.0.1'"],
            "",
            "GLib-GIO-Message: 12:00:00.000: Using the 'memory' GSettings backend.  Your settings will not be saved or shared with other applications.\n",
        );

        let proxy = LinuxSystemProxy::with_runner(runner.clone());
        let err = proxy
            .apply(&ProxyEndpoints {
                http_host: "127.0.0.1".into(),
                http_port: 17890,
                socks_host: Some("127.0.0.1".into()),
                socks_port: Some(17890),
            })
            .expect_err("memory backend warning on stderr must fail apply");
        assert!(matches!(err, ProxySysError::ApplyFailed(ref msg) if msg.contains("in-memory")));
        let calls = runner.calls();
        assert!(
            calls
                .iter()
                .any(|c| c == "gsettings set org.gnome.system.proxy mode 'none'"),
            "memory-backend failure must roll back: {calls:?}"
        );
    }

    #[test]
    fn gnome_apply_reads_back_socks_host_when_http_not_set() {
        let runner = MockLinuxRunner::default();
        seed_gnome_defaults(&runner);
        // Let mode persist, but keep socks host stale so socks read-back fails.
        runner.set_ok(
            "gsettings",
            &["set", "org.gnome.system.proxy.socks", "host", "'127.0.0.1'"],
            "",
        );

        let proxy = LinuxSystemProxy::with_runner(runner.clone());
        let err = proxy
            .apply(&ProxyEndpoints {
                http_host: String::new(),
                http_port: 0,
                socks_host: Some("127.0.0.1".into()),
                socks_port: Some(17890),
            })
            .expect_err("socks.host mismatch should fail when http endpoint is empty");
        assert!(matches!(err, ProxySysError::ApplyFailed(ref msg) if msg.contains("socks")));
    }

    #[test]
    fn kde_backup_apply_and_restore_roundtrip_notifies_kio_and_uses_spaced_pac_key() {
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
        runner.set_ok(
            "kreadconfig6",
            &[
                "--file",
                "kioslaverc",
                "--group",
                "Proxy Settings",
                "--key",
                "Proxy Config Script",
            ],
            "http://corp.local/proxy.pac\n",
        );

        let proxy = LinuxSystemProxy::with_runner(runner.clone());
        let backup = proxy.backup().unwrap();
        assert!(backup.enabled);
        assert_eq!(backup.http.as_deref(), Some("10.0.0.5:8080"));
        assert_eq!(backup.https.as_deref(), Some("10.0.0.5:8080"));
        assert_eq!(
            backup.extra["kde"]["proxy_config_script"],
            "http://corp.local/proxy.pac"
        );

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
        assert!(calls.iter().any(|c| {
            c == "kwriteconfig6 --file kioslaverc --group Proxy Settings --key Proxy Config Script http://corp.local/proxy.pac"
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

    #[test]
    fn sanitized_host_tool_env_is_noop_outside_appimage() {
        let input = vec![
            ("DISPLAY", ":0"),
            ("GIO_MODULE_DIR", "/custom/gio"),
            ("LD_LIBRARY_PATH", "/custom/lib"),
            ("XDG_DATA_DIRS", "/custom/share:/usr/share"),
        ];
        let result = sanitized_host_tool_env(input.clone());
        let expected: Vec<(OsString, OsString)> = input
            .into_iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect();
        assert_eq!(result, expected);
    }

    #[test]
    fn sanitized_host_tool_env_strips_appimage_vars_and_filters_xdg_data_dirs() {
        let mut input: Vec<(&str, &str)> = vec![
            ("APPIMAGE", "/home/u/ice-box.AppImage"),
            ("APPDIR", "/tmp/.mount_ice123"),
            ("DISPLAY", ":0"),
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
            (
                "XDG_DATA_DIRS",
                "/tmp/.mount_ice123/usr/share:/tmp/.mount_ice1234/usr/share:/usr/local/share:/usr/share",
            ),
        ];
        for var in APPIMAGE_CONTAMINATING_ENV_VARS {
            input.push((var, "/tmp/.mount_ice123/usr/lib"));
        }

        let sanitized: HashMap<OsString, OsString> =
            sanitized_host_tool_env(input).into_iter().collect();

        for var in APPIMAGE_CONTAMINATING_ENV_VARS {
            assert!(
                !sanitized.contains_key(OsStr::new(var)),
                "{var} should be stripped under AppImage"
            );
        }
        assert_eq!(
            sanitized.get(OsStr::new("DISPLAY")),
            Some(&OsString::from(":0"))
        );
        assert_eq!(
            sanitized.get(OsStr::new("DBUS_SESSION_BUS_ADDRESS")),
            Some(&OsString::from("unix:path=/run/user/1000/bus"))
        );
        assert_eq!(
            sanitized.get(OsStr::new("XDG_DATA_DIRS")),
            Some(&OsString::from(
                "/tmp/.mount_ice1234/usr/share:/usr/local/share:/usr/share"
            ))
        );
    }

    #[test]
    fn sanitized_host_tool_env_removes_xdg_data_dirs_when_only_appdir_entries_exist() {
        let input = vec![
            ("APPDIR", "/tmp/.mount_ice123/"),
            (
                "XDG_DATA_DIRS",
                "/tmp/.mount_ice123/usr/share:/tmp/.mount_ice123/usr/local/share",
            ),
            ("GSETTINGS_BACKEND", "memory"),
        ];
        let sanitized: HashMap<OsString, OsString> =
            sanitized_host_tool_env(input).into_iter().collect();
        assert!(!sanitized.contains_key(OsStr::new("XDG_DATA_DIRS")));
        assert!(!sanitized.contains_key(OsStr::new("GSETTINGS_BACKEND")));
    }

    #[test]
    fn resolve_program_in_prefers_system_dirs_over_path() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::{SystemTime, UNIX_EPOCH};

        let root = std::env::temp_dir().join(format!(
            "ice-proxy-resolve-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sys_dir = root.join("sys");
        let path_dir = root.join("path");
        std::fs::create_dir_all(&sys_dir).unwrap();
        std::fs::create_dir_all(&path_dir).unwrap();

        let make_exec = |p: &Path| {
            std::fs::write(p, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
        };

        let sys_tool = sys_dir.join("gsettings");
        let path_tool = path_dir.join("gsettings");
        make_exec(&sys_tool);
        make_exec(&path_tool);

        let path_only_tool = path_dir.join("kwriteconfig6");
        make_exec(&path_only_tool);

        let non_exec = sys_dir.join("not-exec");
        std::fs::write(&non_exec, "plain").unwrap();

        let resolved_sys = resolve_program_in(
            "gsettings",
            &[sys_dir.as_path()],
            Some(path_dir.as_os_str()),
        );
        assert_eq!(resolved_sys.as_deref(), Some(sys_tool.as_path()));

        let resolved_path = resolve_program_in(
            "kwriteconfig6",
            &[sys_dir.as_path()],
            Some(path_dir.as_os_str()),
        );
        assert_eq!(resolved_path.as_deref(), Some(path_only_tool.as_path()));

        assert_eq!(
            resolve_program_in("not-exec", &[sys_dir.as_path()], Some(path_dir.as_os_str())),
            None
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn run_command_with_timeout_captures_output_and_kills_hung_children() {
        let sh = Path::new("/bin/sh");
        let ok = run_command_with_timeout(sh, &["-c", "printf hello"], Duration::from_secs(2))
            .expect("fast command");
        assert!(ok.success);
        assert_eq!(ok.stdout, "hello");

        let start = Instant::now();
        let err = run_command_with_timeout(sh, &["-c", "sleep 10"], Duration::from_millis(120))
            .expect_err("slow command must time out");
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "timed-out command should be killed promptly, took {:?}",
            start.elapsed()
        );
    }
}
