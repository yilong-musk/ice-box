// SPDX-License-Identifier: GPL-3.0-or-later

//! Validation for user-added custom route rules.
//!
//! Shared by the desktop and mobile hosts so a rule accepted on one platform
//! cannot be rejected (or emitted verbatim into the runtime config) on the
//! other. The caller maps the returned message to its platform error type.

use serde_json::Value;

use crate::profile::NormalizedProfile;

/// Validate a custom rule before it is persisted.
///
/// `profile` is `None` when no active profile is available; rule-set
/// references are only checked when one is.
pub fn validate_custom_rule_value(
    rule: &Value,
    profile: Option<&NormalizedProfile>,
) -> Result<(), String> {
    if !rule.is_object() {
        return Err("custom rule must be a JSON object".into());
    }
    if rule.get("outbound").and_then(Value::as_str).is_none() {
        return Err(
            "custom rule must reference an outbound (e.g. \"outbound\": \"direct\")".into(),
        );
    }
    // sing-box 1.13 removed the `geoip` / `geosite` rule options; custom rules
    // are emitted verbatim into the runtime config, so these matchers would
    // make sing-box exit FATAL on the next reload. Only subscription rules are
    // geoip-expanded.
    for key in ["geoip", "geosite"] {
        if rule.get(key).is_some() {
            return Err(format!(
                "custom rule cannot use the `{key}` matcher (sing-box 1.13 removed it); use `rule_set` instead"
            ));
        }
    }
    // Validate `rule_set` references against the active profile's rule-sets so
    // a bad reference is caught here instead of failing every config build.
    if let Some(profile) = profile {
        let set_tags: Vec<&str> = profile
            .route
            .rule_sets
            .iter()
            .filter_map(|set| set.get("tag").and_then(Value::as_str))
            .collect();
        if let Some(refs) = rule.get("rule_set").and_then(Value::as_array) {
            for reference in refs {
                if let Some(tag) = reference.as_str() {
                    if !set_tags.contains(&tag) {
                        return Err(format!("custom rule references unknown rule_set: {tag}"));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::validate_custom_rule_value;
    use crate::NormalizedProfile;

    fn profile_with_rule_set() -> NormalizedProfile {
        let mut profile = NormalizedProfile::from_nodes_only(Vec::new());
        profile.route.rule_sets = vec![json!({ "tag": "cn", "type": "remote" })];
        profile
    }

    #[test]
    fn requires_an_object_and_an_outbound() {
        assert!(validate_custom_rule_value(&json!([]), None).is_err());
        assert_eq!(
            validate_custom_rule_value(&json!({ "domain": ["a.com"] }), None).unwrap_err(),
            "custom rule must reference an outbound (e.g. \"outbound\": \"direct\")"
        );
        assert!(validate_custom_rule_value(
            &json!({ "domain": ["a.com"], "outbound": "direct" }),
            None
        )
        .is_ok());
    }

    #[test]
    fn rejects_removed_geoip_and_geosite_matchers() {
        for key in ["geoip", "geosite"] {
            let mut rule = json!({ "outbound": "direct" });
            rule[key] = json!(["cn"]);
            let err = validate_custom_rule_value(&rule, None).unwrap_err();
            assert!(err.contains("sing-box 1.13 removed it"), "{err}");
        }
    }

    #[test]
    fn validates_rule_set_tags_against_the_active_profile() {
        let profile = profile_with_rule_set();
        assert!(validate_custom_rule_value(
            &json!({ "rule_set": ["cn"], "outbound": "direct" }),
            Some(&profile)
        )
        .is_ok());

        let err = validate_custom_rule_value(
            &json!({ "rule_set": ["missing"], "outbound": "direct" }),
            Some(&profile),
        )
        .unwrap_err();
        assert_eq!(err, "custom rule references unknown rule_set: missing");
    }
}
