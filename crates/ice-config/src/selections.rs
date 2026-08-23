//! Persisted per-group member selections (`group-selections.json`).
//!
//! User picks on a strategy group take effect live via Clash API when the core is
//! running, and are applied as selector `default`s at config build time so they
//! survive restarts, reloads and subscription re-application.

use crate::atomic::write_json_atomic;
use crate::{AppError, ErrorCode};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub type GroupSelections = HashMap<String, String>;

/// Load selections; missing / corrupt file degrades to empty.
pub fn load_group_selections(path: &Path) -> GroupSelections {
    let Ok(raw) = fs::read_to_string(path) else {
        return GroupSelections::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_group_selections(path: &Path, selections: &GroupSelections) -> Result<(), AppError> {
    write_json_atomic(path, selections).map_err(|e| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("save group selections: {e}"),
        )
    })
}

/// Apply persisted selections as `default` on selector outbounds during config build.
/// Only members of the group are honored; stale selections are ignored.
pub fn apply_group_selections(outbounds: &mut [Value], selections: &GroupSelections) {
    for ob in outbounds.iter_mut() {
        let Some(tag) = ob.get("tag").and_then(|v| v.as_str()) else {
            continue;
        };
        if ob.get("type").and_then(|v| v.as_str()) != Some("selector") {
            continue;
        }
        let Some(member) = selections.get(tag) else {
            continue;
        };
        let members: Vec<&str> = ob
            .get("outbounds")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        if members.contains(&member.as_str()) {
            ob.as_object_mut()
                .unwrap()
                .insert("default".into(), Value::String(member.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn save_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-selections-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("group-selections.json");
        let mut sel = GroupSelections::new();
        sel.insert("Proxies".into(), "HK".into());
        save_group_selections(&path, &sel).unwrap();
        let loaded = load_group_selections(&path);
        assert_eq!(loaded.get("Proxies").map(String::as_str), Some("HK"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_loads_empty() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-selections-missing-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(load_group_selections(&dir.join("none.json")).is_empty());
    }

    #[test]
    fn apply_overrides_selector_default_only_for_members() {
        let mut outbounds = vec![
            serde_json::json!({
                "type": "selector",
                "tag": "Proxies",
                "outbounds": ["HK", "JP", "direct"],
                "default": "JP",
            }),
            serde_json::json!({
                "type": "selector",
                "tag": "YouTube",
                "outbounds": ["Proxies", "direct"],
                "default": "Proxies",
            }),
            serde_json::json!({"type": "urltest", "tag": "auto", "outbounds": ["HK", "JP"]}),
        ];
        let mut sel = GroupSelections::new();
        sel.insert("Proxies".into(), "HK".into());
        sel.insert("YouTube".into(), "nope".into());
        sel.insert("auto".into(), "JP".into());
        apply_group_selections(&mut outbounds, &sel);

        assert_eq!(outbounds[0]["default"], "HK");
        assert_eq!(
            outbounds[1]["default"], "Proxies",
            "non-member must be ignored"
        );
        assert!(
            outbounds[2].get("default").is_none(),
            "urltest must be untouched"
        );
    }
}
