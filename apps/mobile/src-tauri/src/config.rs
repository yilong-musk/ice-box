// SPDX-License-Identifier: GPL-3.0-or-later

//! Write the config the `:tunnel` process reads. The host never starts libbox.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ice_app::build_mobile_config;
use ice_config::{
    ensure_clash_api_secret, invalidate_geoip_code_cache, load_group_selections,
    load_rule_overrides, load_settings, write_runtime_config_file, AppError, AppPaths, BuildInput,
    CaptureIntent, HostPlatform, LocalTemplate, NormalizedProfile,
};
use ice_subscription::{
    load_active_profile_with_default_rules, load_index, resolve_selected_tag, SubscriptionError,
    SubscriptionPaths,
};

pub const PLATFORM: HostPlatform = HostPlatform::Android;

/// Generate `config.json` under the shared root.
///
/// `package_name` is the running application id (`com.yilongmusk.icebox` or
/// `com.yilongmusk.icebox.debug`). An empty profile becomes a direct-only
/// tunnel so the VPN can still come up.
pub fn write_config(paths: &AppPaths, package_name: Option<&str>) -> Result<(), AppError> {
    ensure_geoip_rule_sets(&paths.geoip_dir());
    // The directory listing cache is process-wide. Rescan after the copy so a
    // config written before the assets landed cannot keep dropping GEOIP rules.
    invalidate_geoip_code_cache();
    let settings = load_settings(&paths.settings())?;
    let mut template = LocalTemplate::from(&settings);
    template.clash_api_secret = ensure_clash_api_secret(&paths.clash_api_secret())?;
    let profile = active_profile(paths, settings.auto_default_rules)?;
    let selected = resolve_selected_tag(settings.selected_tag.as_deref(), &profile);
    let package = package_name.map(str::trim).filter(|name| !name.is_empty());
    let input = BuildInput {
        template,
        profile,
        selected_tag: selected,
        geoip_rule_set_dir: Some(paths.geoip_dir()),
        rule_set_cache_dir: Some(paths.rule_set_cache_dir()),
        group_selections: load_group_selections(&paths.group_selections()),
        rule_overrides: load_rule_overrides(&paths.rule_overrides()),
        capture_intent: CaptureIntent::Tun,
        tun_exclude_package: package.map(str::to_string),
        shared_root: Some(paths.shared_root().to_path_buf()),
        platform: PLATFORM,
    };
    let config = build_mobile_config(&input)?;
    write_runtime_config_file(&paths.config(), &paths.config_bak(), &config)?;
    Ok(())
}

/// The active profile plus its route and custom rules, for rule-set queries.
fn profile_and_rules(
    paths: &AppPaths,
) -> Result<(Arc<NormalizedProfile>, Vec<serde_json::Value>), AppError> {
    let settings = load_settings(&paths.settings())?;
    let profile = active_profile(paths, settings.auto_default_rules)?;
    let mut rules = profile.route.rules.clone();
    rules.extend(load_rule_overrides(&paths.rule_overrides()).custom);
    Ok((profile, rules))
}

/// URLs of the remote rule-sets the active profile and custom rules need.
pub fn required_rule_set_urls(paths: &AppPaths) -> Result<Vec<String>, AppError> {
    let (profile, rules) = profile_and_rules(paths)?;
    Ok(ice_config::required_rule_set_urls(
        &profile.route.rule_sets,
        &rules,
        profile.dns.as_ref(),
    ))
}

/// Where each rule-set the active profile needs comes from.
pub fn rule_set_status(paths: &AppPaths) -> Result<Vec<ice_subscription::RuleSetStatus>, AppError> {
    let (profile, rules) = profile_and_rules(paths)?;
    Ok(ice_subscription::rule_set_statuses(
        &profile.route.rule_sets,
        &rules,
        profile.dns.as_ref(),
        Some(&paths.geoip_dir()),
        &paths.rule_set_cache_dir(),
    ))
}

pub fn active_profile(
    paths: &AppPaths,
    auto_default_rules: bool,
) -> Result<Arc<NormalizedProfile>, AppError> {
    let sub_paths = SubscriptionPaths::from_app(paths);
    let index = load_index(&sub_paths)?;
    match load_active_profile_with_default_rules(&sub_paths, &index, auto_default_rules, PLATFORM) {
        Ok(profile) => Ok(profile),
        Err(SubscriptionError::NoActiveSubscription) => {
            Ok(Arc::new(NormalizedProfile::from_nodes_only(vec![])))
        }
        Err(err) => Err(err.into()),
    }
}

/// `geoip-{code}.srs` files the generator turns into local rule-sets.
///
/// Android extracts the APK assets into this directory before the runtime
/// starts. A repo checkout that has run `scripts/fetch-geoip.sh` fills any
/// name that is not there yet, which is what host-side tests use. Existing
/// files stay; the asset extractor decides when to replace them.
fn ensure_geoip_rule_sets(target: &Path) {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../third_party/bundled-rule-sets/rule-set");
    if let Err(err) = install_geoip_rule_sets(target, &source) {
        log::warn!(
            "geoip rule-sets were not copied from {}: {err}",
            source.display()
        );
    }
}

pub(crate) fn install_geoip_rule_sets(target: &Path, source: &Path) -> std::io::Result<usize> {
    if !source.is_dir() {
        return Ok(0);
    }
    std::fs::create_dir_all(target)?;
    let mut copied = 0usize;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if !name.to_string_lossy().ends_with(".srs") {
            continue;
        }
        let dest = target.join(&name);
        if dest.is_file() {
            continue;
        }
        std::fs::copy(entry.path(), &dest)?;
        copied += 1;
    }
    Ok(copied)
}

/// The `:tunnel` process keeps `sing-box.log` at or under this size
/// (`CORE_LOG_MAX_BYTES` in TunnelFiles.kt). An export never reads more.
pub const CORE_LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;

/// Lock file the `:tunnel` process holds while it compacts the log
/// (`CORE_LOG_LOCK_FILE` in TunnelFiles.kt). It sits next to the log.
const CORE_LOG_LOCK_FILE: &str = ".sing-box-log.lock";

/// Hold the cross-process log lock until dropped. Java file locks are
/// `fcntl` record locks on Android, which `flock` does not see, so this uses
/// `fcntl` too. The lock is released when the file is closed.
#[cfg(unix)]
fn lock_core_log(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::fd::AsRawFd;

    let lock_path = path.with_file_name(CORE_LOG_LOCK_FILE);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    // SAFETY: an all-zero `flock` is valid; the fields that matter are set
    // below (start 0 and len 0 lock the whole file).
    let mut lock: libc::flock = unsafe { std::mem::zeroed() };
    lock.l_type = libc::F_WRLCK as _;
    lock.l_whence = libc::SEEK_SET as _;
    loop {
        // SAFETY: `file` is open for writing and `lock` outlives the call.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLKW, &lock) } == 0 {
            return Ok(file);
        }
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

#[cfg(not(unix))]
fn lock_core_log(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_file_name(CORE_LOG_LOCK_FILE))
}

/// The exported core log: a header naming the app version, then the newest
/// `max_bytes` of the log starting at a line boundary. A missing log exports
/// just the header, so the user still gets a file that says the log is empty.
///
/// The read holds the lock the `:tunnel` process takes to compact the log,
/// so it never sees a half-moved file.
pub fn core_log_export(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut text = format!(
        "# ice-box {} Android core log (sing-box warnings and errors)\n",
        env!("CARGO_PKG_VERSION")
    );
    let _lock = match lock_core_log(path) {
        Ok(lock) => lock,
        // No shared dir yet means no log yet either.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            text.push_str("# no log yet\n");
            return Ok(text);
        }
        Err(err) => return Err(err),
    };
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            text.push_str("# no log yet\n");
            return Ok(text);
        }
        Err(err) => return Err(err),
    };
    let len = file.metadata()?.len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity((len - start) as usize);
    // libbox may append while we read; stop at the length we sized for.
    file.take(len - start).read_to_end(&mut bytes)?;
    if start > 0 {
        // The tail starts mid-line; drop that partial first line. With no
        // newline at all the tail is one over-long line: keep it, marked.
        match bytes.iter().position(|&b| b == b'\n') {
            Some(newline) => {
                bytes.drain(..=newline);
            }
            None => {
                text.push_str("# (the newest line is longer than the export; its start is cut)\n")
            }
        }
    }
    text.push_str(&String::from_utf8_lossy(&bytes));
    Ok(text)
}

pub fn core_log_path(paths: &AppPaths) -> PathBuf {
    // The generator writes `log.output` to `shared/sing-box.log` whenever a
    // shared root is passed, including when that root equals the private root.
    paths.shared_root().join("sing-box.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_config_excludes_the_runtime_package_and_logs_next_to_it() {
        let root = std::env::temp_dir().join(format!(
            "ice-mobile-config-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = AppPaths::with_shared(&root, &root);
        paths.ensure_dirs().unwrap();
        write_config(&paths, Some("com.yilongmusk.icebox.debug")).unwrap();
        let raw = std::fs::read_to_string(paths.config()).unwrap();
        let cfg: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            cfg["inbounds"][0]["exclude_package"][0],
            "com.yilongmusk.icebox.debug"
        );
        assert_eq!(cfg["inbounds"][0]["type"], "tun");
        let log = cfg["log"]["output"].as_str().unwrap();
        assert_eq!(log, core_log_path(&paths).to_string_lossy());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn core_log_export_keeps_the_newest_whole_lines_under_the_cap() {
        let root = temp_root("core-log-export");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("sing-box.log");

        let missing = core_log_export(&path, 64).unwrap();
        assert!(missing.starts_with("# ice-box "), "{missing}");
        assert!(missing.ends_with("# no log yet\n"), "{missing}");

        let lines: String = (0..20).map(|i| format!("WARN line {i:02}\n")).collect();
        std::fs::write(&path, &lines).unwrap();
        let whole = core_log_export(&path, CORE_LOG_MAX_BYTES).unwrap();
        assert!(whole.ends_with(&lines), "a small log is exported whole");

        // Lines are 13 bytes. A 40-byte tail starts on the newline ending
        // line 16, so lines 17-19 are kept whole; 30 bytes starts inside
        // line 17, which is dropped.
        let body = |max| {
            let text = core_log_export(&path, max).unwrap();
            text.split_once('\n').unwrap().1.to_string()
        };
        assert_eq!(body(40), "WARN line 17\nWARN line 18\nWARN line 19\n");
        assert_eq!(body(30), "WARN line 18\nWARN line 19\n");

        // One line longer than the tail is kept, not exported as empty.
        std::fs::write(&path, "x".repeat(100)).unwrap();
        let long = core_log_export(&path, 40).unwrap();
        assert!(long.ends_with(&"x".repeat(40)), "{long}");
        assert!(long.contains("its start is cut"), "{long}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn geoip_rule_sets_copy_srs_files_and_leave_existing_ones() {
        let root = temp_root("geoip-copy");
        let source = root.join("source");
        let target = root.join("geoip");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("geoip-cn.srs"), b"fresh").unwrap();
        std::fs::write(source.join("README.txt"), b"skip").unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("geoip-cn.srs"), b"kept").unwrap();

        let copied = install_geoip_rule_sets(&target, &source).unwrap();
        assert_eq!(copied, 0);
        assert_eq!(std::fs::read(target.join("geoip-cn.srs")).unwrap(), b"kept");
        assert!(!target.join("README.txt").exists());

        std::fs::remove_file(target.join("geoip-cn.srs")).unwrap();
        let copied = install_geoip_rule_sets(&target, &source).unwrap();
        assert_eq!(copied, 1);
        assert_eq!(
            std::fs::read(target.join("geoip-cn.srs")).unwrap(),
            b"fresh"
        );
        assert_eq!(
            install_geoip_rule_sets(&target, &root.join("missing")).unwrap(),
            0
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn geoip_cn_rule_survives_when_the_rule_set_is_installed() {
        let root = temp_root("geoip-config");
        let paths = AppPaths::with_shared(&root, &root);
        paths.ensure_dirs().unwrap();
        let source = root.join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("geoip-cn.srs"), b"srs").unwrap();
        install_geoip_rule_sets(&paths.geoip_dir(), &source).unwrap();

        let id = uuid::Uuid::new_v4();
        let meta = ice_subscription::SubscriptionMeta {
            id,
            name: "geoip".into(),
            url: "https://example.com/sub".into(),
            active: true,
            format: ice_subscription::SubscriptionFormat::UriList,
            node_count: 1,
            group_count: 0,
            rule_count: 0,
            has_dns: false,
            parse_warnings: Vec::new(),
            last_updated: None,
            last_error: None,
            etag: None,
            last_modified: None,
            fetched_as: None,
            userinfo: None,
            provider_info: Vec::new(),
            auto_update: false,
            auto_update_interval: None,
            source: ice_subscription::SubscriptionSource::Remote,
        };
        let profile = NormalizedProfile::from_nodes_only(vec![ice_config::NormalizedOutbound {
            tag: "node".into(),
            outbound: Arc::new(serde_json::json!({
                "type": "socks",
                "tag": "node",
                "server": "203.0.113.10",
                "server_port": 1080
            })),
        }]);
        ice_subscription::write_subscription_success(
            &ice_subscription::SubscriptionPaths::from_app(&paths),
            &meta,
            "",
            &profile,
        )
        .unwrap();

        write_config(&paths, Some("com.yilongmusk.icebox")).unwrap();
        let raw = std::fs::read_to_string(paths.config()).unwrap();
        let cfg: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let rules = cfg["route"]["rules"].as_array().unwrap();
        assert!(
            rules.iter().any(|rule| rule["rule_set"][0] == "geoip-cn"),
            "GEOIP,cn must stay in the mobile config, got {raw}"
        );
        let sets = cfg["route"]["rule_set"].as_array().unwrap();
        let path = sets
            .iter()
            .find(|set| set["tag"] == "geoip-cn")
            .and_then(|set| set["path"].as_str())
            .unwrap();
        assert_eq!(
            path,
            paths.geoip_dir().join("geoip-cn.srs").to_string_lossy()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ice-mobile-{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
