// SPDX-License-Identifier: GPL-3.0-or-later

//! Sanitize untrusted sing-box rule-sets.
//!
//! A rule-set downloaded from a subscription is untrusted input, and the
//! sing-box core that eventually loads it may run elevated (TUN mode). This
//! crate is the only place such bytes are interpreted on our side:
//!
//! 1. Decode the input, either the binary `.srs` format or the `source` JSON
//!    format (detected from the content, never from the provider's claim).
//! 2. Map it onto a strict model with a fixed set of matcher fields; anything
//!    else rejects the whole rule-set.
//! 3. Validate every value (string shape, regex syntax, port ranges, CIDR
//!    breadth) under explicit size limits.
//! 4. Re-encode a canonical, compact `source` JSON document.
//!
//! The core therefore only parses JSON that this crate wrote. The crate does
//! no I/O; callers own downloading, caching and staging.

mod encode;
mod go_regex;
mod model;
mod source;
mod srs;
mod validate;

pub use model::{Cidr, DefaultRule, LogicalMode, LogicalRule, Rule, RuleSet};

use std::fmt;

/// Highest rule-set version understood by the pinned sing-box core (1.14).
pub const MAX_RULE_SET_VERSION: u8 = 5;

/// Size and shape limits applied while decoding and validating.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Largest accepted input, in bytes (either format).
    pub max_input_bytes: usize,
    /// Largest zlib-decompressed `.srs` payload, in bytes.
    pub max_decompressed_bytes: usize,
    /// Largest canonical JSON output, in bytes.
    pub max_output_bytes: usize,
    /// Most rules, counting nested logical rules.
    pub max_rules: usize,
    /// Most matcher values across the whole rule-set.
    pub max_items: usize,
    /// Longest plain string value (domain, process name, ...).
    pub max_string_bytes: usize,
    /// Longest regex value.
    pub max_regex_bytes: usize,
    /// Deepest logical rule nesting.
    pub max_depth: usize,
    /// Shortest accepted IPv4 prefix length (rejects catch-all ranges).
    pub min_ipv4_prefix: u8,
    /// Shortest accepted IPv6 prefix length.
    pub min_ipv6_prefix: u8,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_bytes: 32 * 1024 * 1024,
            max_decompressed_bytes: 64 * 1024 * 1024,
            max_output_bytes: 32 * 1024 * 1024,
            max_rules: 100_000,
            max_items: 2_000_000,
            max_string_bytes: 512,
            max_regex_bytes: 1024,
            max_depth: 8,
            // Real GeoIP data aggregates to /7 (e.g. 214.0.0.0/7 in US);
            // anything wider is treated as a catch-all.
            min_ipv4_prefix: 4,
            // 2000::/3 is all global unicast.
            min_ipv6_prefix: 8,
        }
    }
}

/// Input format, detected from the bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Binary,
    Source,
}

/// Counts reported for a sanitized rule-set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    pub rules: usize,
    pub domains: usize,
    pub cidrs: usize,
    pub regexes: usize,
    pub items: usize,
}

/// A sanitized rule-set and its canonical JSON encoding.
#[derive(Debug, Clone)]
pub struct Sanitized {
    pub input_format: InputFormat,
    pub rule_set: RuleSet,
    /// Compact canonical `source` JSON, ready to write as a local rule-set.
    pub json: Vec<u8>,
    pub stats: Stats,
}

/// Why a rule-set was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleSetError {
    /// A size or count limit was exceeded.
    TooLarge(String),
    /// The bytes are not a well-formed rule-set.
    Malformed(String),
    /// Well-formed, but uses a field or version we do not accept.
    Unsupported(String),
    /// A value failed validation.
    Invalid(String),
}

impl fmt::Display for RuleSetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge(m) => write!(f, "rule-set too large: {m}"),
            Self::Malformed(m) => write!(f, "malformed rule-set: {m}"),
            Self::Unsupported(m) => write!(f, "unsupported rule-set: {m}"),
            Self::Invalid(m) => write!(f, "invalid rule-set: {m}"),
        }
    }
}

impl std::error::Error for RuleSetError {}

/// Detect the input format: `.srs` starts with the `SRS` magic, anything
/// else must be JSON.
pub fn detect_format(bytes: &[u8]) -> InputFormat {
    if bytes.starts_with(&srs::MAGIC) {
        InputFormat::Binary
    } else {
        InputFormat::Source
    }
}

/// Decode, validate and re-encode an untrusted rule-set.
pub fn sanitize(bytes: &[u8], limits: &Limits) -> Result<Sanitized, RuleSetError> {
    if bytes.len() > limits.max_input_bytes {
        return Err(RuleSetError::TooLarge(format!(
            "input is {} bytes, limit {}",
            bytes.len(),
            limits.max_input_bytes
        )));
    }
    let input_format = detect_format(bytes);
    let decoded = match input_format {
        InputFormat::Binary => srs::decode(bytes, limits)?,
        InputFormat::Source => source::parse(bytes, limits)?,
    };
    let (rule_set, stats) = validate::canonicalize(decoded, limits)?;
    let json = encode::to_json(&rule_set);
    if json.len() > limits.max_output_bytes {
        return Err(RuleSetError::TooLarge(format!(
            "canonical output is {} bytes, limit {}",
            json.len(),
            limits.max_output_bytes
        )));
    }
    Ok(Sanitized {
        input_format,
        rule_set,
        json,
        stats,
    })
}

#[cfg(test)]
mod tests;
