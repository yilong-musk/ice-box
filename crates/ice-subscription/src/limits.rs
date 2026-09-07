// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared subscription size caps (architecture §11.4). Over-limit input is
//! truncated with a warning; parsers never hard-fail on size.

/// Caps applied by Clash YAML, sing-box JSON, and URI-list parsers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_nodes: usize,
    pub max_groups: usize,
    pub max_rules: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_nodes: 500,
            max_groups: 128,
            max_rules: 10_000,
        }
    }
}

impl Limits {
    pub fn warning(kind: &str, dropped: usize) -> String {
        format!("truncated {kind}: dropped {dropped}")
    }
}
