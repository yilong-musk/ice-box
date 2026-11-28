// SPDX-License-Identifier: GPL-3.0-or-later

//! Headless daemon runner for macOS, Windows, and Linux.

use crate::exit::{EXIT_ERROR, EXIT_OK};
use crate::signal::{install_shutdown_handlers, ShutdownSignal};
use ice_config::AppPaths;
use ice_runtime::application::{
    append_recovery_warning, auto_start_on_launch, collect_status, lock_orchestrate,
    recover_launch_leftovers, request_runtime_probe_refresh, start_service, AppHost, AppResources,
};
use ice_runtime::mcp::{McpContext, McpPolicy, McpService};
use ice_runtime::shutdown::{request_headless_quit, QuitOutcome};
use ice_runtime::{
    core_watch, helper_install, install_panic_hook, instance, orchestrate, rule_set_watch,
    runtime_status, set_panic_log_path, subscription_watch, AppState, HostCapabilities,
};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

/// Options controlling the headless daemon session.
#[derive(Debug, Clone)]
pub struct DaemonOptions {
    pub data_dir: PathBuf,
    pub resource_dir: Option<PathBuf>,
    pub auto_start_core: bool,
    pub start_service: bool,
    pub policy: McpPolicy,
    pub allow_interactive_elevation: bool,
    pub print_ready: bool,
}

/// Headless implementation of `ice_runtime::AppHost`.
#[derive(Clone)]
pub struct HeadlessHost {
    resource_dir: Option<PathBuf>,
    prefer_resource_dir: bool,
    state: Arc<AppState>,
    interactive_elevation: bool,
}

impl HeadlessHost {
    pub fn new(
        state: Arc<AppState>,
        resource_dir: Option<PathBuf>,
        prefer_resource_dir: bool,
        interactive_elevation: bool,
    ) -> Self {
        Self {
            resource_dir,
            prefer_resource_dir,
            state,
            interactive_elevation,
        }
    }
}

impl AppResources for HeadlessHost {
    fn resource_dir(&self) -> Option<PathBuf> {
        self.resource_dir.clone()
    }

    fn prefer_resource_dir(&self) -> bool {
        self.prefer_resource_dir
    }
}

impl AppHost for HeadlessHost {
    fn state_changed(&self) {
        request_runtime_probe_refresh(&self.state);
    }

    fn interactive_elevation(&self) -> bool {
        self.interactive_elevation
    }
}

/// Resolve the portable CLI resource directory (`sing-box`, `ice-helper`,
/// `ice-tun-launcher.exe`, `geoip/`) and whether it should take precedence
/// over the compile-time development checkout (`third_party/sing-box`).
///
/// Order:
/// 1. Explicit `--resource-dir` or `ICE_BOX_RESOURCE_DIR` (preferred)
/// 2. Sibling `resources/` or same directory as `current_exe()`, checking the
///    canonicalized symlink target first (so symlinks in `/usr/local/bin` or
///    `~/.local/bin` still find the bundled `resources/` next to the real
///    binary on macOS and Linux), then the invocation directory if different,
///    when it contains `sing-box[.exe]` or `geoip/` (preferred)
/// 3. Standard Linux FHS install directories (`/usr/lib/ice-box`, `/usr/share/ice-box`) (preferred)
/// 4. Fallback `exe_dir` / `None` (dev builds resolve `third_party/sing-box` via `orchestrate`)
pub fn resolve_resource_dir(explicit: Option<PathBuf>) -> Option<PathBuf> {
    resolve_resource_dir_with_preference(explicit).0
}

pub(crate) fn resolve_resource_dir_with_preference(
    explicit: Option<PathBuf>,
) -> (Option<PathBuf>, bool) {
    if let Some(dir) = explicit {
        return (Some(dir), true);
    }
    if let Some(env_dir) = std::env::var_os("ICE_BOX_RESOURCE_DIR").filter(|v| !v.is_empty()) {
        return (Some(PathBuf::from(env_dir)), true);
    }
    let exe_path = std::env::current_exe().ok();
    resolve_resource_dir_from_exe_path_with_preference(exe_path.as_deref())
}

pub(crate) fn resolve_resource_dir_from_exe_path_with_preference(
    exe_path: Option<&Path>,
) -> (Option<PathBuf>, bool) {
    let mut exe_dirs = Vec::new();
    if let Some(path) = exe_path {
        if let Ok(canonical) = std::fs::canonicalize(path) {
            if let Some(parent) = canonical.parent() {
                exe_dirs.push(parent.to_path_buf());
            }
        }
        if let Some(parent) = path.parent() {
            let parent_buf = parent.to_path_buf();
            if !exe_dirs.iter().any(|existing| existing == &parent_buf) {
                exe_dirs.push(parent_buf);
            }
        }
    }
    resolve_resource_dir_from_dirs_with_preference(&exe_dirs)
}

#[cfg(test)]
pub(crate) fn resolve_resource_dir_from_candidates(exe_dir: Option<&Path>) -> Option<PathBuf> {
    resolve_resource_dir_from_candidates_with_preference(exe_dir).0
}

#[cfg(test)]
pub(crate) fn resolve_resource_dir_from_candidates_with_preference(
    exe_dir: Option<&Path>,
) -> (Option<PathBuf>, bool) {
    let dirs: Vec<PathBuf> = exe_dir.into_iter().map(Path::to_path_buf).collect();
    resolve_resource_dir_from_dirs_with_preference(&dirs)
}

pub(crate) fn resolve_resource_dir_from_dirs_with_preference(
    exe_dirs: &[PathBuf],
) -> (Option<PathBuf>, bool) {
    let bin_name = if cfg!(windows) {
        "sing-box.exe"
    } else {
        "sing-box"
    };
    let mut candidates = Vec::new();
    for dir in exe_dirs {
        candidates.push(dir.join("resources"));
        candidates.push(dir.to_path_buf());
    }
    #[cfg(target_os = "linux")]
    {
        candidates.push(PathBuf::from("/usr/lib/ice-box"));
        candidates.push(PathBuf::from("/usr/share/ice-box"));
    }
    for candidate in candidates {
        if candidate.join(bin_name).is_file() || candidate.join("geoip").is_dir() {
            return (Some(candidate), true);
        }
    }
    (exe_dirs.first().cloned(), false)
}

/// Run the headless daemon until terminated by an OS signal.
pub fn run_daemon(opts: DaemonOptions) -> i32 {
    install_panic_hook();
    let signal = ShutdownSignal::new();
    if let Err(err) = install_shutdown_handlers(signal.clone()) {
        eprintln!("ice-box-cli daemon: {err}");
        return EXIT_ERROR;
    }
    run_daemon_with_signal(opts, signal)
}

/// Testable entry point accepting an explicit [`ShutdownSignal`].
pub fn run_daemon_with_signal(opts: DaemonOptions, signal: ShutdownSignal) -> i32 {
    struct DoneGuard(ShutdownSignal);
    impl Drop for DoneGuard {
        fn drop(&mut self) {
            self.0.mark_done();
        }
    }
    let _done_guard = DoneGuard(signal.clone());

    let paths = AppPaths::new(&opts.data_dir);
    set_panic_log_path(paths.app_log());

    // Acquire the data-dir lock without writing a GUI focus file.
    let instance_lock = match instance::acquire_or_request_focus(&paths, false) {
        Ok(instance::InstanceLock::Primary(file)) => file,
        Ok(instance::InstanceLock::Secondary) => {
            eprintln!(
                "ice-box-cli daemon: another ice-box instance already owns {}",
                paths.root().display()
            );
            return EXIT_ERROR;
        }
        Err(err) => {
            eprintln!("ice-box-cli daemon: acquire instance lock: {err}");
            return EXIT_ERROR;
        }
    };

    #[cfg(target_os = "windows")]
    ice_runtime::session_end::install(paths.proxy_backup());

    let (resource_dir, prefer_resource_dir) =
        resolve_resource_dir_with_preference(opts.resource_dir);
    let state = match AppState::bootstrap(
        paths.clone(),
        instance_lock,
        resource_dir.clone(),
        HostCapabilities::headless(),
    ) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("ice-box-cli daemon: bootstrap failed: {err}");
            return EXIT_ERROR;
        }
    };

    let host = Arc::new(HeadlessHost::new(
        Arc::clone(&state),
        resource_dir.clone(),
        prefer_resource_dir,
        opts.allow_interactive_elevation,
    ));

    // Establish initial committed read before startup takes the orchestrate lock.
    let _ = collect_status(&state);

    // Warm up bundled geoip rule-sets and helper SHA cache in the background.
    {
        let warmup_paths = state.paths.clone();
        let warmup_resource = resource_dir.clone();
        std::thread::spawn(move || {
            let _ = orchestrate::ensure_geoip_rule_sets(&warmup_paths, warmup_resource.as_deref());
            let _ = helper_install::helper_core_stale(warmup_resource.as_deref());
        });
    }

    // Always run crash leftover recovery under the orchestrate lock before any
    // control RPC can race on stale cores, proxy backups, or TUN journals.
    {
        let startup_host = Arc::clone(&host);
        let startup_state = Arc::clone(&state);
        let auto_start_core = opts.auto_start_core;
        let start_service_after = opts.start_service;
        let (orch_tx, orch_rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            if auto_start_core {
                if let Err(err) = auto_start_on_launch(&startup_host, &startup_state, Some(orch_tx))
                {
                    if startup_state.shutdown_requested.load(Ordering::SeqCst) {
                        tracing::info!(error = %err, "auto-start aborted by shutdown");
                        return;
                    }
                    tracing::error!(error = %err, "daemon auto-start failed");
                    append_recovery_warning(
                        &startup_state,
                        ice_config::UiMessage::new("recover.autoStartFailed")
                            .with("detail", err.to_string()),
                    );
                    return;
                }
                if start_service_after && !startup_state.shutdown_requested.load(Ordering::SeqCst) {
                    if let Err(err) = start_service(&startup_host, &startup_state) {
                        tracing::error!(error = %err, "daemon --start-service failed");
                        append_recovery_warning(
                            &startup_state,
                            ice_config::UiMessage::new("recover.autoStartFailed")
                                .with("detail", err.to_string()),
                        );
                    }
                }
            } else {
                let Ok(_orch) = lock_orchestrate(&startup_state) else {
                    let _ = orch_tx.send(());
                    return;
                };
                let _ = orch_tx.send(());
                recover_launch_leftovers(&startup_state);
            }
        });
        if orch_rx.recv().is_err() {
            tracing::warn!("startup worker exited before acquiring orchestrate lock");
        }
    }

    let mcp = McpService::new(state.paths.root().to_path_buf());
    let ctx = McpContext::new(Arc::clone(&state), host.clone()).with_policy(opts.policy);
    let mcp_status = match mcp.start_headless(&ctx) {
        Ok(status) => status,
        Err(err) => {
            eprintln!("ice-box-cli daemon: start local control listener: {err}");
            let _ = request_headless_quit(&state, host.as_ref(), Some(&mcp));
            return EXIT_ERROR;
        }
    };

    core_watch::spawn_core_watchdog(Arc::clone(&state));
    runtime_status::spawn_probe_watchdog(Arc::clone(&state), Arc::new(|| {}));
    subscription_watch::spawn_subscription_watchdog(Arc::clone(&state), Arc::clone(&host));
    rule_set_watch::spawn_rule_set_watchdog(Arc::clone(&state), Arc::clone(&host));

    tracing::info!(
        data_dir = %state.paths.root().display(),
        endpoint = mcp_status.endpoint.as_deref().unwrap_or(""),
        read_only = opts.policy.read_only,
        allow_secrets = opts.policy.allow_secrets,
        "ice-box headless daemon ready"
    );

    if opts.print_ready {
        let ready = json!({
            "ready": true,
            "pid": std::process::id(),
            "data_dir": state.paths.root().to_string_lossy(),
            "endpoint": mcp_status.endpoint,
            "read_only": opts.policy.read_only,
            "allow_secrets": opts.policy.allow_secrets,
        });
        println!("{ready}");
    }

    signal.wait();
    tracing::info!("ice-box headless daemon shutting down");

    // Bound total shutdown time so systemd / CI / containers never stall.
    let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
    let shutdown_state = Arc::clone(&state);
    let shutdown_host = Arc::clone(&host);
    let outcome = match std::thread::Builder::new()
        .name("ice-cli-shutdown".into())
        .spawn(move || {
            let outcome =
                request_headless_quit(&shutdown_state, shutdown_host.as_ref(), Some(&mcp));
            let _ = done_tx.send(outcome);
        }) {
        Ok(_) => match done_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(outcome) => outcome,
            Err(_) => {
                tracing::warn!("daemon graceful shutdown timed out after 10s");
                QuitOutcome::StopFailed
            }
        },
        Err(err) => {
            tracing::error!(error = %err, "failed to spawn shutdown thread");
            QuitOutcome::StopFailed
        }
    };

    state.release_instance_lock();
    match outcome {
        QuitOutcome::Stopped => EXIT_OK,
        QuitOutcome::ProxyRestoreFailed | QuitOutcome::StopFailed | QuitOutcome::LockPoisoned => {
            EXIT_ERROR
        }
    }
}
