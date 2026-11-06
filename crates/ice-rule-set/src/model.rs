// SPDX-License-Identifier: GPL-3.0-or-later

//! The strict rule-set model. Only the fields listed here survive
//! sanitization; field names match sing-box headless rules.

use std::fmt;
use std::net::IpAddr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleSet {
    pub version: u8,
    pub rules: Vec<Rule>,
}

// Rule-sets hold few rules with many values each, so the size gap between
// variants does not matter; boxing would only add indirection.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rule {
    Default(DefaultRule),
    Logical(LogicalRule),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalMode {
    And,
    Or,
}

impl LogicalMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalRule {
    pub mode: LogicalMode,
    pub rules: Vec<Rule>,
    pub invert: bool,
}

/// A headless default rule restricted to the accepted matchers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefaultRule {
    pub query_type: Vec<u16>,
    pub network: Vec<String>,
    pub domain: Vec<String>,
    pub domain_suffix: Vec<String>,
    pub domain_keyword: Vec<String>,
    pub domain_regex: Vec<String>,
    pub source_ip_cidr: Vec<Cidr>,
    pub ip_cidr: Vec<Cidr>,
    pub source_port: Vec<u16>,
    pub source_port_range: Vec<String>,
    pub port: Vec<u16>,
    pub port_range: Vec<String>,
    pub process_name: Vec<String>,
    pub process_path: Vec<String>,
    pub process_path_regex: Vec<String>,
    pub package_name: Vec<String>,
    pub package_name_regex: Vec<String>,
    pub wifi_ssid: Vec<String>,
    pub wifi_bssid: Vec<String>,
    pub invert: bool,
}

impl DefaultRule {
    /// Whether the rule has no matcher at all.
    pub fn is_empty(&self) -> bool {
        self.query_type.is_empty()
            && self.network.is_empty()
            && self.domain.is_empty()
            && self.domain_suffix.is_empty()
            && self.domain_keyword.is_empty()
            && self.domain_regex.is_empty()
            && self.source_ip_cidr.is_empty()
            && self.ip_cidr.is_empty()
            && self.source_port.is_empty()
            && self.source_port_range.is_empty()
            && self.port.is_empty()
            && self.port_range.is_empty()
            && self.process_name.is_empty()
            && self.process_path.is_empty()
            && self.process_path_regex.is_empty()
            && self.package_name.is_empty()
            && self.package_name_regex.is_empty()
            && self.wifi_ssid.is_empty()
            && self.wifi_bssid.is_empty()
    }
}

/// An IP prefix with host bits cleared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cidr {
    pub addr: IpAddr,
    pub prefix: u8,
}

impl Cidr {
    /// Build a prefix, clearing host bits. `None` when `prefix` is too long
    /// for the address family.
    pub fn new(addr: IpAddr, prefix: u8) -> Option<Self> {
        match addr {
            IpAddr::V4(v4) => {
                if prefix > 32 {
                    return None;
                }
                let mask = if prefix == 0 {
                    0
                } else {
                    u32::MAX << (32 - prefix)
                };
                let bits = u32::from(v4) & mask;
                Some(Self {
                    addr: IpAddr::V4(bits.into()),
                    prefix,
                })
            }
            IpAddr::V6(v6) => {
                if prefix > 128 {
                    return None;
                }
                let mask = if prefix == 0 {
                    0
                } else {
                    u128::MAX << (128 - prefix)
                };
                let bits = u128::from(v6) & mask;
                Some(Self {
                    addr: IpAddr::V6(bits.into()),
                    prefix,
                })
            }
        }
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix)
    }
}
