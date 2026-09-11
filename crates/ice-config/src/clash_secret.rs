// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-install Clash API secret persisted next to `settings.json`.

use std::fs;
use std::path::Path;

use ice_types::is_plausible_clash_api_secret;

use crate::atomic::write_bytes_atomic_mode;
use crate::ConfigError;

const SECRET_FILE_MODE: u32 = 0o600;

/// Load a plausible secret from `path`, or generate one and replace the file.
pub fn ensure_clash_api_secret(path: &Path) -> Result<String, ConfigError> {
    if let Ok(existing) = fs::read_to_string(path) {
        let trimmed = existing.trim();
        if is_plausible_clash_api_secret(trimmed) {
            restrict_unix_secret_file(path);
            return Ok(trimmed.to_string());
        }
    }
    let secret = generate_clash_api_secret()?;
    write_bytes_atomic_mode(path, secret.as_bytes(), SECRET_FILE_MODE)?;
    Ok(secret)
}

fn restrict_unix_secret_file(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(meta) = fs::metadata(path) else {
            return;
        };
        if meta.permissions().mode() & 0o777 != SECRET_FILE_MODE {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(SECRET_FILE_MODE));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

fn generate_clash_api_secret() -> Result<String, ConfigError> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|err| ConfigError::invalid(format!("generate clash api secret: {err}")))?;
    Ok(hex_encode(&bytes))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_secret_path(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ice-box-clash-secret-{label}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("clash-api.secret")
    }

    #[test]
    fn ensure_clash_api_secret_persists_and_reuses() {
        let path = temp_secret_path("persist");
        let first = ensure_clash_api_secret(&path).expect("create");
        assert!(is_plausible_clash_api_secret(&first));
        assert_eq!(first.len(), 64);
        let second = ensure_clash_api_secret(&path).expect("reuse");
        assert_eq!(first, second);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "secret file must be 0600, got {mode:#o}"
            );
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn ensure_clash_api_secret_replaces_invalid_contents() {
        let path = temp_secret_path("invalid");
        std::fs::write(&path, "short").unwrap();
        let secret = ensure_clash_api_secret(&path).expect("replace");
        assert!(is_plausible_clash_api_secret(&secret));
        assert_ne!(secret, "short");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_clash_api_secret_repairs_world_readable_mode() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_secret_path("mode");
        std::fs::write(&path, crate::EXAMPLE_CLASH_API_SECRET).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let secret = ensure_clash_api_secret(&path).expect("reuse");
        assert_eq!(secret, crate::EXAMPLE_CLASH_API_SECRET);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "reused secret file must be repaired to 0600, got {mode:#o}"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
