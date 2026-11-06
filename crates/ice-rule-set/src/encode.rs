// SPDX-License-Identifier: GPL-3.0-or-later

//! Canonical `source` JSON encoding: fixed key order, empty fields omitted,
//! compact output.

use serde_json::{json, Map, Value};

use crate::model::{DefaultRule, Rule, RuleSet};

pub(crate) fn to_json(set: &RuleSet) -> Vec<u8> {
    let rules: Vec<Value> = set.rules.iter().map(rule).collect();
    let doc = json!({ "version": set.version, "rules": rules });
    serde_json::to_vec(&doc).expect("serializing a JSON value cannot fail")
}

fn rule(rule: &Rule) -> Value {
    match rule {
        Rule::Default(rule) => default_rule(rule),
        Rule::Logical(logical) => {
            let mut obj = Map::new();
            obj.insert("type".into(), "logical".into());
            obj.insert("mode".into(), logical.mode.as_str().into());
            obj.insert(
                "rules".into(),
                logical
                    .rules
                    .iter()
                    .map(self::rule)
                    .collect::<Vec<_>>()
                    .into(),
            );
            if logical.invert {
                obj.insert("invert".into(), true.into());
            }
            Value::Object(obj)
        }
    }
}

fn default_rule(rule: &DefaultRule) -> Value {
    let mut obj = Map::new();
    let mut nums = |key: &str, list: &[u16]| {
        if !list.is_empty() {
            obj.insert(key.into(), list.to_vec().into());
        }
    };
    nums("query_type", &rule.query_type);
    nums("source_port", &rule.source_port);
    nums("port", &rule.port);
    let strings: [(&str, &Vec<String>); 14] = [
        ("network", &rule.network),
        ("domain", &rule.domain),
        ("domain_suffix", &rule.domain_suffix),
        ("domain_keyword", &rule.domain_keyword),
        ("domain_regex", &rule.domain_regex),
        ("source_port_range", &rule.source_port_range),
        ("port_range", &rule.port_range),
        ("process_name", &rule.process_name),
        ("process_path", &rule.process_path),
        ("process_path_regex", &rule.process_path_regex),
        ("package_name", &rule.package_name),
        ("package_name_regex", &rule.package_name_regex),
        ("wifi_ssid", &rule.wifi_ssid),
        ("wifi_bssid", &rule.wifi_bssid),
    ];
    for (key, list) in strings {
        if !list.is_empty() {
            obj.insert(key.into(), list.clone().into());
        }
    }
    for (key, list) in [
        ("source_ip_cidr", &rule.source_ip_cidr),
        ("ip_cidr", &rule.ip_cidr),
    ] {
        if !list.is_empty() {
            obj.insert(
                key.into(),
                list.iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .into(),
            );
        }
    }
    if rule.invert {
        obj.insert("invert".into(), true.into());
    }
    Value::Object(obj)
}
