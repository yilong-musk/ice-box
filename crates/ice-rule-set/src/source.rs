// SPDX-License-Identifier: GPL-3.0-or-later

//! Parser for the sing-box `source` (JSON) rule-set format.
//!
//! Strict: unknown keys anywhere reject the rule-set, so a field we do not
//! model can never pass through. List fields accept a single value or an
//! array, like sing-box `Listable`.

use std::net::IpAddr;
use std::str::FromStr;

use serde_json::{Map, Value};

use crate::model::{Cidr, DefaultRule, LogicalMode, LogicalRule, Rule, RuleSet};
use crate::{Limits, RuleSetError, MAX_RULE_SET_VERSION};

fn malformed(msg: impl Into<String>) -> RuleSetError {
    RuleSetError::Malformed(msg.into())
}

pub(crate) fn parse(bytes: &[u8], limits: &Limits) -> Result<RuleSet, RuleSetError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|err| malformed(format!("JSON: {err}")))?;
    let obj = value
        .as_object()
        .ok_or_else(|| malformed("top level must be an object"))?;
    for key in obj.keys() {
        if key != "version" && key != "rules" {
            return Err(RuleSetError::Unsupported(format!("top-level key {key:?}")));
        }
    }
    let version = obj
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| malformed("version must be an integer"))?;
    if version == 0 || version > u64::from(MAX_RULE_SET_VERSION) {
        return Err(RuleSetError::Unsupported(format!(
            "source version {version}"
        )));
    }
    let raw_rules = match obj.get("rules") {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(rules)) => rules.as_slice(),
        Some(_) => return Err(malformed("rules must be an array")),
    };
    let mut parser = Parser {
        limits,
        rules: 0,
        items: 0,
    };
    let mut rules = Vec::with_capacity(raw_rules.len().min(limits.max_rules));
    for (idx, raw) in raw_rules.iter().enumerate() {
        rules.push(
            parser
                .rule(raw, 0)
                .map_err(|err| prefix_error(err, &format!("rules[{idx}]")))?,
        );
    }
    Ok(RuleSet {
        version: version as u8,
        rules,
    })
}

fn prefix_error(err: RuleSetError, at: &str) -> RuleSetError {
    match err {
        RuleSetError::TooLarge(m) => RuleSetError::TooLarge(format!("{at}: {m}")),
        RuleSetError::Malformed(m) => RuleSetError::Malformed(format!("{at}: {m}")),
        RuleSetError::Unsupported(m) => RuleSetError::Unsupported(format!("{at}: {m}")),
        RuleSetError::Invalid(m) => RuleSetError::Invalid(format!("{at}: {m}")),
    }
}

struct Parser<'l> {
    limits: &'l Limits,
    rules: usize,
    items: usize,
}

impl Parser<'_> {
    fn add_items(&mut self, n: usize) -> Result<(), RuleSetError> {
        self.items = self.items.saturating_add(n);
        if self.items > self.limits.max_items {
            return Err(RuleSetError::TooLarge(format!(
                "more than {} items",
                self.limits.max_items
            )));
        }
        Ok(())
    }

    fn rule(&mut self, raw: &Value, depth: usize) -> Result<Rule, RuleSetError> {
        if depth > self.limits.max_depth {
            return Err(RuleSetError::TooLarge(
                "logical rules nested too deep".into(),
            ));
        }
        self.rules += 1;
        if self.rules > self.limits.max_rules {
            return Err(RuleSetError::TooLarge(format!(
                "more than {} rules",
                self.limits.max_rules
            )));
        }
        let obj = raw
            .as_object()
            .ok_or_else(|| malformed("rule must be an object"))?;
        match obj.get("type").map(|t| t.as_str()) {
            None | Some(Some("")) | Some(Some("default")) => {
                self.default_rule(obj).map(Rule::Default)
            }
            Some(Some("logical")) => self.logical_rule(obj, depth).map(Rule::Logical),
            Some(other) => Err(RuleSetError::Unsupported(format!("rule type {other:?}"))),
        }
    }

    fn logical_rule(
        &mut self,
        obj: &Map<String, Value>,
        depth: usize,
    ) -> Result<LogicalRule, RuleSetError> {
        for key in obj.keys() {
            if !matches!(key.as_str(), "type" | "mode" | "rules" | "invert") {
                return Err(RuleSetError::Unsupported(format!(
                    "logical rule key {key:?}"
                )));
            }
        }
        let mode = match obj.get("mode").and_then(Value::as_str) {
            Some("and") => LogicalMode::And,
            Some("or") => LogicalMode::Or,
            other => return Err(malformed(format!("logical mode {other:?}"))),
        };
        let raw_rules = obj
            .get("rules")
            .and_then(Value::as_array)
            .ok_or_else(|| malformed("logical rule needs a rules array"))?;
        let mut rules = Vec::with_capacity(raw_rules.len().min(self.limits.max_rules));
        for (idx, raw) in raw_rules.iter().enumerate() {
            rules.push(
                self.rule(raw, depth + 1)
                    .map_err(|err| prefix_error(err, &format!("rules[{idx}]")))?,
            );
        }
        Ok(LogicalRule {
            mode,
            rules,
            invert: bool_field(obj, "invert")?,
        })
    }

    fn default_rule(&mut self, obj: &Map<String, Value>) -> Result<DefaultRule, RuleSetError> {
        let mut rule = DefaultRule::default();
        for (key, value) in obj {
            match key.as_str() {
                "type" => {}
                "invert" => rule.invert = bool_field(obj, "invert")?,
                "query_type" => rule.query_type = self.query_types(key, value)?,
                "network" => rule.network = self.strings(key, value)?,
                "domain" => rule.domain = self.strings(key, value)?,
                "domain_suffix" => rule.domain_suffix = self.strings(key, value)?,
                "domain_keyword" => rule.domain_keyword = self.strings(key, value)?,
                "domain_regex" => rule.domain_regex = self.strings(key, value)?,
                "source_ip_cidr" => rule.source_ip_cidr = self.cidrs(key, value)?,
                "ip_cidr" => rule.ip_cidr = self.cidrs(key, value)?,
                "source_port" => rule.source_port = self.ports(key, value)?,
                "source_port_range" => rule.source_port_range = self.strings(key, value)?,
                "port" => rule.port = self.ports(key, value)?,
                "port_range" => rule.port_range = self.strings(key, value)?,
                "process_name" => rule.process_name = self.strings(key, value)?,
                "process_path" => rule.process_path = self.strings(key, value)?,
                "process_path_regex" => rule.process_path_regex = self.strings(key, value)?,
                "package_name" => rule.package_name = self.strings(key, value)?,
                "package_name_regex" => rule.package_name_regex = self.strings(key, value)?,
                "wifi_ssid" => rule.wifi_ssid = self.strings(key, value)?,
                "wifi_bssid" => rule.wifi_bssid = self.strings(key, value)?,
                other => {
                    return Err(RuleSetError::Unsupported(format!("rule key {other:?}")));
                }
            }
        }
        Ok(rule)
    }

    fn listable<'v>(
        &mut self,
        key: &str,
        value: &'v Value,
    ) -> Result<Vec<&'v Value>, RuleSetError> {
        let items: Vec<&Value> = match value {
            Value::Null => Vec::new(),
            Value::Array(items) => items.iter().collect(),
            single => vec![single],
        };
        self.add_items(items.len())
            .map_err(|err| prefix_error(err, key))?;
        Ok(items)
    }

    fn strings(&mut self, key: &str, value: &Value) -> Result<Vec<String>, RuleSetError> {
        self.listable(key, value)?
            .into_iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| malformed(format!("{key}: expected a string")))
            })
            .collect()
    }

    fn ports(&mut self, key: &str, value: &Value) -> Result<Vec<u16>, RuleSetError> {
        self.listable(key, value)?
            .into_iter()
            .map(|item| {
                item.as_u64()
                    .and_then(|n| u16::try_from(n).ok())
                    .ok_or_else(|| RuleSetError::Invalid(format!("{key}: {item} is not a port")))
            })
            .collect()
    }

    fn query_types(&mut self, key: &str, value: &Value) -> Result<Vec<u16>, RuleSetError> {
        self.listable(key, value)?
            .into_iter()
            .map(|item| match item {
                Value::Number(n) => n
                    .as_u64()
                    .and_then(|n| u16::try_from(n).ok())
                    .ok_or_else(|| RuleSetError::Invalid(format!("{key}: {n}"))),
                Value::String(name) => query_type_by_name(name).ok_or_else(|| {
                    RuleSetError::Unsupported(format!("{key}: query type {name:?}"))
                }),
                other => Err(malformed(format!("{key}: {other}"))),
            })
            .collect()
    }

    fn cidrs(&mut self, key: &str, value: &Value) -> Result<Vec<Cidr>, RuleSetError> {
        self.listable(key, value)?
            .into_iter()
            .map(|item| {
                let raw = item
                    .as_str()
                    .ok_or_else(|| malformed(format!("{key}: expected a string")))?;
                parse_cidr(raw)
                    .ok_or_else(|| RuleSetError::Invalid(format!("{key}: {raw:?} is not a prefix")))
            })
            .collect()
    }
}

fn bool_field(obj: &Map<String, Value>, key: &str) -> Result<bool, RuleSetError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(malformed(format!("{key} must be a boolean"))),
    }
}

/// A prefix (`10.0.0.0/8`) or a bare address (a host prefix), as sing-box
/// accepts in `ip_cidr`.
fn parse_cidr(raw: &str) -> Option<Cidr> {
    match raw.split_once('/') {
        Some((addr, bits)) => {
            // Reject signs, spaces and leading zeros beyond a single digit.
            if bits.is_empty() || bits.len() > 3 || !bits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let addr = IpAddr::from_str(addr).ok()?;
            Cidr::new(addr, bits.parse().ok()?)
        }
        None => {
            let addr = IpAddr::from_str(raw).ok()?;
            let bits = if addr.is_ipv4() { 32 } else { 128 };
            Cidr::new(addr, bits)
        }
    }
}

/// DNS query type names sing-box accepts (miekg/dns `StringToType`), limited
/// to the common ones; anything else must be given as a number.
fn query_type_by_name(name: &str) -> Option<u16> {
    Some(match name.to_ascii_uppercase().as_str() {
        "A" => 1,
        "NS" => 2,
        "CNAME" => 5,
        "SOA" => 6,
        "PTR" => 12,
        "HINFO" => 13,
        "MX" => 15,
        "TXT" => 16,
        "AAAA" => 28,
        "SRV" => 33,
        "NAPTR" => 35,
        "DS" => 43,
        "RRSIG" => 46,
        "DNSKEY" => 48,
        "SVCB" => 64,
        "HTTPS" => 65,
        "ANY" => 255,
        "CAA" => 257,
        _ => return None,
    })
}
