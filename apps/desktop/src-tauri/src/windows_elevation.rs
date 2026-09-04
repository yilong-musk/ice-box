//! Windows one-shot elevation for the TUN scheduled task (plan B).
//!
//! The scheduled task that runs the TUN core elevated must be created once
//! from an elevated context. The runtime flow does that through a single
//! `runas`-verb UAC prompt (`ensure_tun_elevation`); afterwards every TUN
//! transition runs unelevated via `schtasks /Run` / `/End` — no further
//! prompts. The app-level UAC relaunch flow (relaunch the whole app elevated
//! per session) was removed when the scheduled-task elevation landed.

/// Stable error code when a UAC prompt was cancelled (or the elevated
/// invocation could not be started). Nothing was modified.
#[cfg(target_os = "windows")]
pub const ERR_ELEVATION_CANCELLED: &str = "tun.elevation_cancelled";
