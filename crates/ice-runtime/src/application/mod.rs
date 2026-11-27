// SPDX-License-Identifier: GPL-3.0-or-later

//! Application use cases shared by desktop IPC, tray actions, background
//! workers, MCP, and the headless CLI.
//! This layer does not import Tauri; shell integration uses `AppHost`.

mod common;
mod connections;
mod core;
mod exit_probe;
mod host;
mod logs;
mod nodes;
mod proxy_terminal;
mod settings;
mod subscription;
mod tun;

pub use common::*;
pub use connections::*;
pub use core::*;
pub use host::{AppHost, AppResources};
pub use logs::*;
pub use nodes::*;
pub use proxy_terminal::*;
pub use settings::*;
pub use subscription::*;
pub use tun::*;

#[cfg(test)]
mod tests;
