//! Shared tracing setup for the desktop shell and crates.
//!
//! Convention: `tracing` + optional file append. Log rotation is deferred to a later slice.

use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::sync::Mutex;

use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

/// Initialize global tracing.
///
/// - Always logs to stderr.
/// - When `log_file` is set, also appends to that path (parent dirs created as needed).
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
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("open log file {}: {e}", path.display()))?;

        let file_layer = fmt::layer()
            .with_ansi(false)
            .with_writer(Mutex::new(file))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn append_open_creates_log_file_in_nested_dir() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-log-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
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
