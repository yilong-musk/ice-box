// SPDX-License-Identifier: GPL-3.0-or-later

//! Decoder for the sing-box binary rule-set format (`.srs`).
//!
//! Layout (sing-box `common/srs`): magic `SRS`, a version byte, then a zlib
//! stream holding a uvarint rule count and the rules. A default rule is a
//! sequence of `(item type, payload)` pairs terminated by `0xFF` and an
//! invert byte; a logical rule is a mode byte, nested rules and an invert
//! byte. Domains are stored as a succinct trie (sing `common/domain`), IP
//! sets as inclusive address ranges.
//!
//! Every length read from the input is checked against the bytes that
//! remain before anything is allocated, so a hostile count cannot reserve
//! more memory than the (already bounded) decompressed payload.

use std::collections::BTreeSet;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::model::{Cidr, DefaultRule, LogicalMode, LogicalRule, Rule, RuleSet};
use crate::{Limits, RuleSetError, MAX_RULE_SET_VERSION};

pub(crate) const MAGIC: [u8; 3] = *b"SRS";

const ITEM_QUERY_TYPE: u8 = 0;
const ITEM_NETWORK: u8 = 1;
const ITEM_DOMAIN: u8 = 2;
const ITEM_DOMAIN_KEYWORD: u8 = 3;
const ITEM_DOMAIN_REGEX: u8 = 4;
const ITEM_SOURCE_IP_CIDR: u8 = 5;
const ITEM_IP_CIDR: u8 = 6;
const ITEM_SOURCE_PORT: u8 = 7;
const ITEM_SOURCE_PORT_RANGE: u8 = 8;
const ITEM_PORT: u8 = 9;
const ITEM_PORT_RANGE: u8 = 10;
const ITEM_PROCESS_NAME: u8 = 11;
const ITEM_PROCESS_PATH: u8 = 12;
const ITEM_PACKAGE_NAME: u8 = 13;
const ITEM_WIFI_SSID: u8 = 14;
const ITEM_WIFI_BSSID: u8 = 15;
const ITEM_ADGUARD_DOMAIN: u8 = 16;
const ITEM_PROCESS_PATH_REGEX: u8 = 17;
const ITEM_NETWORK_TYPE: u8 = 18;
const ITEM_NETWORK_IS_EXPENSIVE: u8 = 19;
const ITEM_NETWORK_IS_CONSTRAINED: u8 = 20;
const ITEM_NETWORK_INTERFACE_ADDRESS: u8 = 21;
const ITEM_DEFAULT_INTERFACE_ADDRESS: u8 = 22;
const ITEM_PACKAGE_NAME_REGEX: u8 = 23;
const ITEM_FINAL: u8 = 0xFF;

/// Trie label marking a suffix entry that only matches subdomains
/// (`.example.com`).
const PREFIX_LABEL: char = '\r';
/// Trie label marking a suffix entry that matches the domain and its
/// subdomains (`example.com`).
const ROOT_LABEL: char = '\n';

/// Longest trie key: a 253-byte domain plus a marker, with headroom for
/// non-ASCII labels.
const MAX_KEY_BYTES: usize = 1024;

fn malformed(msg: impl Into<String>) -> RuleSetError {
    RuleSetError::Malformed(msg.into())
}

pub(crate) fn decode(bytes: &[u8], limits: &Limits) -> Result<RuleSet, RuleSetError> {
    if bytes.len() < MAGIC.len() + 1 || bytes[..3] != MAGIC {
        return Err(malformed("missing SRS magic"));
    }
    let version = bytes[3];
    if version == 0 || version > MAX_RULE_SET_VERSION {
        return Err(RuleSetError::Unsupported(format!(
            "binary version {version}"
        )));
    }
    let payload = inflate(&bytes[4..], limits.max_decompressed_bytes)?;
    let mut decoder = Decoder {
        cur: Cursor::new(&payload),
        limits,
        rules: 0,
        items: 0,
    };
    let count = decoder.cur.uvarint()?;
    if count > limits.max_rules as u64 {
        return Err(RuleSetError::TooLarge(format!("{count} rules")));
    }
    let mut rules = Vec::new();
    for _ in 0..count {
        rules.push(decoder.rule(0)?);
    }
    if !decoder.cur.is_empty() {
        return Err(malformed("trailing bytes after the last rule"));
    }
    Ok(RuleSet { version, rules })
}

fn inflate(compressed: &[u8], max: usize) -> Result<Vec<u8>, RuleSetError> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(compressed)
        .take(max as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|err| malformed(format!("zlib: {err}")))?;
    if out.len() > max {
        return Err(RuleSetError::TooLarge(format!(
            "decompressed payload exceeds {max} bytes"
        )));
    }
    Ok(out)
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    fn u8(&mut self) -> Result<u8, RuleSetError> {
        let b = *self
            .buf
            .get(self.pos)
            .ok_or_else(|| malformed("unexpected end of data"))?;
        self.pos += 1;
        Ok(b)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], RuleSetError> {
        if n > self.remaining() {
            return Err(malformed("unexpected end of data"));
        }
        let out = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    /// Go `binary.ReadUvarint`: at most 10 bytes, no overflow.
    fn uvarint(&mut self) -> Result<u64, RuleSetError> {
        let mut x: u64 = 0;
        let mut shift = 0u32;
        for i in 0..10 {
            let b = self.u8()?;
            if b < 0x80 {
                if i == 9 && b > 1 {
                    return Err(malformed("uvarint overflow"));
                }
                return Ok(x | (u64::from(b) << shift));
            }
            x |= u64::from(b & 0x7f) << shift;
            shift += 7;
        }
        Err(malformed("uvarint overflow"))
    }

    /// A uvarint count of elements of `elem_bytes` each, checked against
    /// the remaining input.
    fn count(&mut self, elem_bytes: usize) -> Result<usize, RuleSetError> {
        let n = self.uvarint()?;
        let fits = usize::try_from(n)
            .ok()
            .and_then(|n| n.checked_mul(elem_bytes.max(1)))
            .is_some_and(|total| total <= self.remaining());
        if !fits {
            return Err(malformed(format!("count {n} exceeds remaining data")));
        }
        Ok(n as usize)
    }

    fn u64_be(&mut self) -> Result<u64, RuleSetError> {
        let raw = self.take(8)?;
        Ok(u64::from_be_bytes(raw.try_into().expect("8 bytes")))
    }

    fn bool(&mut self) -> Result<bool, RuleSetError> {
        // Go `binary.Read` into a bool: any non-zero byte is true.
        Ok(self.u8()? != 0)
    }
}

struct Decoder<'a, 'l> {
    cur: Cursor<'a>,
    limits: &'l Limits,
    rules: usize,
    items: usize,
}

impl Decoder<'_, '_> {
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

    fn rule(&mut self, depth: usize) -> Result<Rule, RuleSetError> {
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
        match self.cur.u8()? {
            0 => self.default_rule().map(Rule::Default),
            1 => self.logical_rule(depth).map(Rule::Logical),
            other => Err(malformed(format!("unknown rule type {other}"))),
        }
    }

    fn logical_rule(&mut self, depth: usize) -> Result<LogicalRule, RuleSetError> {
        let mode = match self.cur.u8()? {
            0 => LogicalMode::And,
            1 => LogicalMode::Or,
            other => return Err(malformed(format!("unknown logical mode {other}"))),
        };
        // Each nested rule takes at least two bytes (type and terminator).
        let count = self.cur.count(2)?;
        // A `Rule` is hundreds of bytes in memory, so the byte check above
        // does not bound the reservation: charge the rule budget first.
        if count > self.limits.max_rules.saturating_sub(self.rules) {
            return Err(RuleSetError::TooLarge(format!(
                "more than {} rules",
                self.limits.max_rules
            )));
        }
        let mut rules = Vec::with_capacity(count);
        for _ in 0..count {
            rules.push(self.rule(depth + 1)?);
        }
        let invert = self.cur.bool()?;
        Ok(LogicalRule {
            mode,
            rules,
            invert,
        })
    }

    fn default_rule(&mut self) -> Result<DefaultRule, RuleSetError> {
        let mut rule = DefaultRule::default();
        let mut seen = [false; 256];
        loop {
            let item = self.cur.u8()?;
            if item == ITEM_FINAL {
                rule.invert = self.cur.bool()?;
                return Ok(rule);
            }
            if std::mem::replace(&mut seen[item as usize], true) {
                return Err(malformed(format!("duplicate rule item {item}")));
            }
            match item {
                ITEM_QUERY_TYPE => rule.query_type = self.u16_list()?,
                ITEM_NETWORK => rule.network = self.strings()?,
                ITEM_DOMAIN => {
                    let (domain, suffix) = self.domain_matcher()?;
                    rule.domain = domain;
                    rule.domain_suffix = suffix;
                }
                ITEM_DOMAIN_KEYWORD => rule.domain_keyword = self.strings()?,
                ITEM_DOMAIN_REGEX => rule.domain_regex = self.strings()?,
                ITEM_SOURCE_IP_CIDR => rule.source_ip_cidr = self.ip_set()?,
                ITEM_IP_CIDR => rule.ip_cidr = self.ip_set()?,
                ITEM_SOURCE_PORT => rule.source_port = self.u16_list()?,
                ITEM_SOURCE_PORT_RANGE => rule.source_port_range = self.strings()?,
                ITEM_PORT => rule.port = self.u16_list()?,
                ITEM_PORT_RANGE => rule.port_range = self.strings()?,
                ITEM_PROCESS_NAME => rule.process_name = self.strings()?,
                ITEM_PROCESS_PATH => rule.process_path = self.strings()?,
                ITEM_PROCESS_PATH_REGEX => rule.process_path_regex = self.strings()?,
                ITEM_PACKAGE_NAME => rule.package_name = self.strings()?,
                ITEM_PACKAGE_NAME_REGEX => rule.package_name_regex = self.strings()?,
                ITEM_WIFI_SSID => rule.wifi_ssid = self.strings()?,
                ITEM_WIFI_BSSID => rule.wifi_bssid = self.strings()?,
                ITEM_ADGUARD_DOMAIN => {
                    return Err(RuleSetError::Unsupported("adguard_domain".into()))
                }
                ITEM_NETWORK_TYPE
                | ITEM_NETWORK_IS_EXPENSIVE
                | ITEM_NETWORK_IS_CONSTRAINED
                | ITEM_NETWORK_INTERFACE_ADDRESS
                | ITEM_DEFAULT_INTERFACE_ADDRESS => {
                    return Err(RuleSetError::Unsupported(format!(
                        "network interface matcher (item {item})"
                    )))
                }
                other => return Err(malformed(format!("unknown rule item {other}"))),
            }
        }
    }

    fn u16_list(&mut self) -> Result<Vec<u16>, RuleSetError> {
        let n = self.cur.count(2)?;
        self.add_items(n)?;
        let raw = self.cur.take(n * 2)?;
        Ok(raw
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect())
    }

    fn strings(&mut self) -> Result<Vec<String>, RuleSetError> {
        // Each string takes at least its one-byte length prefix.
        let n = self.cur.count(1)?;
        self.add_items(n)?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let len = self.cur.count(1)?;
            let raw = self.cur.take(len)?;
            let s = std::str::from_utf8(raw).map_err(|_| malformed("string is not UTF-8"))?;
            out.push(s.to_string());
        }
        Ok(out)
    }

    fn u64_words(&mut self) -> Result<Vec<u64>, RuleSetError> {
        let n = self.cur.count(8)?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.cur.u64_be()?);
        }
        Ok(out)
    }

    /// Read the succinct domain trie and dump it the way sing's
    /// `Matcher.Dump` does: `(domain, domain_suffix)`.
    fn domain_matcher(&mut self) -> Result<(Vec<String>, Vec<String>), RuleSetError> {
        let set_version = self.cur.u8()?;
        if set_version != 0 {
            return Err(malformed(format!("domain set version {set_version}")));
        }
        let leaves = self.u64_words()?;
        let label_bitmap = self.u64_words()?;
        let label_count = self.cur.count(1)?;
        let labels = self.cur.take(label_count)?;
        let keys = succinct_keys(&leaves, &label_bitmap, labels, self.limits)?;
        self.add_items(keys.len())?;
        dump_domains(keys)
    }

    fn ip_set(&mut self) -> Result<Vec<Cidr>, RuleSetError> {
        let version = self.cur.u8()?;
        if version != 1 {
            return Err(malformed(format!("IP set version {version}")));
        }
        let n = self.cur.u64_be()?;
        // A range is at least two (length, 4-byte address) pairs.
        if n.saturating_mul(10) > self.cur.remaining() as u64 {
            return Err(malformed(format!(
                "IP set count {n} exceeds remaining data"
            )));
        }
        let mut out = Vec::new();
        for _ in 0..n {
            let from = self.ip_addr()?;
            let to = self.ip_addr()?;
            let before = out.len();
            range_to_cidrs(from, to, &mut out)?;
            self.add_items(out.len() - before)?;
        }
        Ok(out)
    }

    fn ip_addr(&mut self) -> Result<IpAddr, RuleSetError> {
        let len = self.cur.uvarint()?;
        match len {
            4 => {
                let raw: [u8; 4] = self.cur.take(4)?.try_into().expect("4 bytes");
                Ok(IpAddr::V4(Ipv4Addr::from(raw)))
            }
            16 => {
                let raw: [u8; 16] = self.cur.take(16)?.try_into().expect("16 bytes");
                // sing `M.AddrFromIP` unmaps IPv4-mapped IPv6 addresses.
                let v6 = Ipv6Addr::from(raw);
                Ok(match v6.to_ipv4_mapped() {
                    Some(v4) => IpAddr::V4(v4),
                    None => IpAddr::V6(v6),
                })
            }
            other => Err(malformed(format!("IP address length {other}"))),
        }
    }
}

/// Enumerate the keys of a LOUDS-encoded succinct trie.
///
/// `label_bitmap` lists the nodes in breadth-first order: each node is a run
/// of 0 bits (one per child edge, labelled by the next byte of `labels`)
/// closed by a 1 bit. The child created by the i-th 0 bit is node `i + 1`;
/// node 0 is the root. `leaves` marks nodes that end a key.
fn succinct_keys(
    leaves: &[u64],
    label_bitmap: &[u64],
    labels: &[u8],
    limits: &Limits,
) -> Result<Vec<Vec<u8>>, RuleSetError> {
    let ones: usize = label_bitmap.iter().map(|w| w.count_ones() as usize).sum();
    let last_one = label_bitmap
        .iter()
        .enumerate()
        .rev()
        .find(|(_, w)| **w != 0)
        .map(|(i, w)| i * 64 + 63 - w.leading_zeros() as usize);
    let Some(last_one) = last_one else {
        return Err(malformed("empty domain set"));
    };
    let zeros = last_one + 1 - ones;
    // Same structural check as sing `readSuccinctSet`.
    if ones != zeros + 1 || labels.len() != zeros {
        return Err(malformed("succinct set shape"));
    }
    if zeros > limits.max_decompressed_bytes {
        return Err(RuleSetError::TooLarge("succinct set nodes".into()));
    }

    // Per node: parent id and the label of the edge leading to it.
    let mut parent: Vec<u32> = Vec::with_capacity(zeros + 1);
    let mut edge: Vec<u8> = Vec::with_capacity(zeros + 1);
    let mut depth: Vec<u16> = Vec::with_capacity(zeros + 1);
    parent.push(0);
    edge.push(0);
    depth.push(0);
    let mut node = 0usize;
    for idx in 0..=last_one {
        if (label_bitmap[idx >> 6] >> (idx & 63)) & 1 == 1 {
            node += 1;
            continue;
        }
        // The current node must already exist: ids 0..=created.
        if node >= parent.len() {
            return Err(malformed("succinct set references an unknown node"));
        }
        let d = depth[node] as usize + 1;
        if d > MAX_KEY_BYTES {
            return Err(RuleSetError::TooLarge("domain key too long".into()));
        }
        parent.push(node as u32);
        edge.push(labels[parent.len() - 2]);
        depth.push(d as u16);
    }

    let is_leaf = |id: usize| {
        leaves
            .get(id >> 6)
            .is_some_and(|w| (w >> (id & 63)) & 1 == 1)
    };
    if is_leaf(0) {
        // An empty key would make sing's Dump index past the string end.
        return Err(malformed("empty domain key"));
    }
    let mut keys = Vec::new();
    let mut total = 0usize;
    for (id, &len) in depth.iter().enumerate().skip(1) {
        if !is_leaf(id) {
            continue;
        }
        let len = len as usize;
        total += len;
        if total > limits.max_decompressed_bytes {
            return Err(RuleSetError::TooLarge("domain keys".into()));
        }
        let mut key = vec![0u8; len];
        let mut cur = id;
        for slot in key.iter_mut().rev() {
            *slot = edge[cur];
            cur = parent[cur] as usize;
        }
        keys.push(key);
    }
    Ok(keys)
}

/// Turn trie keys into `domain` / `domain_suffix` lists, mirroring sing
/// `Matcher.Dump`. Keys are stored reversed (by Unicode scalar), with a
/// trailing marker for suffix entries.
fn dump_domains(keys: Vec<Vec<u8>>) -> Result<(Vec<String>, Vec<String>), RuleSetError> {
    let mut domains = BTreeSet::new();
    let mut prefixes = BTreeSet::new();
    let mut suffixes = BTreeSet::new();
    for raw in keys {
        let key = String::from_utf8(raw).map_err(|_| malformed("domain key is not UTF-8"))?;
        let key: String = key.chars().rev().collect();
        let mut chars = key.chars();
        match chars.next() {
            Some(PREFIX_LABEL) => {
                prefixes.insert(chars.as_str().to_string());
            }
            Some(ROOT_LABEL) => {
                suffixes.insert(chars.as_str().to_string());
            }
            _ => {
                domains.insert(key);
            }
        }
    }
    for prefix in prefixes {
        // A legacy (version 1) suffix is stored as the bare domain plus
        // `.domain`; fold the pair back into one suffix entry.
        if let Some(root) = prefix.strip_prefix('.') {
            if domains.remove(root) {
                suffixes.insert(root.to_string());
                continue;
            }
        }
        suffixes.insert(prefix);
    }
    Ok((
        domains.into_iter().collect(),
        suffixes.into_iter().collect(),
    ))
}

/// Split an inclusive address range into the minimal list of prefixes.
fn range_to_cidrs(from: IpAddr, to: IpAddr, out: &mut Vec<Cidr>) -> Result<(), RuleSetError> {
    let (start, end, width, v4) = match (from, to) {
        (IpAddr::V4(a), IpAddr::V4(b)) => (
            u128::from(u32::from(a)),
            u128::from(u32::from(b)),
            32u32,
            true,
        ),
        (IpAddr::V6(a), IpAddr::V6(b)) => (u128::from(a), u128::from(b), 128u32, false),
        _ => return Err(malformed("IP range mixes address families")),
    };
    if start > end {
        return Err(malformed("IP range start is after its end"));
    }
    // Last address of the block of 2^bits addresses starting at `cur`. `cur`
    // is aligned to the block, so the sum cannot overflow.
    let block_last = |cur: u128, bits: u32| {
        if bits >= 128 {
            u128::MAX
        } else {
            cur + ((1u128 << bits) - 1)
        }
    };
    let mut cur = start;
    loop {
        // Largest aligned block starting at `cur` that stays within `end`.
        let mut bits = if cur == 0 {
            width
        } else {
            cur.trailing_zeros().min(width)
        };
        while bits > 0 && block_last(cur, bits) > end {
            bits -= 1;
        }
        let addr = if v4 {
            IpAddr::V4(Ipv4Addr::from(cur as u32))
        } else {
            IpAddr::V6(Ipv6Addr::from(cur))
        };
        out.push(Cidr::new(addr, (width - bits) as u8).expect("prefix within family width"));
        let last = block_last(cur, bits);
        if last >= end {
            return Ok(());
        }
        cur = last + 1;
    }
}

#[cfg(test)]
pub(crate) fn range_to_cidrs_for_test(from: IpAddr, to: IpAddr) -> Vec<Cidr> {
    let mut out = Vec::new();
    range_to_cidrs(from, to, &mut out).expect("valid range");
    out
}
