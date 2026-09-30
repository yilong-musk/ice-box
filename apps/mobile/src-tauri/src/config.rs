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
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../third_party/sing-geoip/rule-set");
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
            userinfo: None,
            provider_info: Vec::new(),
            auto_update: false,
            auto_update_interval: None,
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
