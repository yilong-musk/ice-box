// SPDX-License-Identifier: GPL-3.0-or-later

//! Tauri IPC commands.

mod core;
mod logs;
mod nodes;
mod proxy_terminal;
mod settings;
mod status;
mod subscription;
mod tun;

use crate::application::*;
use crate::tray::{self, TrayLanguage};
pub use core::*;
pub use logs::*;
pub use nodes::*;
pub use proxy_terminal::*;
pub use settings::*;
pub use status::*;
pub use subscription::*;
use tauri::{AppHandle, Manager, State};
pub use tun::*;

/// Join-error mapping for `spawn_blocking` (blocking work must not run on the
/// main thread — sync commands freeze the UI event loop).
pub(crate) fn blocking_join_err<E: std::fmt::Display>(context: &str) -> impl FnOnce(E) -> AppError {
    let context = context.to_string();
    move |e| AppError::new(ErrorCode::ConfigInvalid, format!("{context}: {e}"))
}

/// Run blocking IPC work on Tokio's blocking pool so the UI event loop stays live.
pub(crate) async fn run_blocking<T: Send + 'static>(
    context: &'static str,
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(blocking_join_err(context))?
}
