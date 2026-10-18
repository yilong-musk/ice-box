// SPDX-License-Identifier: GPL-3.0-or-later

//! Embedded MCP transport for the desktop runtime.
//!
//! The desktop process owns the only `AppState`. The MCP listener accepts
//! authenticated loopback connections from the lightweight stdio bridge, then
//! dispatches tools through the same application use cases as the GUI.

mod bridge;
mod protocol;
mod service;

pub(crate) use bridge::run_stdio_bridge;
pub use service::{McpService, McpStatus};
