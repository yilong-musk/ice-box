// SPDX-License-Identifier: GPL-3.0-or-later

//! `sing-box.pid` read / write helpers.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    let tmp_name = format!(".{file_name}.{nanos}.tmp");
    let tmp = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(tmp_name),
        _ => PathBuf::from(tmp_name),
    };
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut f = File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// Parse pid file contents. Invalid / empty / zero → `None` (never panics).
pub fn parse_pid_contents(raw: &str) -> Option<u32> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let pid: u32 = trimmed.parse().ok()?;
    if pid == 0 {
        None
    } else {
        Some(pid)
    }
}

/// Read pid file. Missing or invalid contents → `Ok(None)`.
pub fn read_pid(path: &Path) -> io::Result<Option<u32>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)?;
    Ok(parse_pid_contents(&raw))
}

/// Atomically write a pid file.
pub fn write_pid(path: &Path, pid: u32) -> io::Result<()> {
    if pid == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pid must be non-zero",
        ));
    }
    write_bytes_atomic(path, format!("{pid}\n").as_bytes())
}

/// Remove pid file if present. Missing file is Ok.
pub fn clear_pid(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// If the file exists but contents are not a usable pid, delete it (no panic).
pub fn purge_invalid_pid_file(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    match read_pid(path)? {
        Some(_) => Ok(()),
        None => clear_pid(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_pid_path(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-pid-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("mkdir");
        dir.join("sing-box.pid")
    }

    #[test]
    fn invalid_pid_contents_are_ignored() {
        assert_eq!(parse_pid_contents(""), None);
        assert_eq!(parse_pid_contents("   "), None);
        assert_eq!(parse_pid_contents("0"), None);
        assert_eq!(parse_pid_contents("not-a-pid"), None);
        assert_eq!(parse_pid_contents("12abc"), None);
        assert_eq!(parse_pid_contents("12345"), Some(12345));
    }

    #[test]
    fn invalid_pid_file_does_not_panic_and_can_be_purged() {
        let path = temp_pid_path("bad");
        fs::write(&path, b"garbage-pid").expect("seed");

        let parsed = read_pid(&path).expect("read");
        assert_eq!(parsed, None);

        purge_invalid_pid_file(&path).expect("purge");
        assert!(!path.exists());

        // Missing after purge is fine
        assert_eq!(read_pid(&path).expect("read missing"), None);
        purge_invalid_pid_file(&path).expect("purge missing");

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn write_and_read_pid_roundtrip() {
        let path = temp_pid_path("ok");
        write_pid(&path, 4242).expect("write");
        assert_eq!(read_pid(&path).expect("read"), Some(4242));
        clear_pid(&path).expect("clear");
        assert!(!path.exists());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
