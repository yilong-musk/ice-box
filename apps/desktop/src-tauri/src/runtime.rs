// SPDX-License-Identifier: GPL-3.0-or-later

//! Desktop-shell process runtime helpers.
//!
//! Tracing initialization lives here so `ice-config` stays a pure config
//! builder. Size-cap helpers live in `ice-core` (the process layer also
//! caps the core log on spawn).

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ice_core::{cap_log_file, trim_log_file, APP_LOG_KEEP, SIZED_LOG_MAX_BYTES};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

static FILE_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// Initialize global tracing.
///
/// - Always logs to stderr.
/// - When `log_file` is set, also writes to that path and drops the oldest
///   quarter in place at [`SIZED_LOG_MAX_BYTES`] (5 MiB at 20 MiB; same inode;
///   leftover `path.N` siblings from the old rename-rotation scheme are deleted).
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
        let _ = cap_log_file(path, SIZED_LOG_MAX_BYTES, APP_LOG_KEEP);
        let capped = SizeCappedWriter::open(path, SIZED_LOG_MAX_BYTES, APP_LOG_KEEP)
            .map_err(|e| format!("open log file {}: {e}", path.display()))?;
        let (non_blocking, guard) = tracing_appender::non_blocking(capped);
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

struct SizeCappedWriter {
    path: PathBuf,
    max_bytes: u64,
    keep: u32,
    file: Option<File>,
    len: u64,
}

impl SizeCappedWriter {
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

    fn wrap(&mut self) -> io::Result<()> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
            drop(file);
        }
        self.len = trim_log_file(&self.path, self.max_bytes, self.keep)?;
        self.file = Some(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?,
        );
        Ok(())
    }

    fn file_mut(&mut self) -> io::Result<&mut File> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("log writer is closed"))
    }
}

impl Write for SizeCappedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.len > 0 && self.len.saturating_add(buf.len() as u64) > self.max_bytes {
            self.wrap()?;
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

    #[test]
    fn size_capped_writer_drops_oldest_quarter_without_siblings() {
        let dir = temp_dir("cap");
        let path = dir.join("ice-box.log");
        fs::write(dir.join("ice-box.log.1"), b"old").expect("rot");

        let mut writer = SizeCappedWriter::open(&path, 32, APP_LOG_KEEP).expect("open");
        writer.write_all(&[b'a'; 20]).expect("first");
        writer.write_all(&[b'b'; 20]).expect("wrap");
        writer.flush().expect("flush");
        drop(writer);

        let body = fs::read(&path).expect("read");
        assert_eq!(&body[body.len().saturating_sub(20)..], &[b'b'; 20]);
        assert!(
            body.len() <= 32,
            "wrapped file must stay at or under the cap"
        );
        assert!(
            body.contains(&b'a'),
            "newest part of the first chunk is kept"
        );
        assert!(!dir.join("ice-box.log.1").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
