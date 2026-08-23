//! Resolve Clash policy / proxy names against known tags.

use std::collections::HashSet;

/// Known sing-box special outbound tags.
pub const SPECIAL_TARGETS: &[&str] = &["DIRECT", "REJECT", "REJECT-DROP", "PASS"];

pub fn normalize_clash_target(name: &str) -> String {
    match name.to_ascii_uppercase().as_str() {
        "DIRECT" => "direct".into(),
        "REJECT" | "REJECT-DROP" => "block".into(),
        "PASS" => "direct".into(),
        _ => name.to_string(),
    }
}

pub fn resolve_member(name: &str, known: &HashSet<String>) -> Option<String> {
    let upper = name.to_ascii_uppercase();
    if SPECIAL_TARGETS.contains(&upper.as_str()) {
        return Some(normalize_clash_target(name));
    }
    if known.contains(name) {
        return Some(name.to_string());
    }
    None
}

pub fn detect_default_outbound(group_names: &[String]) -> Option<String> {
    for candidate in ["Proxies", "Final", "Proxy", "GLOBAL"] {
        if group_names.iter().any(|g| g == candidate) {
            return Some(candidate.to_string());
        }
    }
    group_names.first().cloned()
}
