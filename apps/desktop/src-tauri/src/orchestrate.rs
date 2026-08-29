//! Start / Stop / Apply orchestration (architecture §8). Does not touch system proxy from crates.

use ice_config::{
    build_direct_only_config, build_runtime_config, clash_mode_name, load_group_selections,
    load_rule_overrides, load_settings, restore_runtime_config_from_bak, save_settings,
    write_runtime_config_file, AppError, AppPaths, AppSettings, BuildInput, CaptureIntent,
    ErrorCode, NormalizedProfile,
};
use ice_core::{
    get_mode, resolve_singbox_binary, set_mode, CoreHandle, CorePaths, CoreStatus, HealthEndpoints,
    ReloadOutcome,
};
use ice_proxy_sys::{
    apply_and_record, is_proxy_applied_on_disk, is_proxy_live_applied, restore_and_clear_flag,
    ProxyEndpoints, SystemProxy,
};
use ice_subscription::{
    load_active_profile, load_index, resolve_selected_tag, SubscriptionError, SubscriptionPaths,
};
use std::path::{Path, PathBuf};

pub fn repo_third_party_singbox() -> PathBuf {
    // apps/desktop/src-tauri → repo root
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../third_party/sing-box")
}

pub fn repo_geoip_rule_sets() -> PathBuf {
    // apps/desktop/src-tauri → repo root
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../third_party/sing-geoip/rule-set")
}

/// Ensure `geoip-{code}.srs` rule-sets exist in the app data dir (copied from bundled
/// resources, falling back to the repo copy for dev/tests). Returns the directory.
pub fn ensure_geoip_rule_sets(app_paths: &AppPaths, resource_dir: Option<&Path>) -> PathBuf {
    let target = app_paths.geoip_dir();
    let mut sources: Vec<PathBuf> = Vec::new();
    if let Some(dir) = resource_dir {
        sources.push(dir.join("geoip"));
    }
    sources.push(repo_geoip_rule_sets());

    for src in sources {
        if !src.is_dir() {
            continue;
        }
        if std::fs::create_dir_all(&target).is_err() {
            break;
        }
        let mut copied = 0usize;
        if let Ok(entries) = std::fs::read_dir(&src) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                if !name.to_string_lossy().ends_with(".srs") {
                    continue;
                }
                let dest = target.join(&name);
                if !dest.is_file() && std::fs::copy(entry.path(), &dest).is_ok() {
                    copied += 1;
                }
            }
        }
        tracing::info!(dir = %target.display(), copied, "geoip rule-sets ensured");
        break;
    }
    target
}

pub fn build_core_paths(
    app_paths: &AppPaths,
    settings: &AppSettings,
    binary: PathBuf,
) -> CorePaths {
    // With allow_lan the mixed inbound binds 0.0.0.0; probe/UI keep loopback so the
    // health check and displayed endpoint always work.
    let probe_host = if settings.allow_lan {
        "127.0.0.1"
    } else {
        &settings.mixed_listen
    };
    CorePaths {
        binary,
        config: app_paths.config(),
        log_file: app_paths.core_log(),
        pid_file: app_paths.pid(),
        inbound_host: probe_host.to_string(),
        inbound_port: settings.mixed_port,
        clash_api_host: settings.clash_api_listen.clone(),
        clash_api_port: settings.clash_api_port,
        allow_lan: settings.allow_lan,
    }
}

pub fn endpoints_from_settings(settings: &AppSettings) -> ProxyEndpoints {
    let host = if settings.allow_lan {
        "127.0.0.1".to_string()
    } else {
        settings.mixed_listen.clone()
    };
    ProxyEndpoints {
        http_host: host.clone(),
        http_port: settings.mixed_port,
        // Windows WinInet stores SOCKS in the multi-protocol ProxyServer string
        // (`socks=host:port`); macOS sets a real SOCKS service. Both expose it here
        // so `is_proxy_live_applied` can verify the apply.
        socks_host: Some(host),
        socks_port: Some(settings.mixed_port),
    }
}

/// When `selected_tag` no longer exists in the active profile (e.g. subscription switch),
/// persist the resolved fallback so UI and on-disk settings stay aligned.
pub fn reconcile_selected_tag_in_settings(
    app_paths: &AppPaths,
    settings: &AppSettings,
    profile: &NormalizedProfile,
) -> Result<AppSettings, AppError> {
    let updated = reconcile_selected_tag(settings, profile);
    if updated.selected_tag != settings.selected_tag {
        save_settings(&app_paths.settings(), &updated)?;
    }
    Ok(updated)
}

/// Resolve `selected_tag` against the active profile **without** writing disk.
///
/// The capture controller uses this to pre-reconcile a transition candidate so
/// `generate_config` never persists settings mid-transition (plan §4.3 commits
/// `settings.json` only after the requested backend is healthy).
pub fn reconcile_selected_tag(settings: &AppSettings, profile: &NormalizedProfile) -> AppSettings {
    let resolved = resolve_selected_tag(settings.selected_tag.as_deref(), profile);
    if settings.selected_tag.as_ref() != resolved.as_ref() {
        let mut updated = settings.clone();
        updated.selected_tag = resolved;
        updated
    } else {
        settings.clone()
    }
}

/// Rebuild `config.json` from the single active subscription profile. Returns whether inbound
/// listen changed vs previous on-disk settings snapshot (caller compares ports).
///
/// `capture_intent` is supplied explicitly by the caller (plan §4.1): automatic core start
/// and every pre-T3 path pass [`CaptureIntent::Diagnostic`]; the TUN controller (slice T3)
/// passes `Tun` only during a TUN capture transition.
pub fn generate_config(
    app_paths: &AppPaths,
    settings: &AppSettings,
    resource_dir: Option<&Path>,
    capture_intent: CaptureIntent,
) -> Result<(), AppError> {
    let sub_paths = SubscriptionPaths::from_app(app_paths);
    let index = load_index(&sub_paths).map_err(AppError::from)?;
    let profile = match load_active_profile(&sub_paths, &index) {
        Ok(profile) => profile,
        Err(SubscriptionError::NoActiveSubscription) => {
            // First-run / all subscriptions removed: fall back to a direct-only
            // config so Start keeps working (system proxy + inbound, all traffic
            // direct) until a subscription is imported.
            let config = build_direct_only_config(&settings.to_local_template(), capture_intent)?;
            write_runtime_config_file(&app_paths.config(), &app_paths.config_bak(), &config)?;
            return Ok(());
        }
        Err(err) => return Err(AppError::from(err)),
    };
    if profile.nodes.is_empty() {
        // Active subscription exists but yields no leaf outbounds (e.g. groups-only, or a
        // hand-edited profile): nothing usable to route through — direct-only fallback so
        // Start/Apply keep working (build_runtime_config errors on empty nodes).
        let config = build_direct_only_config(&settings.to_local_template(), capture_intent)?;
        write_runtime_config_file(&app_paths.config(), &app_paths.config_bak(), &config)?;
        return Ok(());
    }
    let settings = reconcile_selected_tag_in_settings(app_paths, settings, &profile)?;
    let selected = resolve_selected_tag(settings.selected_tag.as_deref(), &profile);
    let geoip_dir = ensure_geoip_rule_sets(app_paths, resource_dir);
    let group_selections = load_group_selections(&app_paths.group_selections());
    let rule_overrides = load_rule_overrides(&app_paths.rule_overrides());
    let config = build_runtime_config(&BuildInput {
        template: settings.to_local_template(),
        profile,
        selected_tag: selected,
        geoip_rule_set_dir: Some(geoip_dir),
        group_selections,
        rule_overrides,
        capture_intent,
    })?;
    write_runtime_config_file(&app_paths.config(), &app_paths.config_bak(), &config)?;
    Ok(())
}

pub fn resolve_binary(resource_dir: Option<&Path>) -> Result<PathBuf, AppError> {
    resolve_singbox_binary(&repo_third_party_singbox(), resource_dir).map_err(AppError::from)
}

/// Start: generate config → core.start.
///
/// System proxy is **not** applied here — the home-page「启动代理服务」button
/// calls [`orchestrate_enable_system_proxy`]. Quitting the app still restores
/// any applied system proxy via [`orchestrate_stop`].
///
/// Automatic core start always uses [`CaptureIntent::Diagnostic`]: the TUN
/// inbound exists in `config.json` only while a TUN capture transition is in
/// flight or active (architecture §24.1).
pub fn orchestrate_start(
    app_paths: &AppPaths,
    settings: &AppSettings,
    core: &mut dyn CoreHandle,
    binary: PathBuf,
    resource_dir: Option<&Path>,
    capture_intent: CaptureIntent,
) -> Result<Option<String>, AppError> {
    generate_config(app_paths, settings, resource_dir, capture_intent)?;

    let core_paths = build_core_paths(app_paths, settings, binary);
    core.start(&core_paths).map_err(AppError::from)?;
    Ok(None)
}

/// Take over the OS system proxy while the core is already Running.
///
/// If a previous apply is still recorded on disk (including after the OS proxy was
/// changed externally), restore first so [`apply_and_record`] can snapshot cleanly.
pub fn orchestrate_enable_system_proxy(
    app_paths: &AppPaths,
    settings: &AppSettings,
    core: &dyn CoreHandle,
    proxy: &dyn SystemProxy,
) -> Result<(), AppError> {
    if core.state().status != CoreStatus::Running {
        return Err(AppError::new(
            ErrorCode::CoreInvalidState,
            "内核未运行，无法启动系统代理",
        ));
    }
    if !proxy.is_available() {
        return Err(AppError::new(
            ErrorCode::ProxyApplyFailed,
            "当前平台不支持系统代理",
        ));
    }
    let endpoints = endpoints_from_settings(settings);
    if is_proxy_live_applied(proxy, &app_paths.proxy_backup(), &endpoints) {
        return Ok(());
    }
    let backup_path = app_paths.proxy_backup();
    if let Ok(record) = ice_proxy_sys::ProxyBackupFile::load(&backup_path) {
        if record.applied || record.pending_apply {
            restore_and_clear_flag(&backup_path, proxy).map_err(AppError::from)?;
        }
    }
    apply_and_record(&backup_path, proxy, &endpoints).map_err(AppError::from)
}

/// Restore the OS system proxy without stopping the core.
pub fn orchestrate_disable_system_proxy(
    app_paths: &AppPaths,
    proxy: &dyn SystemProxy,
) -> Result<(), AppError> {
    restore_and_clear_flag(&app_paths.proxy_backup(), proxy).map_err(AppError::from)?;
    Ok(())
}

/// Stop: restore proxy first (if applied), then kill core.
pub fn orchestrate_stop(
    app_paths: &AppPaths,
    core: &mut dyn CoreHandle,
    proxy: &dyn SystemProxy,
) -> Result<(), AppError> {
    let restore_err = restore_and_clear_flag(&app_paths.proxy_backup(), proxy).err();
    if let Some(ref err) = restore_err {
        tracing::error!(error = %err, "restore system proxy during stop");
    }
    core.stop(&app_paths.pid()).map_err(AppError::from)?;
    if let Some(err) = restore_err {
        return Err(AppError::new(
            ErrorCode::ProxyRestoreFailed,
            format!("内核已停止，但系统代理恢复失败: {err}"),
        ));
    }
    Ok(())
}

/// Restore system proxy after sing-box exits unexpectedly while the app keeps running.
/// Returns a UI warning when restore was needed but failed.
pub fn restore_proxy_after_unexpected_core_exit(
    app_paths: &AppPaths,
    proxy: &dyn SystemProxy,
) -> Option<String> {
    match restore_and_clear_flag(&app_paths.proxy_backup(), proxy) {
        Ok(true) => {
            tracing::info!("restored system proxy after unexpected sing-box exit");
            None
        }
        Ok(false) => None,
        Err(err) => {
            tracing::error!(error = %err, "proxy restore after unexpected core exit");
            Some(format!("sing-box 意外退出后系统代理恢复失败: {err}"))
        }
    }
}

/// Apply subscriptions/settings to disk; if Running, reload (and sync system proxy when needed).
// Argument count is consolidated by the TUN CaptureController restructure (slice T3).
#[allow(clippy::too_many_arguments)]
pub fn orchestrate_apply(
    app_paths: &AppPaths,
    settings: &AppSettings,
    previous_settings: &AppSettings,
    core: &mut dyn CoreHandle,
    proxy: &dyn SystemProxy,
    binary: PathBuf,
    resource_dir: Option<&Path>,
    capture_intent: CaptureIntent,
) -> Result<(), AppError> {
    // generate_config falls back to a direct-only config when no subscription /
    // no usable nodes exist, so Apply always writes a valid config.json.
    generate_config(app_paths, settings, resource_dir, capture_intent)?;

    let status = core.state().status;
    if status != CoreStatus::Running {
        return Ok(());
    }

    let core_paths = build_core_paths(app_paths, settings, binary);
    let previous_endpoints = endpoints_from_settings(previous_settings);
    let new_endpoints = endpoints_from_settings(settings);
    let inbound_changed = previous_endpoints.http_port != new_endpoints.http_port
        || previous_endpoints.http_host != new_endpoints.http_host
        || previous_endpoints.socks_port != new_endpoints.socks_port
        || previous_endpoints.socks_host != new_endpoints.socks_host;

    match core.reload(&core_paths) {
        Ok(ReloadOutcome::HotReloaded | ReloadOutcome::Restarted) => {
            sync_system_proxy_after_reload(
                app_paths,
                proxy,
                previous_settings,
                settings,
                &new_endpoints,
                inbound_changed,
            )
        }
        Err(err) => {
            if core.needs_proxy_restore() {
                let _ = restore_and_clear_flag(&app_paths.proxy_backup(), proxy);
                core.clear_needs_proxy_restore();
            }
            rollback_runtime_config_after_reload_failure(
                app_paths,
                previous_settings,
                resource_dir,
                capture_intent,
            );
            Err(AppError::from(err))
        }
    }
}

/// Reconcile system proxy after a successful hot reload / restart.
/// Only touches the OS proxy when the user already enabled it (on-disk applied).
fn sync_system_proxy_after_reload(
    app_paths: &AppPaths,
    proxy: &dyn SystemProxy,
    _previous_settings: &AppSettings,
    _settings: &AppSettings,
    new_endpoints: &ProxyEndpoints,
    inbound_changed: bool,
) -> Result<(), AppError> {
    if !is_proxy_applied_on_disk(&app_paths.proxy_backup()) {
        return Ok(());
    }
    if !inbound_changed {
        return Ok(());
    }
    if let Err(err) = restore_and_clear_flag(&app_paths.proxy_backup(), proxy) {
        return Err(AppError::new(
            ErrorCode::ProxyRestoreFailed,
            format!("内核已重载，但在同步系统代理前无法恢复旧设置，已中止以免覆盖备份（{err}）"),
        ));
    }
    if let Err(err) = apply_and_record(&app_paths.proxy_backup(), proxy, new_endpoints) {
        if apply_and_record(&app_paths.proxy_backup(), proxy, new_endpoints).is_err() {
            return Err(AppError::new(
                ErrorCode::ProxyApplyFailedCoreReloaded,
                format!(
                    "内核已在新端口 {}:{} 运行，但系统代理未能同步，请检查权限或手动设置系统代理（{}）",
                    new_endpoints.http_host, new_endpoints.http_port, err
                ),
            ));
        }
    }
    Ok(())
}

pub fn current_settings(app_paths: &AppPaths) -> Result<AppSettings, AppError> {
    load_settings(&app_paths.settings())
}

/// True when the on-disk `config.json` (the config the running core was started from)
/// carries the `clash_mode` route rules. Under the pinned sing-box 1.13.19 the runtime
/// `mode-list` is always `[<default_mode>]`, so a `PATCH /configs` to a different mode is
/// silently ignored and mode switching must rebuild + reload (SIGHUP on Unix) so the new
/// `default_mode` is baked in; the `clash_mode` rules make the baked mode take effect at
/// match time. Old-style configs (pre-Slice 4c) strip rules in global/direct mode and have
/// no `clash_mode` rule. This check gates whether the running config already routes by
/// `clash_mode` before attempting the forward-compatible PATCH fast path.
pub fn running_config_supports_clash_mode(app_paths: &AppPaths) -> bool {
    let Ok(raw) = std::fs::read_to_string(app_paths.config()) else {
        return false;
    };
    let Ok(config) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };
    config["route"]["rules"]
        .as_array()
        .is_some_and(|rules| rules.iter().any(|r| r.get("clash_mode").is_some()))
}

/// Switch routing mode. Under the pinned sing-box 1.13.19 the runtime Clash `mode-list`
/// is `[<default_mode>]`, so a `PATCH /configs` to another mode is silently ignored: the
/// PATCH + `GET /configs` verification below always falls through to the rebuild +
/// reload/restart path (SIGHUP on Unix, restart on Windows). The PATCH attempt is kept as
/// a forward-compatible capability gate — if a future core accepts live mode switches it
/// activates automatically — and `running_config_supports_clash_mode` skips even that
/// attempt for pre-Slice 4c configs. While stopped the switch just persists settings (the
/// next apply builds the new `default_mode`). The caller persists `settings.proxy_mode`
/// beforehand; on-disk `config.json` intentionally lags while running (its baked
/// `default_mode` is refreshed on the next apply/restart).
///
/// The rebuild + reload fallback is dispatched through [`orchestrate_apply`]; the TUN
/// controller wires a TUN-aware apply via [`orchestrate_set_proxy_mode_with_apply`]
/// because the elevated core cannot be signalled by the app.
// Argument count is consolidated by the TUN CaptureController restructure (slice T3).
// Currently exercised by the orchestration tests; the shell uses the `_with_apply`
// variant so the fallback can route through the capture controller while TUN is active.
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub fn orchestrate_set_proxy_mode(
    app_paths: &AppPaths,
    settings: &AppSettings,
    previous_settings: &AppSettings,
    core: &mut dyn CoreHandle,
    proxy: &dyn SystemProxy,
    binary: PathBuf,
    resource_dir: Option<&Path>,
    capture_intent: CaptureIntent,
) -> Result<(), AppError> {
    orchestrate_set_proxy_mode_with_apply(
        app_paths,
        settings,
        previous_settings,
        core,
        proxy,
        binary,
        resource_dir,
        capture_intent,
        |app_paths, settings, previous_settings, core, proxy, binary, resource_dir, intent| {
            orchestrate_apply(
                app_paths,
                settings,
                previous_settings,
                core,
                proxy,
                binary,
                resource_dir,
                intent,
            )
        },
    )
}

/// [`orchestrate_set_proxy_mode`] with an injectable rebuild + reload step;
/// the shell passes a TUN-aware apply while TUN capture is active.
#[allow(clippy::too_many_arguments)]
pub fn orchestrate_set_proxy_mode_with_apply(
    app_paths: &AppPaths,
    settings: &AppSettings,
    previous_settings: &AppSettings,
    core: &mut dyn CoreHandle,
    proxy: &dyn SystemProxy,
    binary: PathBuf,
    resource_dir: Option<&Path>,
    capture_intent: CaptureIntent,
    apply: impl FnOnce(
        &AppPaths,
        &AppSettings,
        &AppSettings,
        &mut dyn CoreHandle,
        &dyn SystemProxy,
        PathBuf,
        Option<&Path>,
        CaptureIntent,
    ) -> Result<(), AppError>,
) -> Result<(), AppError> {
    if core.state().status != CoreStatus::Running {
        return Ok(());
    }
    if running_config_supports_clash_mode(app_paths) {
        let endpoints = HealthEndpoints {
            host: settings.clash_api_listen.clone(),
            port: settings.clash_api_port,
        };
        let mode_name = clash_mode_name(settings.proxy_mode);
        match set_mode(&endpoints, mode_name) {
            Ok(()) if get_mode(&endpoints).ok().as_deref() == Some(mode_name) => {
                tracing::info!(mode = %mode_name, "routing mode switched live via Clash API");
                return Ok(());
            }
            Ok(()) => {
                tracing::warn!(
                    mode = %mode_name,
                    "clash PATCH /configs accepted but runtime mode did not change; falling back to rebuild + reload"
                );
            }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "clash PATCH /configs failed; falling back to rebuild + reload"
                );
            }
        }
    }
    apply(
        app_paths,
        settings,
        previous_settings,
        core,
        proxy,
        binary,
        resource_dir,
        capture_intent,
    )
}

/// Prefer restoring the last good on-disk config; fall back to regenerating from settings.
fn rollback_runtime_config_after_reload_failure(
    app_paths: &AppPaths,
    previous_settings: &AppSettings,
    resource_dir: Option<&Path>,
    capture_intent: CaptureIntent,
) {
    match restore_runtime_config_from_bak(&app_paths.config(), &app_paths.config_bak()) {
        Ok(true) => {
            tracing::info!("restored config.json from config.json.bak after reload failure");
        }
        Ok(false) => {
            if let Err(rollback) =
                generate_config(app_paths, previous_settings, resource_dir, capture_intent)
            {
                tracing::error!(
                    error = %rollback,
                    "failed to regenerate config after reload failure (no .bak present)"
                );
            }
        }
        Err(restore_err) => {
            tracing::error!(
                error = %restore_err,
                "failed to restore config.json.bak after reload failure"
            );
            if let Err(rollback) =
                generate_config(app_paths, previous_settings, resource_dir, capture_intent)
            {
                tracing::error!(
                    error = %rollback,
                    "failed to regenerate config after .bak restore error"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ice_config::NormalizedOutbound as NO;
    use ice_config::ProxyMode;
    use ice_core::{
        CoreController, ImmediateHealthProbe, MockClashApi, MockReloadMode, MockReloader,
        MockSpawner, SequenceHealthProbe,
    };
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    use ice_proxy_sys::create_system_proxy;
    use ice_proxy_sys::{NoopSystemProxy, ProxyBackup, ProxyBackupFile, ProxySysError};
    use ice_subscription::{
        write_subscription_success, SubscriptionFormat, SubscriptionMeta, SubscriptionPaths,
    };
    use std::cell::Cell;
    use std::fs;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    #[derive(Default)]
    struct TrackProxy {
        apply_calls: Cell<usize>,
        restore_calls: Cell<usize>,
        fail_apply: Cell<bool>,
        fail_restore: Cell<bool>,
        order: Arc<std::sync::Mutex<Vec<&'static str>>>,
    }

    impl SystemProxy for TrackProxy {
        fn backup(&self) -> Result<ProxyBackup, ProxySysError> {
            Ok(ProxyBackup::default())
        }

        fn apply(&self, _endpoints: &ProxyEndpoints) -> Result<(), ProxySysError> {
            self.order.lock().unwrap().push("apply");
            self.apply_calls.set(self.apply_calls.get() + 1);
            if self.fail_apply.get() {
                return Err(ProxySysError::ApplyFailed("mock apply fail".into()));
            }
            Ok(())
        }

        fn restore(&self, _backup: &ProxyBackup) -> Result<(), ProxySysError> {
            self.order.lock().unwrap().push("restore");
            self.restore_calls.set(self.restore_calls.get() + 1);
            if self.fail_restore.get() {
                return Err(ProxySysError::RestoreFailed("mock restore fail".into()));
            }
            Ok(())
        }
    }

    fn temp_app(label: &str) -> AppPaths {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-orch-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = AppPaths::new(&dir);
        paths.ensure_dirs().unwrap();
        paths
    }

    fn marker_bin(paths: &AppPaths) -> PathBuf {
        let bin = paths.root().join("sing-box");
        fs::write(&bin, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = fs::metadata(&bin).unwrap().permissions();
            p.set_mode(0o755);
            fs::set_permissions(&bin, p).unwrap();
        }
        bin
    }

    fn seed_one_node(paths: &AppPaths) {
        let sub = SubscriptionPaths::from_app(paths);
        let id = Uuid::new_v4();
        let meta = SubscriptionMeta {
            id,
            name: "t".into(),
            url: "https://example.com/s".into(),
            active: true,
            format: SubscriptionFormat::SingBox,
            node_count: 1,
            group_count: 0,
            rule_count: 0,
            has_dns: false,
            parse_warnings: vec![],
            last_updated: None,
            last_error: None,
            etag: None,
            last_modified: None,
        };
        let nodes = vec![NO {
            tag: "n1".into(),
            outbound: serde_json::json!({
                "type": "socks",
                "tag": "n1",
                "server": "127.0.0.1",
                "server_port": 1080
            }),
        }];
        write_subscription_success(
            &sub,
            &meta,
            "{}",
            &ice_config::NormalizedProfile::from_nodes_only(nodes),
        )
        .unwrap();
    }

    fn mock_core_ok() -> CoreController<MockSpawner, ImmediateHealthProbe> {
        CoreController::with_deps(
            MockSpawner::with_start_pid(5000),
            ImmediateHealthProbe,
            Box::new(MockReloader::new(MockReloadMode::Ok)),
            Duration::from_millis(50),
            Duration::from_millis(50),
        )
    }

    fn mock_core_with_reloader(
        reloader: MockReloader,
    ) -> CoreController<MockSpawner, ImmediateHealthProbe> {
        CoreController::with_deps(
            MockSpawner::with_start_pid(5000),
            ImmediateHealthProbe,
            Box::new(reloader),
            Duration::from_millis(50),
            Duration::from_millis(50),
        )
    }

    fn mock_core_fail_health() -> CoreController<MockSpawner, ice_core::FailingHealthProbe> {
        CoreController::with_deps(
            MockSpawner::with_start_pid(6000),
            ice_core::FailingHealthProbe,
            Box::new(MockReloader::default()),
            Duration::from_millis(20),
            Duration::from_millis(20),
        )
    }

    fn mock_core_reload_restart_fail() -> CoreController<MockSpawner, SequenceHealthProbe> {
        CoreController::with_deps(
            MockSpawner::with_start_pid(8000),
            SequenceHealthProbe::new(vec![
                Ok(()),
                Err(ice_core::CoreError::HealthcheckFailed(
                    "restart probe fail".into(),
                )),
            ]),
            Box::new(MockReloader::new(MockReloadMode::Http5xx)),
            Duration::from_millis(50),
            Duration::from_millis(50),
        )
    }

    #[test]
    fn g7_1_start_without_subscription_runs_direct_only() {
        let paths = temp_app("empty");
        let settings = AppSettings::default();
        let mut core = mock_core_ok();
        let proxy = TrackProxy::default();
        let bin = marker_bin(&paths);

        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("direct-only");
        assert_eq!(core.state().status, CoreStatus::Running);
        assert_eq!(
            proxy.apply_calls.get(),
            0,
            "start must not take over system proxy; home button does"
        );
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(paths.config()).unwrap()).unwrap();
        let tags: Vec<&str> = config["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|o| o["tag"].as_str())
            .collect();
        assert_eq!(
            tags,
            ["direct", "block"],
            "no-subscription config is direct-only"
        );
        assert_eq!(config["route"]["final"], "direct");
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_2_enable_system_proxy_after_start() {
        let paths = temp_app("start-ok");
        seed_one_node(&paths);
        let settings = AppSettings::default();
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut core = mock_core_ok();
        let proxy = TrackProxy {
            order: order.clone(),
            ..TrackProxy::default()
        };
        let bin = marker_bin(&paths);

        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        assert_eq!(core.state().status, CoreStatus::Running);
        assert_eq!(proxy.apply_calls.get(), 0);
        orchestrate_enable_system_proxy(&paths, &settings, &core, &proxy).unwrap();
        assert_eq!(proxy.apply_calls.get(), 1);
        let backup = ProxyBackupFile::load(&paths.proxy_backup()).unwrap();
        assert!(backup.applied);
        assert_eq!(order.lock().unwrap().as_slice(), &["apply"]);
        assert!(paths.config().is_file());
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_3_healthcheck_fail_no_applied() {
        let paths = temp_app("hc-fail");
        seed_one_node(&paths);
        let settings = AppSettings::default();
        let mut core = mock_core_fail_health();
        let proxy = TrackProxy::default();
        let bin = marker_bin(&paths);

        let err = orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .expect_err("hc");
        assert_eq!(err.code, "core.healthcheck_failed");
        assert_eq!(proxy.apply_calls.get(), 0);
        if paths.proxy_backup().exists() {
            let b = ProxyBackupFile::load(&paths.proxy_backup()).unwrap();
            assert!(!b.applied);
        }
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_4_enable_system_proxy_fail_keeps_core() {
        let paths = temp_app("apply-fail");
        seed_one_node(&paths);
        let settings = AppSettings::default();
        let mut core = mock_core_ok();
        let proxy = TrackProxy {
            fail_apply: Cell::new(true),
            ..TrackProxy::default()
        };
        let bin = marker_bin(&paths);

        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        assert_eq!(core.state().status, CoreStatus::Running);
        let err = orchestrate_enable_system_proxy(&paths, &settings, &core, &proxy)
            .expect_err("apply fail");
        assert_eq!(err.code, "proxy.apply_failed");
        assert_eq!(core.state().status, CoreStatus::Running);
        if paths.proxy_backup().exists() {
            let b = ProxyBackupFile::load(&paths.proxy_backup()).unwrap();
            assert!(!b.applied);
            assert!(!b.pending_apply);
        }
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn enable_system_proxy_with_noop_rejected_as_unavailable() {
        let paths = temp_app("noop-auto");
        seed_one_node(&paths);
        let settings = AppSettings::default();
        let mut core = mock_core_ok();
        let proxy = NoopSystemProxy;
        let bin = marker_bin(&paths);

        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        assert_eq!(core.state().status, CoreStatus::Running);
        let err = orchestrate_enable_system_proxy(&paths, &settings, &core, &proxy)
            .expect_err("noop unavailable");
        assert_eq!(err.code, "proxy.apply_failed");
        assert!(err.message.contains("不支持"));
        assert!(!paths.proxy_backup().exists());
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn default_start_never_applies_system_proxy() {
        let paths = temp_app("platform-default");
        let settings = AppSettings::default();
        assert!(
            !settings.auto_set_system_proxy,
            "legacy auto_set flag defaults off; home button controls system proxy"
        );
        let mut core = mock_core_ok();
        let bin = marker_bin(&paths);

        let warning = orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .expect("start core only");
        assert_eq!(core.state().status, CoreStatus::Running);
        assert_eq!(warning, None);
        assert!(!ice_proxy_sys::is_proxy_applied_on_disk(
            &paths.proxy_backup()
        ));
        let _ = fs::remove_dir_all(paths.root());
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn real_backend_can_backup_without_mutating() {
        create_system_proxy()
            .backup()
            .expect("read-only backup must work in CI without mutating the OS proxy");
    }

    #[test]
    fn g7_5_stop_restores_then_kills_idempotent() {
        let paths = temp_app("stop");
        seed_one_node(&paths);
        let settings = AppSettings::default();
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut core = mock_core_ok();
        let proxy = TrackProxy {
            order: order.clone(),
            ..TrackProxy::default()
        };
        let bin = marker_bin(&paths);
        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin.clone(),
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        orchestrate_enable_system_proxy(&paths, &settings, &core, &proxy).unwrap();

        order.lock().unwrap().clear();
        orchestrate_stop(&paths, &mut core, &proxy).unwrap();
        assert_eq!(core.state().status, CoreStatus::Stopped);
        assert_eq!(order.lock().unwrap().as_slice(), &["restore"]);

        orchestrate_stop(&paths, &mut core, &proxy).unwrap();
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_5b_stop_restore_failure_still_stops_core() {
        let paths = temp_app("stop-restore-fail");
        seed_one_node(&paths);
        let settings = AppSettings::default();
        let mut core = mock_core_ok();
        let start_proxy = TrackProxy::default();
        let stop_proxy = TrackProxy {
            fail_restore: Cell::new(true),
            ..TrackProxy::default()
        };
        let bin = marker_bin(&paths);
        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        orchestrate_enable_system_proxy(&paths, &settings, &core, &start_proxy).unwrap();
        assert_eq!(core.state().status, CoreStatus::Running);

        let err = orchestrate_stop(&paths, &mut core, &stop_proxy).expect_err("restore fail");
        assert_eq!(err.code, "proxy.restore_failed");
        assert_eq!(core.state().status, CoreStatus::Stopped);
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_6_apply_while_stopped_only_writes_config() {
        let paths = temp_app("apply-stopped");
        seed_one_node(&paths);
        let settings = AppSettings::default();
        let mut core = mock_core_ok();
        let proxy = TrackProxy::default();
        let bin = marker_bin(&paths);

        orchestrate_apply(
            &paths,
            &settings,
            &settings,
            &mut core,
            &proxy,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        assert!(paths.config().is_file());
        assert_eq!(core.state().status, CoreStatus::Stopped);
        assert_eq!(proxy.apply_calls.get(), 0);
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_7_apply_running_same_inbound_reloads_no_proxy() {
        let paths = temp_app("apply-reload");
        seed_one_node(&paths);
        let settings = AppSettings::default();
        let mut core = mock_core_ok();
        let proxy = TrackProxy::default();
        let bin = marker_bin(&paths);
        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin.clone(),
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        orchestrate_enable_system_proxy(&paths, &settings, &core, &proxy).unwrap();
        let apply_before = proxy.apply_calls.get();
        let restore_before = proxy.restore_calls.get();

        orchestrate_apply(
            &paths,
            &settings,
            &settings,
            &mut core,
            &proxy,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        assert_eq!(core.state().status, CoreStatus::Running);
        assert_eq!(proxy.apply_calls.get(), apply_before);
        assert_eq!(proxy.restore_calls.get(), restore_before);
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_8_apply_running_port_change_restore_apply() {
        let paths = temp_app("apply-port");
        seed_one_node(&paths);
        let settings = AppSettings::default();
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut core = mock_core_ok();
        let proxy = TrackProxy {
            order: order.clone(),
            ..TrackProxy::default()
        };
        let bin = marker_bin(&paths);
        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin.clone(),
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        orchestrate_enable_system_proxy(&paths, &settings, &core, &proxy).unwrap();

        let new_settings = AppSettings {
            mixed_port: 17900,
            ..settings.clone()
        };
        order.lock().unwrap().clear();
        orchestrate_apply(
            &paths,
            &new_settings,
            &settings,
            &mut core,
            &proxy,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        assert!(order.lock().unwrap().contains(&"restore"));
        assert!(order.lock().unwrap().contains(&"apply"));
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_10_disable_system_proxy_keeps_core_running() {
        let paths = temp_app("disable-proxy");
        seed_one_node(&paths);
        let settings = AppSettings::default();
        let mut core = mock_core_ok();
        let proxy = TrackProxy::default();
        let bin = marker_bin(&paths);
        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        orchestrate_enable_system_proxy(&paths, &settings, &core, &proxy).unwrap();
        assert!(
            ProxyBackupFile::load(&paths.proxy_backup())
                .unwrap()
                .applied
        );

        orchestrate_disable_system_proxy(&paths, &proxy).unwrap();
        assert_eq!(core.state().status, CoreStatus::Running);
        assert_eq!(proxy.restore_calls.get(), 1);
        assert!(
            !ProxyBackupFile::load(&paths.proxy_backup())
                .unwrap()
                .applied
        );
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_11_enable_system_proxy_while_running() {
        let paths = temp_app("enable-proxy");
        seed_one_node(&paths);
        let settings = AppSettings::default();
        let mut core = mock_core_ok();
        let proxy = TrackProxy::default();
        let bin = marker_bin(&paths);
        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        assert_eq!(proxy.apply_calls.get(), 0);

        orchestrate_enable_system_proxy(&paths, &settings, &core, &proxy).unwrap();
        assert_eq!(proxy.apply_calls.get(), 1);
        assert!(
            ProxyBackupFile::load(&paths.proxy_backup())
                .unwrap()
                .applied
        );
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_11b_enable_restores_then_reapplies_when_disk_still_marked() {
        let paths = temp_app("enable-reapply");
        seed_one_node(&paths);
        let settings = AppSettings::default();
        let mut core = mock_core_ok();
        let proxy = TrackProxy::default();
        let bin = marker_bin(&paths);
        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        orchestrate_enable_system_proxy(&paths, &settings, &core, &proxy).unwrap();
        assert_eq!(proxy.apply_calls.get(), 1);

        // Disk still applied but live check fails (TrackProxy backup never matches) —
        // re-enable must restore first, then apply again.
        orchestrate_enable_system_proxy(&paths, &settings, &core, &proxy).unwrap();
        assert_eq!(proxy.restore_calls.get(), 1);
        assert_eq!(proxy.apply_calls.get(), 2);
        assert!(
            ProxyBackupFile::load(&paths.proxy_backup())
                .unwrap()
                .applied
        );
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_9_subscription_mutations_trigger_apply_path() {
        // Exercise generate_config after seed — add/update/set_active/remove call Apply in commands;
        // here we assert generate_config succeeds after each store mutation.
        let paths = temp_app("sub-apply");
        let sub = SubscriptionPaths::from_app(&paths);
        let id = Uuid::new_v4();
        let meta = SubscriptionMeta {
            id,
            name: "t".into(),
            url: "https://example.com/s".into(),
            active: true,
            format: SubscriptionFormat::SingBox,
            node_count: 1,
            group_count: 0,
            rule_count: 0,
            has_dns: false,
            parse_warnings: vec![],
            last_updated: None,
            last_error: None,
            etag: None,
            last_modified: None,
        };
        let nodes = vec![NO {
            tag: "n1".into(),
            outbound: serde_json::json!({"type":"socks","tag":"n1","server":"1.1.1.1","server_port":1}),
        }];
        write_subscription_success(
            &sub,
            &meta,
            "{}",
            &ice_config::NormalizedProfile::from_nodes_only(nodes),
        )
        .unwrap();
        generate_config(
            &paths,
            &AppSettings::default(),
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();

        fn direct_only_tags(paths: &AppPaths) -> Vec<String> {
            let config: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(paths.config()).unwrap()).unwrap();
            config["outbounds"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|o| o["tag"].as_str().map(String::from))
                .collect()
        }

        ice_subscription::set_enabled(&sub, id, false).unwrap();
        generate_config(
            &paths,
            &AppSettings::default(),
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        assert_eq!(
            direct_only_tags(&paths),
            ["direct", "block"],
            "no active subscription falls back to direct-only config"
        );

        ice_subscription::set_enabled(&sub, id, true).unwrap();
        generate_config(
            &paths,
            &AppSettings::default(),
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        assert_ne!(direct_only_tags(&paths), ["direct", "block"]);

        ice_subscription::remove_subscription(&sub, id).unwrap();
        generate_config(
            &paths,
            &AppSettings::default(),
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        assert_eq!(
            direct_only_tags(&paths),
            ["direct", "block"],
            "all subscriptions removed falls back to direct-only config"
        );
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_12_proxy_apply_failed_after_reload_returns_core_reloaded_code() {
        let paths = temp_app("proxy-reload-fail");
        seed_one_node(&paths);
        let settings = AppSettings::default();
        let mut core = mock_core_ok();
        let good = TrackProxy::default();
        let bin = marker_bin(&paths);
        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin.clone(),
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        orchestrate_enable_system_proxy(&paths, &settings, &core, &good).unwrap();

        let proxy = TrackProxy {
            fail_apply: Cell::new(true),
            ..TrackProxy::default()
        };
        let new_settings = AppSettings {
            mixed_port: 17900,
            ..settings.clone()
        };
        let err = orchestrate_apply(
            &paths,
            &new_settings,
            &settings,
            &mut core,
            &proxy,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .expect_err("proxy apply after reload");
        assert_eq!(err.code, "proxy.apply_failed_core_reloaded");
        assert!(err.message.contains("17900"));
        assert_eq!(core.state().status, CoreStatus::Running);
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_13_port_change_aborts_when_pre_restore_fails() {
        let paths = temp_app("restore-before-apply");
        seed_one_node(&paths);
        let settings = AppSettings::default();
        let start_proxy = TrackProxy::default();
        let mut core = mock_core_ok();
        let bin = marker_bin(&paths);
        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin.clone(),
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        orchestrate_enable_system_proxy(&paths, &settings, &core, &start_proxy).unwrap();
        assert_eq!(start_proxy.apply_calls.get(), 1);

        let sync_proxy = TrackProxy {
            fail_restore: Cell::new(true),
            ..TrackProxy::default()
        };
        let new_settings = AppSettings {
            mixed_port: 17900,
            ..settings.clone()
        };
        let err = orchestrate_apply(
            &paths,
            &new_settings,
            &settings,
            &mut core,
            &sync_proxy,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .expect_err("restore before re-apply");
        assert_eq!(err.code, "proxy.restore_failed");
        assert_eq!(
            sync_proxy.apply_calls.get(),
            0,
            "must not apply after restore fail"
        );
        assert_eq!(core.state().status, CoreStatus::Running);
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_14_reload_failure_restores_config_from_bak() {
        let paths = temp_app("config-bak-rollback");
        seed_one_node(&paths);
        let settings = AppSettings::default();
        let mut core = mock_core_reload_restart_fail();
        let proxy = TrackProxy::default();
        let bin = marker_bin(&paths);
        orchestrate_start(
            &paths,
            &settings,
            &mut core,
            bin.clone(),
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();

        let before: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(paths.config()).unwrap()).unwrap();
        assert_eq!(before["inbounds"][0]["listen_port"], 17890);

        let new_settings = AppSettings {
            mixed_port: 17900,
            ..settings.clone()
        };
        let err = orchestrate_apply(
            &paths,
            &new_settings,
            &settings,
            &mut core,
            &proxy,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .expect_err("reload restart fail");
        assert_eq!(err.code, "core.healthcheck_failed");

        let after: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(paths.config()).unwrap()).unwrap();
        assert_eq!(
            after["inbounds"][0]["listen_port"], 17890,
            "config.json should roll back to .bak after reload failure"
        );
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn generate_config_reconciles_stale_selected_tag_on_disk() {
        let paths = temp_app("reconcile-tag");
        seed_one_node(&paths);
        let settings_path = paths.settings();
        let stale = AppSettings {
            selected_tag: Some("removed-node".into()),
            ..AppSettings::default()
        };
        ice_config::save_settings(&settings_path, &stale).unwrap();

        generate_config(&paths, &stale, None, CaptureIntent::Diagnostic).unwrap();

        let on_disk = ice_config::load_settings(&settings_path).unwrap();
        assert_eq!(on_disk.selected_tag.as_deref(), Some("n1"));
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn generate_config_groups_only_profile_falls_back_to_direct_only() {
        let paths = temp_app("groups-only");
        let sub = SubscriptionPaths::from_app(&paths);
        let id = Uuid::new_v4();
        let meta = SubscriptionMeta {
            id,
            name: "groups-only".into(),
            url: "https://example.com/s".into(),
            active: true,
            format: SubscriptionFormat::Clash,
            node_count: 0,
            group_count: 1,
            rule_count: 0,
            has_dns: false,
            parse_warnings: vec![],
            last_updated: None,
            last_error: None,
            etag: None,
            last_modified: None,
        };
        let profile = ice_config::NormalizedProfile {
            nodes: vec![],
            groups: vec![NO {
                tag: "Proxies".into(),
                outbound: serde_json::json!({
                    "type": "selector",
                    "tag": "Proxies",
                    "outbounds": ["direct"],
                }),
            }],
            route: Default::default(),
            dns: None,
            default_outbound: Some("Proxies".into()),
            parse_stats: Default::default(),
        };
        write_subscription_success(&sub, &meta, "{}", &profile).unwrap();

        generate_config(
            &paths,
            &AppSettings::default(),
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(paths.config()).unwrap()).unwrap();
        let tags: Vec<String> = config["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|o| o["tag"].as_str().map(String::from))
            .collect();
        assert_eq!(
            tags,
            ["direct", "block"],
            "groups-only profile must fall back to direct-only config"
        );
        let _ = fs::remove_dir_all(paths.root());
    }

    // --- Slice 4c: mode switch (capability-gate fast path + rebuild/reload fallback) ---

    fn start_running(
        paths: &AppPaths,
        reloader: &MockReloader,
    ) -> CoreController<MockSpawner, ImmediateHealthProbe> {
        let settings = AppSettings::default();
        let mut core = mock_core_with_reloader(reloader.clone());
        let bin = marker_bin(paths);
        orchestrate_start(
            paths,
            &settings,
            &mut core,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();
        assert_eq!(core.state().status, CoreStatus::Running);
        core
    }

    #[test]
    fn g7_15_set_mode_running_patches_and_does_not_reload() {
        // Exercises the forward-compatible PATCH capability gate in isolation: this mock
        // applies the PATCH (a real 1.13.19 core never does — its runtime mode-list is
        // `[<default_mode>]` — so in production this branch is never taken and every mode
        // switch reloads). The gate is kept so a future core that accepts live switches
        // activates without code changes.
        let paths = temp_app("mode-patch");
        seed_one_node(&paths);
        let reloader = MockReloader::default();
        let mut core = start_running(&paths, &reloader);
        assert!(
            running_config_supports_clash_mode(&paths),
            "generated config must carry clash_mode rules"
        );

        let server = MockClashApi::start(204, "Rule");
        let previous = AppSettings::default();
        let settings = AppSettings {
            clash_api_port: server.addr.port(),
            proxy_mode: ProxyMode::Global,
            ..previous.clone()
        };
        let proxy = TrackProxy::default();
        let bin = marker_bin(&paths);
        orchestrate_set_proxy_mode(
            &paths,
            &settings,
            &previous,
            &mut core,
            &proxy,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();

        assert_eq!(
            reloader.call_count(),
            0,
            "capability-gate fast path must not reload (mock applies the PATCH)"
        );
        std::thread::sleep(Duration::from_millis(100));
        let reqs = server.requests.lock().unwrap();
        assert_eq!(reqs.len(), 2, "expected PATCH then verification GET");
        assert_eq!(reqs[0].method, "PATCH");
        assert_eq!(reqs[0].path, "/configs");
        let body: serde_json::Value = serde_json::from_str(&reqs[0].body).unwrap();
        assert_eq!(body["mode"], "Global");
        assert_eq!(reqs[1].method, "GET");
        assert_eq!(reqs[1].path, "/configs");
        assert_eq!(
            proxy.apply_calls.get(),
            0,
            "no system proxy churn on mode switch"
        );
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_16_set_mode_running_patch_failure_falls_back_to_reload() {
        let paths = temp_app("mode-patch-fail");
        seed_one_node(&paths);
        let reloader = MockReloader::default();
        let mut core = start_running(&paths, &reloader);

        let server = MockClashApi::start(400, "Rule");
        let previous = AppSettings::default();
        let settings = AppSettings {
            clash_api_port: server.addr.port(),
            proxy_mode: ProxyMode::Direct,
            ..previous.clone()
        };
        let proxy = TrackProxy::default();
        let bin = marker_bin(&paths);
        orchestrate_set_proxy_mode(
            &paths,
            &settings,
            &previous,
            &mut core,
            &proxy,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();

        assert_eq!(
            reloader.call_count(),
            1,
            "PATCH failure must fall back to the rebuild + reload path"
        );
        assert_eq!(core.state().status, CoreStatus::Running);
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_16b_set_mode_running_silently_ignored_patch_falls_back_to_reload() {
        let paths = temp_app("mode-patch-ignored");
        seed_one_node(&paths);
        let reloader = MockReloader::default();
        let mut core = start_running(&paths, &reloader);

        // 2xx PATCH that the core silently ignores (mock never applies the mode).
        let server = MockClashApi::start_with_ignored_patch(204, "Rule");
        let previous = AppSettings::default();
        let settings = AppSettings {
            clash_api_port: server.addr.port(),
            proxy_mode: ProxyMode::Global,
            ..previous.clone()
        };
        let proxy = TrackProxy::default();
        let bin = marker_bin(&paths);
        orchestrate_set_proxy_mode(
            &paths,
            &settings,
            &previous,
            &mut core,
            &proxy,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();

        assert_eq!(
            reloader.call_count(),
            1,
            "a 2xx PATCH that does not change the mode must fall back to the rebuild + reload path"
        );
        assert_eq!(core.state().status, CoreStatus::Running);
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_17_set_mode_running_old_style_config_reloads_once() {
        let paths = temp_app("mode-old-config");
        seed_one_node(&paths);
        let reloader = MockReloader::default();
        let mut core = start_running(&paths, &reloader);

        // Simulate a pre-Slice 4c config: rules present but no clash_mode rule.
        ice_config::write_json_atomic(
            &paths.config(),
            &serde_json::json!({
                "route": {
                    "final": "proxy",
                    "rules": [ { "domain_suffix": ["keep.com"], "outbound": "direct" } ]
                }
            }),
        )
        .unwrap();
        assert!(!running_config_supports_clash_mode(&paths));

        let previous = AppSettings::default();
        let settings = AppSettings {
            proxy_mode: ProxyMode::Global,
            ..previous.clone()
        };
        let proxy = TrackProxy::default();
        let bin = marker_bin(&paths);
        orchestrate_set_proxy_mode(
            &paths,
            &settings,
            &previous,
            &mut core,
            &proxy,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();

        assert_eq!(
            reloader.call_count(),
            1,
            "first switch on an old-style config must rebuild + reload"
        );
        assert!(
            running_config_supports_clash_mode(&paths),
            "rebuild must regenerate a clash_mode config"
        );
        assert_eq!(core.state().status, CoreStatus::Running);
        let _ = fs::remove_dir_all(paths.root());
    }

    #[test]
    fn g7_18_set_mode_stopped_persists_without_patch_or_reload() {
        let paths = temp_app("mode-stopped");
        seed_one_node(&paths);
        let reloader = MockReloader::default();
        let mut core = mock_core_with_reloader(reloader.clone());
        assert_eq!(core.state().status, CoreStatus::Stopped);

        let previous = AppSettings::default();
        let settings = AppSettings {
            proxy_mode: ProxyMode::Global,
            ..previous.clone()
        };
        let proxy = TrackProxy::default();
        let bin = marker_bin(&paths);
        orchestrate_set_proxy_mode(
            &paths,
            &settings,
            &previous,
            &mut core,
            &proxy,
            bin,
            None,
            CaptureIntent::Diagnostic,
        )
        .unwrap();

        assert_eq!(reloader.call_count(), 0);
        assert_eq!(core.state().status, CoreStatus::Stopped);
        let _ = fs::remove_dir_all(paths.root());
    }
}
