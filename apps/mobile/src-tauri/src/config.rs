// SPDX-License-Identifier: GPL-3.0-or-later

//! Write the config the `:tunnel` process reads. The host never starts libbox.

use std::path::PathBuf;
use std::sync::Arc;

use ice_app::build_mobile_config;
use ice_config::{
    ensure_clash_api_secret, load_group_selections, load_rule_overrides, load_settings,
    write_runtime_config_file, AppError, AppPaths, BuildInput, CaptureIntent, HostPlatform,
    LocalTemplate, NormalizedProfile,
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
}
