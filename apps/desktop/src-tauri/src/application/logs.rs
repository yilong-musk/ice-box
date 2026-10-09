// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[derive(Deserialize)]
pub struct LogViewRequest {
    pub n: usize,
}

/// Cap core/helper logs at 20 MiB by dropping the oldest 5 MiB of the same
/// inode. Used by the watchdog so a long-running sing-box stays inside the
/// cap. The desktop process owns the app-log writer and caps it itself.
pub(crate) fn cap_oversized_logs(state: &AppState) {
    let mut failed = false;
    if ice_core::cap_log_file(
        &state.paths.core_log(),
        ice_core::CORE_LOG_MAX_BYTES,
        ice_core::CORE_LOG_KEEP,
    )
    .is_err()
    {
        failed = true;
    }
    if state.capture.helper_core_used() && !ice_tun_sys::dev_sudo_runner_enabled() {
        if let Some(helper_log) = ice_tun_sys::elevated_core_log_path() {
            match ice_core::cap_log_file(
                &helper_log,
                ice_core::CORE_LOG_MAX_BYTES,
                ice_core::CORE_LOG_KEEP,
            ) {
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                    #[cfg(unix)]
                    {
                        if ice_core::log_file_oversized(&helper_log, ice_core::CORE_LOG_MAX_BYTES)
                            && truncate_helper_core_log(state).is_err()
                        {
                            failed = true;
                        }
                    }
                }
                Err(_) => failed = true,
            }
        }
    }
    if failed {
        push_oversized_warning(state);
    } else {
        clear_oversized_warning(state);
    }
}

fn push_oversized_warning(state: &AppState) {
    let Ok(mut slot) = state.proxy_recovery_warning.lock() else {
        return;
    };
    if slot
        .iter()
        .any(|m| m.key == ErrorCode::LogsOversized.message_key())
    {
        return;
    }
    slot.push(ErrorCode::LogsOversized.ui_message());
}

fn clear_oversized_warning(state: &AppState) {
    if let Ok(mut slot) = state.proxy_recovery_warning.lock() {
        slot.retain(|m| m.key != ErrorCode::LogsOversized.message_key());
    }
}

#[cfg(unix)]
fn map_truncate_err(label: &str, err: std::io::Error) -> AppError {
    AppError::new(ErrorCode::LogsOversized, format!("truncate {label}: {err}"))
}

/// `/var/log/ice-box-core.log` is created by the privileged helper. Trim as
/// the user first (install/start now chown it to the authorized uid); if that
/// is denied, ask the helper to empty the same inode.
#[cfg(unix)]
fn truncate_helper_core_log(state: &AppState) -> Result<(), AppError> {
    let helper_log = match ice_tun_sys::elevated_core_log_path() {
        Some(path) => path,
        None => {
            return Err(map_truncate_err(
                "helper core log",
                std::io::Error::from(std::io::ErrorKind::NotFound),
            ))
        }
    };
    match ice_core::truncate_log_file(&helper_log, ice_core::CORE_LOG_KEEP) {
        Ok(()) => return Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {}
        Err(err) => return Err(map_truncate_err("helper core log", err)),
    }
    let helper = ice_tun_sys::helper::HelperCoreCoordinator::from_data_dir(state.paths.root())
        .map_err(AppError::from)?;
    helper
        .truncate_core_log()
        .map_err(|e| AppError::new(ErrorCode::LogsOversized, e.to_string()))?;
    Ok(())
}

/// Parsed log tails. Retained only while the Logs page is open. Each source
/// keeps up to a megabyte, and that page is the only reader.
#[derive(Default)]
pub(crate) struct LogViewSlot {
    active: bool,
    reader: Option<LogViewCache>,
}

impl LogViewSlot {
    pub(crate) fn set_active(&mut self, active: bool) {
        self.active = active;
        if !active {
            self.reader = None;
        }
    }

    fn read(
        &mut self,
        app: &std::path::Path,
        core: &std::path::Path,
        helper: Option<&std::path::Path>,
        n: usize,
        debug: bool,
    ) -> Result<Vec<String>, AppError> {
        if !self.active {
            // A read that finishes after the page closes must not rebuild the
            // tails `set_active(false)` just dropped.
            let mut transient = LogViewCache::default();
            return transient.read(app, core, helper, n, debug);
        }
        self.reader
            .get_or_insert_with(LogViewCache::default)
            .read(app, core, helper, n, debug)
    }
}

/// `active` retains parsed tails. Leaving the page drops them and ignores
/// later reads until the page is open again.
pub(crate) fn set_log_view_retained(state: &AppState, active: bool) {
    if let Ok(mut slot) = state.log_view_cache.lock() {
        slot.set_active(active);
    }
}

pub(crate) fn get_log_view_use_case(
    state: &AppState,
    req: LogViewRequest,
) -> Result<Vec<String>, AppError> {
    // While TUN capture runs through the privileged helper (production
    // macOS path), the elevated core's output goes to the helper's fixed
    // log instead of the app-data core log; the dev sudo runner keeps the
    // app-data path, so it must not read the helper log. The capture
    // controller latches helper usage for the app session, so the merge
    // also persists after a TUN session ends.
    let extra_core_log = (state.capture.helper_core_used()
        && !ice_tun_sys::dev_sudo_runner_enabled())
    .then(ice_tun_sys::elevated_core_log_path)
    .flatten();
    let extra_core_log = extra_core_log.as_deref();
    let debug = current_settings(&state.paths)
        .map(|s| s.log_debug)
        .unwrap_or(false);
    // Serialize readers so overlapping polls cannot duplicate appended
    // lines or replace a newer cursor with an older snapshot.
    let mut slot = state
        .log_view_cache
        .lock()
        .map_err(|_| AppError::new(ErrorCode::ConfigInvalid, "log view cache poisoned"))?;
    slot.read(
        &state.paths.app_log(),
        &state.paths.core_log(),
        extra_core_log,
        req.n,
        debug,
    )
}

pub(crate) fn get_runtime_config_use_case(state: &AppState) -> Result<String, AppError> {
    let path = state.paths.config();
    if !path.exists() {
        return Ok(String::new());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("read config: {e}")))?;
    redact_config_str(&raw).map_err(|e| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("redact runtime config: {e}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn a_read_after_close_does_not_keep_the_parsed_tail() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-log-slot-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("dir");
        let app = dir.join("app.log");
        let core = dir.join("core.log");
        fs::write(
            &app,
            "2026-08-23T13:47:01.123456Z  INFO ice_core: sing-box ready\n",
        )
        .expect("app log");
        fs::write(&core, "").expect("core log");

        let mut slot = LogViewSlot::default();
        slot.set_active(true);
        let lines = slot.read(&app, &core, None, 20, false).expect("open read");
        assert!(
            lines.iter().any(|line| line.contains("sing-box ready")),
            "open read should show the app line, got {lines:?}"
        );
        assert!(slot.reader.is_some(), "an open page keeps the parsed tail");

        slot.set_active(false);
        assert!(slot.reader.is_none());
        let again = slot
            .read(&app, &core, None, 20, false)
            .expect("closed read");
        assert!(
            again.iter().any(|line| line.contains("sing-box ready")),
            "a late read can still return lines"
        );
        assert!(
            slot.reader.is_none(),
            "a read after close must not rebuild the cached tail"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
