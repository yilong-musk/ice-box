// SPDX-License-Identifier: GPL-3.0-or-later

//! Size cap for the core process and the desktop shell (ARCH-1 / CORE-7).
//!
//! A single file stays at most [`SIZED_LOG_MAX_BYTES`]. Overflow drops the
//! oldest [`SIZED_LOG_TRIM_BYTES`] in place (same inode, line-aligned) so a
//! process that already holds the file (`O_APPEND`) keeps writing into the
//! retained tail. Leftover `path.N` siblings from the old rename-rotation
//! scheme are deleted.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Shrink when a log file exceeds 20 MiB (architecture review CORE-7).
pub const SIZED_LOG_MAX_BYTES: u64 = 20 * 1024 * 1024;
/// Oldest bytes dropped at the 20 MiB cap (`SIZED_LOG_MAX_BYTES / 4`).
pub const SIZED_LOG_TRIM_BYTES: u64 = 5 * 1024 * 1024;
/// Leftover `ice-box.log.1` .. `ice-box.log.5` from rename-rotation.
pub const APP_LOG_KEEP: u32 = 5;
/// Leftover `sing-box.log.1` .. `sing-box.log.3` from rename-rotation.
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

fn drop_rotated_siblings(path: &Path, keep: u32) {
    for i in 1..=keep {
        let _ = fs::remove_file(rotated_path(path, i));
    }
}

/// Drop at least a quarter of `max_bytes` (5 MiB at the 20 MiB cap), and
/// enough extra that the retained tail is at most `max_bytes`.
fn drop_prefix_len(len: u64, max_bytes: u64) -> u64 {
    if len <= 1 {
        return 0;
    }
    let drop = (max_bytes / 4).max(1);
    debug_assert!(
        max_bytes != SIZED_LOG_MAX_BYTES || drop == SIZED_LOG_TRIM_BYTES,
        "20 MiB cap must drop 5 MiB"
    );
    let over = len.saturating_sub(max_bytes);
    drop.max(over).min(len - 1)
}

/// Keep `file` from `start`, aligned to the next full line.
fn tail_from(file: &mut File, start: u64) -> io::Result<Vec<u8>> {
    let len = file.metadata()?.len();
    if start == 0 || len <= 1 {
        file.seek(SeekFrom::Start(0))?;
        let mut all = Vec::new();
        file.read_to_end(&mut all)?;
        return Ok(all);
    }
    let start = start.min(len - 1);
    let at_line_start = {
        file.seek(SeekFrom::Start(start - 1))?;
        let mut prev = [0u8; 1];
        file.read_exact(&mut prev)?;
        prev[0] == b'\n'
    };
    file.seek(SeekFrom::Start(start))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail)?;
    if !at_line_start {
        if let Some(i) = tail.iter().position(|&b| b == b'\n') {
            if i + 1 < tail.len() {
                tail.drain(..=i);
            }
        }
    }
    Ok(tail)
}

fn rewrite_in_place(file: &mut File, data: &[u8]) -> io::Result<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(data)?;
    file.flush()?;
    Ok(())
}

/// Drop the oldest slice of `path` in place (same inode) so an `O_APPEND`
/// writer keeps going. Also drops leftover `path.N` siblings. Returns the
/// new file length.
pub fn trim_log_file(path: &Path, max_bytes: u64, keep: u32) -> io::Result<u64> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let len = file.metadata()?.len();
    let kept = tail_from(&mut file, drop_prefix_len(len, max_bytes))?;
    rewrite_in_place(&mut file, &kept)?;
    drop_rotated_siblings(path, keep);
    Ok(kept.len() as u64)
}

/// When `path` exceeds `max_bytes`, drop the oldest quarter in place and
/// always drop leftover rename-rotation siblings. Returns `true` when the
/// current file was shrunk.
pub fn cap_log_file(path: &Path, max_bytes: u64, keep: u32) -> io::Result<bool> {
    if log_file_oversized(path, max_bytes) {
        trim_log_file(path, max_bytes, keep)?;
        return Ok(true);
    }
    drop_rotated_siblings(path, keep);
    Ok(false)
}

/// Truncate `path` in place (same inode) so a process that already holds the
/// file still writes into an empty log. Also drops leftover `path.1..`.
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
    drop_rotated_siblings(path, keep);
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
    fn drop_prefix_len_is_five_mib_at_the_sized_cap() {
        assert_eq!(SIZED_LOG_MAX_BYTES / 4, SIZED_LOG_TRIM_BYTES);
        let just_over = SIZED_LOG_MAX_BYTES + 1;
        assert_eq!(
            drop_prefix_len(just_over, SIZED_LOG_MAX_BYTES),
            SIZED_LOG_TRIM_BYTES
        );
    }

    #[test]
    fn cap_log_file_drops_oldest_quarter_and_siblings() {
        let dir = temp_dir("cap");
        let path = dir.join("sing-box.log");
        fs::write(&path, vec![b'x'; 64]).expect("seed");
        fs::write(dir.join("sing-box.log.1"), b"old").expect("rot");
        assert!(cap_log_file(&path, 32, 3).expect("cap"));
        assert_eq!(fs::read(&path).unwrap(), vec![b'x'; 32]);
        assert!(!dir.join("sing-box.log.1").exists());
        assert!(!path.with_file_name("sing-box.log.2").exists());

        fs::write(&path, vec![b'y'; 8]).expect("small");
        assert!(!cap_log_file(&path, 32, 3).expect("under cap"));
        assert_eq!(fs::read(&path).unwrap()[0], b'y');

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cap_log_file_drops_oldest_bytes_on_line_boundary() {
        let dir = temp_dir("lines");
        let path = dir.join("sing-box.log");
        let mut body = String::new();
        for _ in 0..16 {
            body.push_str("OLD-LINE\n");
        }
        for _ in 0..24 {
            body.push_str("NEW-LINE\n");
        }
        fs::write(&path, &body).expect("seed");
        assert!(cap_log_file(&path, 200, 3).expect("cap"));
        let kept = fs::read_to_string(&path).unwrap();
        assert!(!kept.contains("OLD-LINE"), "{kept:?}");
        assert!(kept.starts_with("NEW-LINE\n"), "{kept:?}");
        assert!(kept.contains("NEW-LINE\n"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cap_log_file_drops_siblings_when_under_cap() {
        let dir = temp_dir("siblings");
        let path = dir.join("ice-box.log");
        fs::write(&path, b"keep").expect("seed");
        fs::write(dir.join("ice-box.log.1"), b"old").expect("rot");
        assert!(!cap_log_file(&path, 32, 5).expect("under cap"));
        assert_eq!(fs::read(&path).unwrap(), b"keep");
        assert!(!dir.join("ice-box.log.1").exists());
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
