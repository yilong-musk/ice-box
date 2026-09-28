// SPDX-License-Identifier: GPL-3.0-or-later

//! Application use cases shared by IPC, tray actions, and background workers.
//! This layer does not import Tauri; shell integration uses `AppHost`.

mod common;
mod core;
mod host;
mod logs;
mod nodes;
mod proxy_terminal;
mod settings;
mod subscription;
mod tun;

pub(crate) use common::*;
pub(crate) use core::*;
pub(crate) use host::{AppHost, AppResources};
pub(crate) use logs::*;
pub(crate) use nodes::*;
pub(crate) use proxy_terminal::*;
pub(crate) use settings::*;
pub(crate) use subscription::*;
pub(crate) use tun::*;

#[cfg(test)]
mod tests;
