// SPDX-License-Identifier: GPL-3.0-or-later

//! Embedded MCP and local control transport for desktop and headless runtimes.
//!
//! One process owns `AppState`. The loopback listener accepts authenticated
//! connections from either the MCP stdio bridge (`--mcp-stdio`) or the CLI
//! control client, then dispatches every tool through the shared application
//! use cases under the same orchestration lock.

pub mod bridge;
pub mod protocol;
mod service;

use crate::{AppHost, AppState};
use std::sync::Arc;

/// Access policy enforced by the local MCP/control listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpPolicy {
    /// Reject mutating tools (only tools with `readOnlyHint: true` may run).
    pub read_only: bool,
    /// Allow tools that return credentials (currently `export_subscription`).
    pub allow_secrets: bool,
}

impl Default for McpPolicy {
    fn default() -> Self {
        Self {
            read_only: false,
            allow_secrets: true,
        }
    }
}

/// Shared runtime handle passed into the MCP listener and protocol dispatcher.
#[derive(Clone)]
pub struct McpContext {
    pub state: Arc<AppState>,
    pub host: Arc<dyn AppHost + Send + Sync>,
    pub policy: McpPolicy,
}

impl McpContext {
    pub fn new(state: Arc<AppState>, host: Arc<dyn AppHost + Send + Sync>) -> Self {
        Self {
            state,
            host,
            policy: McpPolicy::default(),
        }
    }

    pub fn with_policy(mut self, policy: McpPolicy) -> Self {
        self.policy = policy;
        self
    }
}

pub use bridge::{
    call_tool, default_data_dir, list_tools, ping, run_stdio_bridge, BridgeError, ToolCallOutput,
};
pub use protocol::tools;
pub use service::{McpService, McpStatus};
