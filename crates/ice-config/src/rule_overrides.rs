//! Rule overrides: disabled subscription rules + user-added custom rules.
//!
//! Persisted at `rules.json` in the app data dir (architecture §6). Disabled rules are
//! keyed by a stable fingerprint (canonical JSON of the rule object), so the state
//! survives subscription updates / profile switches as long as the rule content is
//! unchanged. Overrides apply at config build time only; subscription bytes are never
//! modified.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::atomic::write_json_atomic;
use crate::ConfigError;

/// Matcher keys recognized as the rule "type" for classification (priority order).
/// First present key wins; a rule with no known matcher classifies as `other`.
pub const RULE_TYPE_KEYS: &[&str] = &[
    "domain",
    "domain_suffix",
    "domain_keyword",
    "domain_regex",
    "ip_cidr",
    "ip_is_private",
    "source_ip_cidr",
    "source_ip_is_private",
    "rule_set",
    "geoip",
    "geosite",
    "port",
    "source_port",
    "network",
    "protocol",
    "process_name",
    "process_path",
    "package_name",
    "inbound",
    "wifi_ssid",
    "wifi_bssid",
    "clash_mode",
    "user",
];

/// Stable identity of a rule: its canonical JSON serialization.
pub fn rule_fingerprint(rule: &Value) -> String {
    serde_json::to_string(rule).unwrap_or_default()
}

/// Classify a rule by its first recognized matcher key.
pub fn rule_type_of(rule: &Value) -> &'static str {
    let Some(obj) = rule.as_object() else {
        return "other";
    };
    for key in RULE_TYPE_KEYS {
        if obj.contains_key(*key) {
            return key;
        }
    }
    "other"
}

/// User-level rule overrides (architecture §14.5).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleOverrides {
    /// Fingerprints of rules skipped at config build time.
    #[serde(default)]
    pub disabled: BTreeSet<String>,
    /// User-added rules, prepended ahead of subscription rules at build time.
    #[serde(default)]
    pub custom: Vec<Value>,
}

impl RuleOverrides {
    pub fn is_disabled(&self, fingerprint: &str) -> bool {
        self.disabled.contains(fingerprint)
    }

    pub fn set_disabled(&mut self, fingerprint: String, disabled: bool) {
        if disabled {
            self.disabled.insert(fingerprint);
        } else {
            self.disabled.remove(&fingerprint);
        }
    }

    pub fn remove_custom(&mut self, fingerprint: &str) {
        self.custom
            .retain(|rule| rule_fingerprint(rule) != fingerprint);
        self.disabled.remove(fingerprint);
    }
}

/// Load `rules.json`; a missing file yields the default (empty) overrides.
pub fn load_rule_overrides(path: &Path) -> RuleOverrides {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => RuleOverrides::default(),
    }
}

pub fn save_rule_overrides(path: &Path, overrides: &RuleOverrides) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_json_atomic(path, overrides)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "ice-box-overrides-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn fingerprint_is_canonical_json() {
        let rule = json!({ "domain_suffix": ["a.com"], "outbound": "direct" });
        let rule2 = json!({ "outbound": "direct", "domain_suffix": ["a.com"] });
        assert_eq!(rule_fingerprint(&rule), rule_fingerprint(&rule2));
    }

    #[test]
    fn rule_type_classifies_by_priority() {
        assert_eq!(
            rule_type_of(&json!({ "domain_suffix": ["x"] })),
            "domain_suffix"
        );
        assert_eq!(rule_type_of(&json!({ "geoip": ["cn"] })), "geoip");
        assert_eq!(rule_type_of(&json!({ "rule_set": ["s"] })), "rule_set");
        assert_eq!(
            rule_type_of(&json!({ "ip_is_private": true })),
            "ip_is_private"
        );
        assert_eq!(rule_type_of(&json!({ "outbound": "direct" })), "other");
        assert_eq!(rule_type_of(&json!("nope")), "other");
    }

    #[test]
    fn set_disabled_toggles_and_remove_custom_cleans_both() {
        let mut o = RuleOverrides::default();
        let rule = json!({ "domain_suffix": ["a.com"], "outbound": "direct" });
        let fp = rule_fingerprint(&rule);

        o.set_disabled(fp.clone(), true);
        assert!(o.is_disabled(&fp));

        o.custom.push(rule);
        assert!(!o.custom.is_empty());
        o.remove_custom(&fp);
        assert!(o.custom.is_empty());
        assert!(
            !o.is_disabled(&fp),
            "removing a custom rule clears its disabled mark"
        );
    }

    #[test]
    fn round_trip_via_disk() {
        let path = temp_file("roundtrip");
        let mut o = RuleOverrides::default();
        o.set_disabled("fp-1".into(), true);
        o.custom
            .push(json!({ "domain": ["example.com"], "outbound": "block" }));
        save_rule_overrides(&path, &o).unwrap();
        let loaded = load_rule_overrides(&path);
        assert!(loaded.is_disabled("fp-1"));
        assert_eq!(loaded.custom.len(), 1);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn missing_file_loads_default() {
        let path = temp_file("missing");
        let loaded = load_rule_overrides(&path);
        assert!(loaded.disabled.is_empty());
        assert!(loaded.custom.is_empty());
        let _ = fs::remove_file(&path);
    }
}
