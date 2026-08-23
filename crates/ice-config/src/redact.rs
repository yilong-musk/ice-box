//! Redact sensitive fields before exposing runtime config to the UI.

use serde_json::Value;

const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "pass",
    "uuid",
    "private_key",
    "private-key",
    "psk",
    "pre_shared_key",
    "token",
    "secret",
    "credentials",
    "access_key",
    "secret_key",
    "short_id",
    "auth_str",
    "auth",
    "obfs_password",
    "ech_key",
    "seed",
];

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.replace('-', "_");
    SENSITIVE_KEYS
        .iter()
        .any(|s| normalized.eq_ignore_ascii_case(s))
}

/// Walk `value` in place and replace known secret fields with `"***"`.
pub fn redact_config_json(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if is_sensitive_key(key) {
                    *val = Value::String("***".into());
                } else {
                    redact_config_json(val);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_config_json(item);
            }
        }
        _ => {}
    }
}

/// Parse JSON config text, redact secrets, return pretty-printed string.
pub fn redact_config_str(raw: &str) -> Result<String, serde_json::Error> {
    let mut value: Value = serde_json::from_str(raw)?;
    redact_config_json(&mut value);
    serde_json::to_string_pretty(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_nested_outbound_secrets() {
        let mut cfg: Value = serde_json::from_str(
            r#"{
            "outbounds": [{
                "type": "vmess",
                "tag": "n1",
                "uuid": "real-uuid",
                "server": "1.2.3.4"
            }]
        }"#,
        )
        .unwrap();
        redact_config_json(&mut cfg);
        assert_eq!(cfg["outbounds"][0]["uuid"], "***");
        assert_eq!(cfg["outbounds"][0]["server"], "1.2.3.4");
    }

    #[test]
    fn redacts_hyphenated_and_alias_secret_keys() {
        let mut cfg: Value = serde_json::from_str(
            r#"{
            "outbounds": [{
                "type": "trojan",
                "tag": "n1",
                "pass": "secret-pass",
                "private-key": "secret-key",
                "auth_str": "secret-auth"
            }]
        }"#,
        )
        .unwrap();
        redact_config_json(&mut cfg);
        let ob = &cfg["outbounds"][0];
        assert_eq!(ob["pass"], "***");
        assert_eq!(ob["private-key"], "***");
        assert_eq!(ob["auth_str"], "***");
    }
}
