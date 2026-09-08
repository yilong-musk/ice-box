// SPDX-License-Identifier: GPL-3.0-or-later

//! Size-based log rotation used by the core process and the desktop shell (ARCH-1).

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

/// Rotate when a log file exceeds 20 MiB (architecture review CORE-7).
pub const SIZED_LOG_MAX_BYTES: u64 = 20 * 1024 * 1024;
/// Rotated `ice-box.log.1` .. `ice-box.log.5`.
pub const APP_LOG_KEEP: u32 = 5;
/// Rotated `sing-box.log.1` .. `sing-box.log.3`.
pub const CORE_LOG_KEEP: u32 = 3;
/// Same cap as [`SIZED_LOG_MAX_BYTES`]; exported for the core watchdog.
pub const CORE_LOG_MAX_BYTES: u64 = SIZED_LOG_MAX_BYTES;

/// Whether `path` exists and is larger than `max_bytes`.
pub fn log_file_oversized(path: &Path, max_bytes: u64) -> bool {
    fs::metadata(path)
        .map(|m| m.len() > max_bytes)
        .unwrap_or(false)
}

fn rotated_path(path: &Path, n: u32) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{n}"));
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

/// Rotate `path` to `path.1` .. `path.{keep}` when it exceeds `max_bytes`.
/// Returns `true` when a rotation happened.
pub fn rotate_sized_log(path: &Path, max_bytes: u64, keep: u32) -> io::Result<bool> {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if meta.len() <= max_bytes {
        return Ok(false);
    }
    rotate_log_now(path, keep)?;
    Ok(true)
}

/// Rename `path` to `path.1` and shift existing rotations, regardless of size.
pub fn rotate_log_now(path: &Path, keep: u32) -> io::Result<()> {
    if keep == 0 {
        let _ = fs::remove_file(path);
        return Ok(());
    }
    let oldest = rotated_path(path, keep);
    let _ = fs::remove_file(&oldest);
    for i in (1..keep).rev() {
        let from = rotated_path(path, i);
        let to = rotated_path(path, i + 1);
        if from.exists() {
            fs::rename(&from, &to)?;
        }
    }
    if path.exists() {
        fs::rename(path, rotated_path(path, 1))?;
    }
    Ok(())
}

/// Truncate `path` in place (same inode) so a process that already holds the
/// file still writes into an empty log. Also drops rotated siblings `path.1..`.
pub fn truncate_log_file(path: &Path, keep: u32) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let _ = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    for i in 1..=keep {
        let _ = fs::remove_file(rotated_path(path, i));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-log-rotate-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn rotate_sized_log_shifts_generations_and_keeps_cap() {
        let dir = temp_dir("rotate");
        let path = dir.join("sing-box.log");
        fs::write(&path, vec![b'x'; 64]).expect("seed");
        assert!(rotate_sized_log(&path, 32, 3).expect("rotate"));
        assert!(!path.exists(), "current file is renamed to .1");
        assert_eq!(fs::read(dir.join("sing-box.log.1")).unwrap().len(), 64);

        fs::write(&path, vec![b'y'; 64]).expect("seed2");
        assert!(rotate_sized_log(&path, 32, 3).expect("rotate2"));
        assert_eq!(fs::read(dir.join("sing-box.log.1")).unwrap()[0], b'y');
        assert_eq!(fs::read(dir.join("sing-box.log.2")).unwrap()[0], b'x');

        fs::write(&path, vec![b'z'; 8]).expect("small");
        assert!(!rotate_sized_log(&path, 32, 3).expect("under cap"));
        assert_eq!(fs::read(&path).unwrap()[0], b'z');

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn truncate_log_file_empties_current_and_drops_rotations() {
        let dir = temp_dir("truncate");
        let path = dir.join("ice-box.log");
        fs::write(&path, b"keep-inode").expect("seed");
        fs::write(dir.join("ice-box.log.1"), b"old").expect("rot");
        truncate_log_file(&path, 5).expect("truncate");
        assert_eq!(fs::read(&path).unwrap(), b"");
        assert!(!dir.join("ice-box.log.1").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_file_oversized_reflects_cap() {
        let dir = temp_dir("oversize");
        let path = dir.join("ice-box.log");
        fs::write(&path, vec![b'x'; 11]).expect("seed");
        assert!(!log_file_oversized(&path, 11));
        assert!(log_file_oversized(&path, 10));
        assert!(!log_file_oversized(&path, 11));
        let _ = fs::remove_dir_all(&dir);
    }
}
