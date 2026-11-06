// SPDX-License-Identifier: GPL-3.0-or-later

//! Remote rule-sets that the app downloads itself.
//!
//! The sing-box core never fetches a rule-set (the guard rejects `remote`
//! sets). A subscription's `remote` set is stored in the profile as a
//! `download` marker instead: `{"type": "download", "tag", "url", "format"?,
//! "bundled"?}`. The unprivileged app downloads the URL, sanitizes it with
//! `ice-rule-set` and caches canonical source JSON under the rule-set cache
//! directory; the config builder points the set at that file. When nothing
//! is cached yet, a set that is a copy of a bundled file (`bundled`) falls
//! back to that file.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::bundled_rule_set::{bundled_name_for_url, is_bundled_rule_set, upstream_url};

/// Profile rule-set `type` for an app-downloaded set.
pub const DOWNLOAD_RULE_SET_TYPE: &str = "download";

/// Longest accepted rule-set URL.
const MAX_URL_BYTES: usize = 2048;

/// Whether `url` may be downloaded: `https`, a host, bounded, no
/// whitespace or control characters. SSRF checks happen at fetch time.
pub fn is_downloadable_rule_set_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    url.len() <= MAX_URL_BYTES
        && !rest.is_empty()
        && !rest.starts_with('/')
        && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// Rewrite a subscription's `remote` rule-set into a `download` marker, or
/// `None` when it is not a downloadable remote set.
pub fn download_marker_for_remote(set: &Value) -> Option<Value> {
    let obj = set.as_object()?;
    if obj.get("type").and_then(Value::as_str) != Some("remote") {
        return None;
    }
    let tag = obj.get("tag").and_then(Value::as_str)?;
    let url = obj.get("url").and_then(Value::as_str)?.trim();
    if tag.trim().is_empty() || !is_downloadable_rule_set_url(url) {
        return None;
    }
    let mut marker = json!({ "type": DOWNLOAD_RULE_SET_TYPE, "tag": tag, "url": url });
    if let Some(format) = obj.get("format").and_then(Value::as_str) {
        if matches!(format, "binary" | "source") {
            marker["format"] = json!(format);
        }
    }
    // The bundled fallback must hold the same data: only binary copies of a
    // known upstream file are mapped.
    if obj
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or("binary")
        == "binary"
    {
        if let Some(name) = bundled_name_for_url(url) {
            marker["bundled"] = json!(name);
        }
    }
    Some(marker)
}

/// The URL of a `download` marker.
pub fn download_rule_set_url(set: &Value) -> Option<&str> {
    if set.get("type").and_then(Value::as_str) != Some(DOWNLOAD_RULE_SET_TYPE) {
        return None;
    }
    set.get("url")
        .and_then(Value::as_str)
        .filter(|url| is_downloadable_rule_set_url(url))
}

/// The bundled fallback file stem of a `download` marker.
pub fn download_rule_set_fallback(set: &Value) -> Option<&str> {
    if set.get("type").and_then(Value::as_str) != Some(DOWNLOAD_RULE_SET_TYPE) {
        return None;
    }
    set.get("bundled")
        .and_then(Value::as_str)
        .filter(|name| is_bundled_rule_set(name))
}

/// The portable remote form of a `download` marker, for sharing.
pub fn portable_remote_for_download(set: &Value) -> Option<Value> {
    let url = download_rule_set_url(set)?;
    let tag = set.get("tag").and_then(Value::as_str)?;
    let format = set
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or("binary");
    Some(json!({ "type": "remote", "tag": tag, "format": format, "url": url }))
}

/// URL a legacy `geoip` / `geosite` code downloads from when no bundled
/// file exists: the same upstream the bundled files come from. `name` is a
/// file stem such as `geosite-category-ru` or `geoip-ir`.
pub fn upstream_url_for_name(name: &str) -> Option<String> {
    let valid = |s: &str| {
        !s.is_empty()
            && s.len() <= 128
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_!@.".contains(&b))
            && !s.contains("..")
    };
    if let Some(code) = name.strip_prefix("geoip-") {
        return (code.len() == 2 && code.bytes().all(|b| b.is_ascii_lowercase()))
            .then(|| upstream_url(name));
    }
    let category = name.strip_prefix("geosite-")?;
    valid(category).then(|| upstream_url(name))
}

/// Cache file for a downloaded rule-set URL: `<sha256(url)>.json`.
pub fn rule_set_cache_path(cache_dir: &Path, url: &str) -> PathBuf {
    let digest = Sha256::digest(url.as_bytes());
    let mut name = String::with_capacity(69);
    for byte in digest {
        name.push_str(&format!("{byte:02x}"));
    }
    name.push_str(".json");
    cache_dir.join(name)
}

/// Sidecar metadata path for a cache file (`<hash>.meta.json`).
pub fn rule_set_cache_meta_path(cache_dir: &Path, url: &str) -> PathBuf {
    rule_set_cache_path(cache_dir, url).with_extension("meta.json")
}

/// Legacy matchers the config builder expands into rule-set references:
/// both kinds in route rules, only `geosite` in DNS rules (`geoip` DNS rules
/// are left as they are).
pub(crate) const ROUTE_GEO_KINDS: &[&str] = &["geoip", "geosite"];
pub(crate) const DNS_GEO_KINDS: &[&str] = &["geosite"];

/// How the builder treats a rule's legacy `geoip` / `geosite` matcher.
pub(crate) enum GeoMatcher<'a> {
    /// Nothing to expand; the rule is kept as it is.
    None,
    /// Expand `kind` (`geoip` / `geosite`) with these codes.
    Expand {
        kind: &'static str,
        codes: Vec<&'a str>,
    },
    /// The rule cannot be expanded safely and is dropped.
    Invalid(&'static str),
}

/// Classify `rule` for the builder. `kinds` are the matchers expanded in
/// this rule list. A rule that combines `geoip` with `geosite`, or a geo
/// matcher with an existing `rule_set`, is invalid: one `rule_set` list
/// would turn their AND into an OR.
pub(crate) fn geo_matcher<'a>(rule: &'a Value, kinds: &[&str]) -> GeoMatcher<'a> {
    let (kind, value) = match (rule.get("geoip"), rule.get("geosite")) {
        (None, None) => return GeoMatcher::None,
        (Some(_), Some(_)) => return GeoMatcher::Invalid("geoip+geosite"),
        (Some(value), None) => ("geoip", value),
        (None, Some(value)) => ("geosite", value),
    };
    if !kinds.contains(&kind) {
        return GeoMatcher::None;
    }
    if rule.get("rule_set").is_some() {
        return GeoMatcher::Invalid(if kind == "geoip" {
            "geoip+rule_set"
        } else {
            "geosite+rule_set"
        });
    }
    let codes: Vec<&str> = match value {
        Value::String(code) => vec![code.as_str()],
        Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    if codes.is_empty() {
        return GeoMatcher::Invalid(kind);
    }
    GeoMatcher::Expand { kind, codes }
}

/// Rule-set names (`geoip-ir`, `geosite-category-ru`) the builder resolves
/// for the legacy matchers in `rules` and `dns`. Sorted, without duplicates.
pub fn legacy_geo_rule_set_names(rules: &[Value], dns: Option<&Value>) -> Vec<String> {
    let dns_rules = dns
        .and_then(|d| d.get("rules"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let route = rules.iter().map(|rule| (rule, ROUTE_GEO_KINDS));
    let dns = dns_rules.iter().map(|rule| (rule, DNS_GEO_KINDS));
    let mut names: Vec<String> = route
        .chain(dns)
        .filter_map(|(rule, kinds)| match geo_matcher(rule, kinds) {
            GeoMatcher::Expand { kind, codes } => {
                Some(codes.into_iter().map(move |code| format!("{kind}-{code}")))
            }
            _ => None,
        })
        .flatten()
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Every URL a profile's rule-sets need downloaded: `download` markers plus
/// legacy geo names (see [`legacy_geo_rule_set_names`]) whose file is not in
/// `bundled_dir`. Presence is checked on disk, as the builder does, so a
/// bundled file that is missing is downloaded instead. Sorted, without
/// duplicates.
pub fn required_rule_set_urls(
    rule_sets: &[Value],
    rules: &[Value],
    dns: Option<&Value>,
    bundled_dir: Option<&Path>,
) -> Vec<String> {
    let mut urls: Vec<String> = rule_sets
        .iter()
        .filter_map(download_rule_set_url)
        .map(str::to_string)
        .collect();
    let present = bundled_dir.map(crate::build::bundled_rule_set_names);
    urls.extend(
        legacy_geo_rule_set_names(rules, dns)
            .into_iter()
            .filter(|name| !present.as_ref().is_some_and(|names| names.contains(name)))
            .filter_map(|name| upstream_url_for_name(&name)),
    );
    urls.sort();
    urls.dedup();
    urls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_set_becomes_download_marker_with_bundled_fallback() {
        let set = json!({
            "type": "remote", "tag": "cn", "format": "binary",
            "url": "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.srs",
            "download_detour": "proxy", "update_interval": "1d"
        });
        let marker = download_marker_for_remote(&set).unwrap();
        assert_eq!(marker["type"], DOWNLOAD_RULE_SET_TYPE);
        assert_eq!(marker["tag"], "cn");
        assert_eq!(marker["bundled"], "geosite-cn");
        assert!(marker.get("download_detour").is_none());
        assert_eq!(download_rule_set_fallback(&marker), Some("geosite-cn"));
    }

    #[test]
    fn unknown_https_source_has_no_fallback() {
        let set = json!({
            "type": "remote", "tag": "ru", "format": "source",
            "url": "https://example.org/rules/ru.json"
        });
        let marker = download_marker_for_remote(&set).unwrap();
        assert_eq!(marker["format"], "source");
        assert!(marker.get("bundled").is_none());
        assert_eq!(
            download_rule_set_url(&marker),
            Some("https://example.org/rules/ru.json")
        );
    }

    #[test]
    fn non_https_or_odd_urls_are_not_downloadable() {
        for url in [
            "http://example.org/a.srs",
            "https://",
            "https:///a.srs",
            "https://exa mple.org/a.srs",
            "ftp://example.org/a.srs",
        ] {
            let set = json!({"type": "remote", "tag": "t", "url": url});
            assert!(download_marker_for_remote(&set).is_none(), "{url}");
        }
        assert!(
            download_marker_for_remote(&json!({"type": "local", "tag": "t", "path": "x"}))
                .is_none()
        );
    }

    #[test]
    fn portable_form_restores_remote() {
        let marker = json!({"type": "download", "tag": "t", "url": "https://e.org/a.srs", "bundled": "geosite-cn"});
        assert_eq!(
            portable_remote_for_download(&marker).unwrap(),
            json!({"type": "remote", "tag": "t", "format": "binary", "url": "https://e.org/a.srs"})
        );
    }

    #[test]
    fn cache_paths_are_stable_hashes() {
        let dir = Path::new("/cache");
        let a = rule_set_cache_path(dir, "https://e.org/a.srs");
        assert_eq!(a, rule_set_cache_path(dir, "https://e.org/a.srs"));
        assert_ne!(a, rule_set_cache_path(dir, "https://e.org/b.srs"));
        let name = a.file_name().unwrap().to_str().unwrap();
        assert_eq!(name.len(), 64 + ".json".len());
        assert!(rule_set_cache_meta_path(dir, "https://e.org/a.srs")
            .to_str()
            .unwrap()
            .ends_with(".meta.json"));
    }

    fn bundled_dir(tag: &str, names: &[&str]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ice-required-urls-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in names {
            std::fs::write(dir.join(format!("{name}.srs")), b"x").unwrap();
        }
        dir
    }

    const SAGERNET: &str = "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set";
    const LOYALSOLDIER: &str = "https://raw.githubusercontent.com/Loyalsoldier/geoip/release/srs";

    #[test]
    fn required_urls_cover_markers_and_unbundled_codes() {
        let dir = bundled_dir("cover", &["geoip-cn", "geosite-cn"]);
        let sets = vec![
            json!({"type": "download", "tag": "x", "url": "https://e.org/x.srs"}),
            json!({"type": "bundled", "tag": "cn", "name": "geosite-cn"}),
        ];
        let rules = vec![
            json!({"geosite": ["cn", "category-ru"], "outbound": "direct"}),
            json!({"geoip": "ir", "outbound": "direct"}),
            json!({"geoip": "cn", "outbound": "direct"}),
        ];
        let dns = json!({"rules": [{"geosite": "category-gov-ru", "server": "local"}]});
        let urls = required_rule_set_urls(&sets, &rules, Some(&dns), Some(&dir));
        assert_eq!(
            urls,
            vec![
                "https://e.org/x.srs".to_string(),
                format!("{LOYALSOLDIER}/ir.srs"),
                format!("{SAGERNET}/geosite-category-gov-ru.srs"),
                format!("{SAGERNET}/geosite-category-ru.srs"),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_bundled_file_is_downloaded_instead() {
        // `geosite-cn` is on the bundled list, but its file is not on disk.
        let dir = bundled_dir("missing", &["geoip-cn"]);
        let rules = vec![
            json!({"geosite": "cn", "outbound": "direct"}),
            json!({"geoip": "cn", "outbound": "direct"}),
        ];
        let urls = required_rule_set_urls(&[], &rules, None, Some(&dir));
        assert_eq!(urls, vec![format!("{SAGERNET}/geosite-cn.srs")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_expandable_matchers_need_downloads() {
        let rules = vec![
            // Dropped by the builder: geoip and geosite together, or a geo
            // matcher next to `rule_set`.
            json!({"geoip": "ir", "geosite": "category-ir", "outbound": "direct"}),
            json!({"geosite": "category-ru", "rule_set": "x", "outbound": "direct"}),
        ];
        // `geoip` DNS rules are not expanded.
        let dns = json!({"rules": [{"geoip": "ir", "server": "local"}]});
        assert!(required_rule_set_urls(&[], &rules, Some(&dns), None).is_empty());
        assert!(legacy_geo_rule_set_names(&rules, Some(&dns)).is_empty());
    }

    #[test]
    fn upstream_names_are_restricted() {
        assert!(upstream_url_for_name("geoip-xyz").is_none());
        assert!(upstream_url_for_name("geosite-../x").is_none());
        assert!(upstream_url_for_name("geosite-a/b").is_none());
        assert!(upstream_url_for_name("other-cn").is_none());
        assert!(upstream_url_for_name("geosite-category-ai-!cn").is_some());
    }
}
