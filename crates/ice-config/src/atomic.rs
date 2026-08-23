//! Atomic file replace: write temp sibling → `rename`.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::ConfigError;

fn tmp_path_for(target: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let file_name = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let tmp_name = format!(".{file_name}.{nanos}.tmp");
    match target.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(tmp_name),
        _ => PathBuf::from(tmp_name),
    }
}

/// Write `bytes` to `path` via temp file + rename. Leaves no `.tmp` on success.
pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    write_bytes_atomic_inner(path, bytes, false)
}

fn write_bytes_atomic_inner(
    path: &Path,
    bytes: &[u8],
    fail_before_rename: bool,
) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let tmp = tmp_path_for(path);
    let write_result = (|| -> io::Result<()> {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(err) = write_result {
        let _ = fs::remove_file(&tmp);
        return Err(ConfigError::from(err));
    }

    if fail_before_rename {
        let _ = fs::remove_file(&tmp);
        return Err(ConfigError::Io(io::Error::other(
            "injected failure before rename",
        )));
    }

    fs::rename(&tmp, path).map_err(|err| {
        let _ = fs::remove_file(&tmp);
        ConfigError::from(err)
    })?;

    Ok(())
}

/// Serialize `value` as pretty JSON and atomically replace `path`.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), ConfigError> {
    let text = serde_json::to_string_pretty(value)?;
    write_bytes_atomic(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-atomic-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn atomic_write_success_leaves_complete_file_without_tmp() {
        let dir = temp_dir("ok");
        let path = dir.join("settings.json");
        write_bytes_atomic(&path, br#"{"ok":true}"#).expect("write");

        let contents = fs::read_to_string(&path).expect("read");
        assert_eq!(contents, r#"{"ok":true}"#);

        let leftovers: Vec<_> = fs::read_dir(&dir)
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "no tmp leftovers: {leftovers:?}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_failure_before_rename_keeps_old_content() {
        let dir = temp_dir("fail");
        let path = dir.join("settings.json");
        fs::write(&path, b"old-content").expect("seed");

        let err = write_bytes_atomic_inner(&path, b"new-content", true).expect_err("injected");
        assert!(matches!(err, ConfigError::Io(_)));

        let contents = fs::read_to_string(&path).expect("read");
        assert_eq!(contents, "old-content");

        let leftovers: Vec<_> = fs::read_dir(&dir)
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "tmp cleaned after inject: {leftovers:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
