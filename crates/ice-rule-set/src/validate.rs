// SPDX-License-Identifier: GPL-3.0-or-later

//! Value validation and canonical ordering.
//!
//! Any invalid value rejects the whole rule-set. Dropping a single matcher
//! is not safe: inside a rule, matcher groups are ANDed, so removing one
//! widens the rule, and `invert` flips that again.

use std::net::IpAddr;

use crate::model::{Cidr, DefaultRule, Rule, RuleSet};
use crate::{Limits, RuleSetError, Stats};

fn invalid(msg: impl Into<String>) -> RuleSetError {
    RuleSetError::Invalid(msg.into())
}

pub(crate) fn canonicalize(
    mut set: RuleSet,
    limits: &Limits,
) -> Result<(RuleSet, Stats), RuleSetError> {
    let mut stats = Stats::default();
    for rule in &mut set.rules {
        visit(rule, limits, &mut stats)?;
    }
    if stats.items > limits.max_items {
        return Err(RuleSetError::TooLarge(format!(
            "more than {} items",
            limits.max_items
        )));
    }
    Ok((set, stats))
}

fn visit(rule: &mut Rule, limits: &Limits, stats: &mut Stats) -> Result<(), RuleSetError> {
    stats.rules += 1;
    match rule {
        Rule::Default(rule) => default_rule(rule, limits, stats),
        Rule::Logical(logical) => {
            if logical.rules.is_empty() {
                return Err(invalid("logical rule without rules"));
            }
            for rule in &mut logical.rules {
                visit(rule, limits, stats)?;
            }
            Ok(())
        }
    }
}

fn default_rule(
    rule: &mut DefaultRule,
    limits: &Limits,
    stats: &mut Stats,
) -> Result<(), RuleSetError> {
    if rule.is_empty() {
        return Err(invalid("rule without matchers"));
    }
    for network in &rule.network {
        if !matches!(network.as_str(), "tcp" | "udp" | "icmp") {
            return Err(invalid(format!("network {network:?}")));
        }
    }
    for list in [
        &mut rule.domain,
        &mut rule.domain_suffix,
        &mut rule.domain_keyword,
    ] {
        for value in list.iter() {
            check_domain(value, limits)?;
        }
        sort_dedup(list);
        stats.domains += list.len();
    }
    for list in [
        &mut rule.domain_regex,
        &mut rule.process_path_regex,
        &mut rule.package_name_regex,
    ] {
        for value in list.iter() {
            check_regex(value, limits)?;
        }
        sort_dedup(list);
        stats.regexes += list.len();
    }
    for list in [
        &mut rule.process_name,
        &mut rule.process_path,
        &mut rule.package_name,
        &mut rule.wifi_ssid,
        &mut rule.wifi_bssid,
    ] {
        for value in list.iter() {
            check_plain(value, limits)?;
        }
        sort_dedup(list);
    }
    for list in [&mut rule.source_port_range, &mut rule.port_range] {
        for value in list.iter() {
            check_port_range(value)?;
        }
        sort_dedup(list);
    }
    for list in [&mut rule.ip_cidr, &mut rule.source_ip_cidr] {
        for cidr in list.iter() {
            check_cidr(cidr, limits)?;
        }
        list.sort();
        list.dedup();
        stats.cidrs += list.len();
    }
    for list in [&mut rule.query_type, &mut rule.port, &mut rule.source_port] {
        list.sort_unstable();
        list.dedup();
    }
    sort_dedup(&mut rule.network);
    stats.items += item_count(rule);
    Ok(())
}

fn item_count(rule: &DefaultRule) -> usize {
    rule.query_type.len()
        + rule.network.len()
        + rule.domain.len()
        + rule.domain_suffix.len()
        + rule.domain_keyword.len()
        + rule.domain_regex.len()
        + rule.source_ip_cidr.len()
        + rule.ip_cidr.len()
        + rule.source_port.len()
        + rule.source_port_range.len()
        + rule.port.len()
        + rule.port_range.len()
        + rule.process_name.len()
        + rule.process_path.len()
        + rule.process_path_regex.len()
        + rule.package_name.len()
        + rule.package_name_regex.len()
        + rule.wifi_ssid.len()
        + rule.wifi_bssid.len()
}

fn sort_dedup(list: &mut Vec<String>) {
    list.sort_unstable();
    list.dedup();
}

/// Non-empty, bounded, no control characters.
fn check_plain(value: &str, limits: &Limits) -> Result<(), RuleSetError> {
    if value.is_empty() {
        return Err(invalid("empty string"));
    }
    if value.len() > limits.max_string_bytes {
        return Err(invalid(format!(
            "string longer than {} bytes",
            limits.max_string_bytes
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(invalid(format!("control character in {value:?}")));
    }
    Ok(())
}

/// A plain string that also has no whitespace. Unicode labels are allowed
/// (sing-box compares raw strings), but `\r` / `\n` would collide with the
/// trie markers and are already excluded as control characters.
fn check_domain(value: &str, limits: &Limits) -> Result<(), RuleSetError> {
    check_plain(value, limits)?;
    if value.chars().any(char::is_whitespace) {
        return Err(invalid(format!("whitespace in domain {value:?}")));
    }
    Ok(())
}

/// Bounded, and certain to compile under Go's `regexp` with the same
/// meaning: a pattern the core refuses would stop it from starting. Go's
/// RE2 engine matches in linear time, so matching cost is not a concern.
fn check_regex(value: &str, limits: &Limits) -> Result<(), RuleSetError> {
    if value.is_empty() {
        return Err(invalid("empty regex"));
    }
    if value.len() > limits.max_regex_bytes {
        return Err(invalid(format!(
            "regex longer than {} bytes",
            limits.max_regex_bytes
        )));
    }
    if value.contains('\0') {
        return Err(invalid("NUL in regex"));
    }
    crate::go_regex::check(value).map_err(|err| invalid(format!("regex {value:?}: {err}")))
}

/// `start:end`, `start:` or `:end` with ports in range and start <= end.
fn check_port_range(value: &str) -> Result<(), RuleSetError> {
    let bad = || invalid(format!("port range {value:?}"));
    let (start, end) = value.split_once(':').ok_or_else(bad)?;
    let port = |s: &str| -> Result<Option<u16>, RuleSetError> {
        if s.is_empty() {
            return Ok(None);
        }
        if s.len() > 5 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        s.parse::<u16>().map(Some).map_err(|_| bad())
    };
    match (port(start)?, port(end)?) {
        (None, None) => Err(bad()),
        (Some(a), Some(b)) if a > b => Err(bad()),
        _ => Ok(()),
    }
}

fn check_cidr(cidr: &Cidr, limits: &Limits) -> Result<(), RuleSetError> {
    let min = match cidr.addr {
        IpAddr::V4(_) => limits.min_ipv4_prefix,
        IpAddr::V6(_) => limits.min_ipv6_prefix,
    };
    if cidr.prefix < min {
        return Err(invalid(format!("prefix {cidr} is broader than /{min}")));
    }
    Ok(())
}
