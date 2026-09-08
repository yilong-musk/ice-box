// SPDX-License-Identifier: GPL-3.0-or-later

//! Desktop-shell process runtime helpers (architecture review ARCH-1).
//!
//! Tracing initialization lives here so `ice-config` stays a pure config
//! builder. Size-based rotation itself lives in `ice-core` (the process
//! layer also rotates the core log on spawn).

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ice_core::{rotate_log_now, rotate_sized_log, APP_LOG_KEEP, SIZED_LOG_MAX_BYTES};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

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
            "ice-box-runtime-log-{label}-{}",
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
}
