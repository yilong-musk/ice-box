//! Resolve sing-box binary path (dev tree → resource).

use std::path::{Path, PathBuf};

use crate::error::CoreError;

/// Bundled sing-box version pin (architecture §21 / `third_party/sing-box/VERSION`).
///
/// Source of truth lives in the config engine (`ice_config::ENGINE_COMPAT_CORE_VERSION`);
/// the desktop process layer only mirrors it for packaging checks.
pub const BUNDLED_SINGBOX_VERSION: &str = ice_config::ENGINE_COMPAT_CORE_VERSION;

/// Current packaging target directory name under `third_party/sing-box/`.
pub fn current_target_dir() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "darwin-aarch64"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "darwin-x86_64"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "windows-x86_64"
    }
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64"),
    )))]
    {
        "unknown-target"
    }
}

fn binary_file_name() -> &'static str {
    if cfg!(windows) {
        "sing-box.exe"
    } else {
        "sing-box"
    }
}

/// Look for `root/<target>/sing-box[.exe]`.
pub fn binary_in_target_root(root: &Path) -> PathBuf {
    root.join(current_target_dir()).join(binary_file_name())
}

/// Resolve binary: development `third_party` first, then optional resource dir.
///
/// Order (architecture §4.3):
/// 1. `dev_third_party/<current-target>/sing-box`
/// 2. `resource_dir/sing-box` (flat) or `resource_dir/<target>/sing-box`
/// 3. else `core.not_found`
///
/// Packaged builds ship a flat `sing-box` under the Tauri resource directory; when the
/// development tree path is missing, that resource must still resolve (G8.3).
pub fn resolve_singbox_binary(
    dev_third_party: &Path,
    resource_dir: Option<&Path>,
) -> Result<PathBuf, CoreError> {
    let candidates = {
        let mut list = Vec::new();
        list.push(binary_in_target_root(dev_third_party));
        if let Some(res) = resource_dir {
            list.push(res.join(binary_file_name()));
            // Older / misconfigured bundles nested under Resources/resources/
            list.push(res.join("resources").join(binary_file_name()));
            list.push(binary_in_target_root(res));
        }
        list
    };

    for path in &candidates {
        if path.is_file() {
            return Ok(path.clone());
        }
    }

    Err(CoreError::NotFound(format!(
        "sing-box binary not found (tried {}); place it under third_party/sing-box/{}/",
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", "),
        current_target_dir()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-bin-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn missing_binary_returns_not_found() {
        let dir = temp_dir("missing");
        let err = resolve_singbox_binary(&dir, None).expect_err("missing");
        assert!(matches!(err, CoreError::NotFound(_)));
        assert_eq!(err.code().as_str(), "core.not_found");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_dev_tree_binary() {
        let root = temp_dir("dev");
        let path = binary_in_target_root(&root);
        fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        fs::write(&path, b"fake").expect("write");
        let found = resolve_singbox_binary(&root, None).expect("found");
        assert_eq!(found, path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn g8_3_resource_used_when_dev_tree_missing() {
        let empty_dev = temp_dir("empty-dev");
        let res = temp_dir("res-flat");
        let bin = res.join(binary_file_name());
        fs::write(&bin, b"from-resource").expect("write");
        let found = resolve_singbox_binary(&empty_dev, Some(&res)).expect("resource");
        assert_eq!(found, bin);
        let _ = fs::remove_dir_all(&empty_dev);
        let _ = fs::remove_dir_all(&res);
    }

    #[test]
    fn g8_3_dev_preferred_over_resource_when_both_exist() {
        let root = temp_dir("dev-pref");
        let res = temp_dir("res-pref");
        let dev_bin = binary_in_target_root(&root);
        fs::create_dir_all(dev_bin.parent().unwrap()).unwrap();
        fs::write(&dev_bin, b"dev").unwrap();
        let res_bin = res.join(binary_file_name());
        fs::write(&res_bin, b"res").unwrap();
        let found = resolve_singbox_binary(&root, Some(&res)).unwrap();
        assert_eq!(found, dev_bin);
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&res);
    }

    #[test]
    fn bundled_version_pin_matches_constant() {
        assert_eq!(BUNDLED_SINGBOX_VERSION, "1.13.19");
    }
}
