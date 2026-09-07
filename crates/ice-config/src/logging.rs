// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared tracing setup for the desktop shell and crates.
//!
//! App logs use a size-rotating file writer behind `tracing-appender`'s
//! non-blocking worker (keep 5). Core logs are rotated on spawn by
//! [`rotate_sized_log`].

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

/// Rotate when a log file exceeds 20 MiB (architecture review CORE-7).
pub const SIZED_LOG_MAX_BYTES: u64 = 20 * 1024 * 1024;
/// Rotated `ice-box.log.1` .. `ice-box.log.5`.
pub const APP_LOG_KEEP: u32 = 5;
/// Rotated `sing-box.log.1` .. `sing-box.log.3`.
pub const CORE_LOG_KEEP: u32 = 3;
/// Same cap as [`SIZED_LOG_MAX_BYTES`]; exported for the core watchdog.
pub const CORE_LOG_MAX_BYTES: u64 = SIZED_LOG_MAX_BYTES;

static FILE_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// Initialize global tracing.
///
/// - Always logs to stderr.
/// - When `log_file` is set, also writes to that path with size-based rotation
///   (keep [`APP_LOG_KEEP`]).
/// - Filter defaults to `info`; override with `RUST_LOG`.
///
/// Safe to call once at process start. Subsequent calls return an error from
/// `tracing_subscriber` if a global subscriber is already set.
pub fn init_logging(log_file: Option<&Path>) -> Result<(), String> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let stderr_layer = fmt::layer()
        .with_ansi(true)
        .with_writer(io::stderr)
        .with_target(true);

    if let Some(path) = log_file {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create log dir {}: {e}", parent.display()))?;
        }
        let _ = rotate_sized_log(path, SIZED_LOG_MAX_BYTES, APP_LOG_KEEP);
        let rotating = SizeRotatingWriter::open(path, SIZED_LOG_MAX_BYTES, APP_LOG_KEEP)
            .map_err(|e| format!("open log file {}: {e}", path.display()))?;
        let (non_blocking, guard) = tracing_appender::non_blocking(rotating);
        let _ = FILE_GUARD.set(guard);

        let file_layer = fmt::layer()
            .with_ansi(false)
            .with_writer(non_blocking)
            .with_target(true);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(stderr_layer)
            .with(file_layer)
            .try_init()
            .map_err(|e| format!("init tracing: {e}"))?;

        tracing::info!(path = %path.display(), "file logging enabled");
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(stderr_layer)
            .try_init()
            .map_err(|e| format!("init tracing: {e}"))?;
    }

    Ok(())
}

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

fn rotate_log_now(path: &Path, keep: u32) -> io::Result<()> {
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

struct SizeRotatingWriter {
    path: PathBuf,
    max_bytes: u64,
    keep: u32,
    file: Option<File>,
    len: u64,
}

impl SizeRotatingWriter {
    fn open(path: &Path, max_bytes: u64, keep: u32) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            path: path.to_path_buf(),
            max_bytes,
            keep,
            file: Some(file),
            len,
        })
    }

    fn rotate(&mut self) -> io::Result<()> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
            drop(file);
        }
        rotate_log_now(&self.path, self.keep)?;
        self.file = Some(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?,
        );
        self.len = 0;
        Ok(())
    }

    fn file_mut(&mut self) -> io::Result<&mut File> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("log writer is closed"))
    }
}

impl Write for SizeRotatingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.len > 0 && self.len.saturating_add(buf.len() as u64) > self.max_bytes {
            self.rotate()?;
        }
        let n = self.file_mut()?.write(buf)?;
        self.len = self.len.saturating_add(n as u64);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file_mut()?.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
    fn append_open_creates_log_file_in_nested_dir() {
        let dir = temp_dir("nested");
        let path = dir.join("logs").join("ice-box.log");
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("open append");
        writeln!(file, "probe").expect("write");
        drop(file);

        let contents = fs::read_to_string(&path).expect("read");
        assert!(contents.contains("probe"));
        let _ = fs::remove_dir_all(&dir);
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
        let path = dir.join("sing-box.log");
        assert!(!log_file_oversized(&path, 10));
        fs::write(&path, vec![0u8; 11]).expect("seed");
        assert!(log_file_oversized(&path, 10));
        assert!(!log_file_oversized(&path, 11));
        let _ = fs::remove_dir_all(&dir);
    }
}
