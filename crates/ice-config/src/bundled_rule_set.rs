// SPDX-License-Identifier: GPL-3.0-or-later

//! Rule-sets shipped with the app, and the mapping from well-known remote
//! rule-set URLs onto them.
//!
//! ice-box never downloads a rule-set. A subscription's `remote` rule-set is
//! kept only when its URL names a file the app bundles; it is stored in the
//! profile as a `bundled` marker that carries the file name, not a path, and
//! the config builder resolves the marker to the bundled directory of the
//! running app. Recognition is strict: an exact host and path layout of a
//! known upstream, no query string, binary format, and a name on
//! [`BUNDLED_RULE_SETS`]. Anything else stays unsupported.

use serde_json::{json, Value};

/// Profile rule-set `type` for a bundled file: `{"type": "bundled", "tag":
/// <provider tag>, "name": "geosite-cn"}`.
pub const BUNDLED_RULE_SET_TYPE: &str = "bundled";

/// File stems of the bundled `.srs` rule-sets, kept in sync with
/// `scripts/fetch-geoip.sh` (a test compares it with the fetched directory).
pub const BUNDLED_RULE_SETS: &[&str] = &[
    "geoip-ae",
    "geoip-au",
    "geoip-br",
    "geoip-ca",
    "geoip-ch",
    "geoip-cn",
    "geoip-de",
    "geoip-es",
    "geoip-fr",
    "geoip-gb",
    "geoip-hk",
    "geoip-id",
    "geoip-il",
    "geoip-in",
    "geoip-it",
    "geoip-jp",
    "geoip-kr",
    "geoip-mx",
    "geoip-nl",
    "geoip-ph",
    "geoip-pl",
    "geoip-ru",
    "geoip-se",
    "geoip-sg",
    "geoip-th",
    "geoip-tr",
    "geoip-tw",
    "geoip-ua",
    "geoip-us",
    "geoip-vn",
    "geosite-anthropic",
    "geosite-apple",
    "geosite-bilibili",
    "geosite-category-ads-all",
    "geosite-category-ai-!cn",
    "geosite-cn",
    "geosite-disney",
    "geosite-facebook",
    "geosite-geolocation-!cn",
    "geosite-geolocation-cn",
    "geosite-github",
    "geosite-google",
    "geosite-microsoft",
    "geosite-netflix",
    "geosite-openai",
    "geosite-private",
    "geosite-spotify",
    "geosite-steam",
    "geosite-telegram",
    "geosite-tiktok",
    "geosite-twitter",
    "geosite-youtube",
];

/// Canonical upstream of the bundled GeoIP files (`<code>.srs`), used when a
/// profile is shared as a portable document.
pub const GEOIP_RULE_SET_BASE: &str =
    "https://raw.githubusercontent.com/Loyalsoldier/geoip/release/srs";
/// Canonical upstream of the bundled GeoSite files.
pub const GEOSITE_RULE_SET_BASE: &str =
    "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set";

/// jsDelivr hosts that serve the same GitHub content.
const JSDELIVR_HOSTS: &[&str] = &[
    "cdn.jsdelivr.net",
    "fastly.jsdelivr.net",
    "gcore.jsdelivr.net",
    "testingcf.jsdelivr.net",
];

/// Whether `name` is a bundled rule-set file stem.
pub fn is_bundled_rule_set(name: &str) -> bool {
    BUNDLED_RULE_SETS.contains(&name)
}

/// The bundled file stem of a `bundled` profile rule-set, when it names one.
pub fn bundled_rule_set_name(set: &Value) -> Option<&str> {
    if set.get("type").and_then(Value::as_str) != Some(BUNDLED_RULE_SET_TYPE) {
        return None;
    }
    set.get("name")
        .and_then(Value::as_str)
        .filter(|name| is_bundled_rule_set(name))
}

/// Rewrite a subscription's `remote` rule-set onto a bundled file. Returns
/// the `bundled` marker, keeping the provider's tag, or `None` when the set
/// is not a recognized remote copy of a bundled file.
pub fn bundled_marker_for_remote(set: &Value) -> Option<Value> {
    let obj = set.as_object()?;
    if obj.get("type").and_then(Value::as_str) != Some("remote") {
        return None;
    }
    if obj
        .get("format")
        .and_then(Value::as_str)
        .is_some_and(|format| format != "binary")
    {
        return None;
    }
    let tag = obj.get("tag").and_then(Value::as_str)?;
    let url = obj.get("url").and_then(Value::as_str)?;
    let name = bundled_name_for_url(url)?;
    Some(json!({ "type": BUNDLED_RULE_SET_TYPE, "tag": tag, "name": name }))
}

/// The portable remote form of a `bundled` profile rule-set, pointing at the
/// canonical upstream of the bundled file.
pub fn portable_remote_for_bundled(set: &Value) -> Option<Value> {
    let name = bundled_rule_set_name(set)?;
    let tag = set.get("tag").and_then(Value::as_str)?;
    Some(json!({
        "type": "remote",
        "tag": tag,
        "format": "binary",
        "url": upstream_url(name),
    }))
}

/// Upstream URL of a bundled file stem (`geoip-cn`, `geosite-netflix`).
pub(crate) fn upstream_url(name: &str) -> String {
    match name.strip_prefix("geoip-") {
        Some(code) => format!("{GEOIP_RULE_SET_BASE}/{code}.srs"),
        None => format!("{GEOSITE_RULE_SET_BASE}/{name}.srs"),
    }
}

/// The bundled file stem a remote rule-set URL is a copy of.
///
/// Recognized layouts, on `raw.githubusercontent.com`, `github.com/<o>/<r>/raw`
/// and the jsDelivr GitHub mirrors:
/// - `SagerNet/sing-geosite`, branch `rule-set`, file
///   `geosite-<category>.srs`.
/// - `Loyalsoldier/geoip`, branch `release`, file `srs/<code>.srs` for a
///   two-letter country code, bundled as `geoip-<code>`.
///
/// Only the sources the bundled files come from are recognized; another
/// GeoIP source (SagerNet/sing-geoip, MetaCubeX) is not mapped, as its
/// ranges differ.
pub fn bundled_name_for_url(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    if rest.contains(['?', '#', '\\']) || rest.contains("..") || rest.contains("%") {
        return None;
    }
    let (host, path) = rest.split_once('/')?;
    let host = host.to_ascii_lowercase();
    // Normalize to `<owner>/<repo>/<branch>/<file path>`.
    let segments: Vec<&str> = path.split('/').collect();
    let normalized: Vec<&str> = match host.as_str() {
        "raw.githubusercontent.com" => segments,
        "github.com" => match segments.as_slice() {
            [owner, repo, "raw", rest @ ..] => [&[*owner, *repo][..], rest].concat(),
            _ => return None,
        },
        host if JSDELIVR_HOSTS.contains(&host) => match segments.as_slice() {
            ["gh", owner, repo_ref, rest @ ..] => {
                let (repo, branch) = repo_ref.split_once('@')?;
                [&[*owner, repo, branch][..], rest].concat()
            }
            _ => return None,
        },
        _ => return None,
    };
    let name = match normalized.as_slice() {
        ["SagerNet", "sing-geosite", "rule-set", file] => {
            let stem = file.strip_suffix(".srs")?;
            stem.starts_with("geosite-").then(|| stem.to_string())?
        }
        ["Loyalsoldier", "geoip", "release", "srs", file] => {
            let code = file.strip_suffix(".srs")?;
            if code.len() != 2 || !code.bytes().all(|b| b.is_ascii_lowercase()) {
                return None;
            }
            format!("geoip-{code}")
        }
        _ => return None,
    };
    is_bundled_rule_set(&name).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_upstream_layouts_of_bundled_files() {
        for (url, name) in [
            (
                "https://testingcf.jsdelivr.net/gh/SagerNet/sing-geosite@rule-set/geosite-cn.srs",
                "geosite-cn",
            ),
            (
                "https://cdn.jsdelivr.net/gh/SagerNet/sing-geosite@rule-set/geosite-category-ads-all.srs",
                "geosite-category-ads-all",
            ),
            (
                "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-geolocation-!cn.srs",
                "geosite-geolocation-!cn",
            ),
            (
                "https://github.com/Loyalsoldier/geoip/raw/release/srs/us.srs",
                "geoip-us",
            ),
            (
                "https://testingcf.jsdelivr.net/gh/Loyalsoldier/geoip@release/srs/cn.srs",
                "geoip-cn",
            ),
            (
                "https://raw.githubusercontent.com/Loyalsoldier/geoip/release/srs/jp.srs",
                "geoip-jp",
            ),
        ] {
            assert_eq!(bundled_name_for_url(url).as_deref(), Some(name), "{url}");
        }
    }

    #[test]
    fn rejects_anything_else() {
        for url in [
            // Not bundled.
            "https://cdn.jsdelivr.net/gh/SagerNet/sing-geosite@rule-set/geosite-bahamut.srs",
            "https://testingcf.jsdelivr.net/gh/Loyalsoldier/geoip@release/srs/private.srs",
            // Not the bundled GeoIP source.
            "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-cn.srs",
            "https://testingcf.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@sing/geo/geoip/cn.srs",
            // Wrong branch, repo, prefix or format.
            "https://cdn.jsdelivr.net/gh/SagerNet/sing-geosite@main/geosite-cn.srs",
            "https://cdn.jsdelivr.net/gh/SagerNet/sing-geosite@rule-set/geoip-cn.srs",
            "https://cdn.jsdelivr.net/gh/someone/sing-geosite@rule-set/geosite-cn.srs",
            "https://cdn.jsdelivr.net/gh/SagerNet/sing-geosite@rule-set/geosite-cn.json",
            // Wrong scheme, host, extra segments, query or escapes.
            "http://cdn.jsdelivr.net/gh/SagerNet/sing-geosite@rule-set/geosite-cn.srs",
            "https://ghproxy.example/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.srs",
            "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/x/geosite-cn.srs",
            "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.srs?v=1",
            "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-%63n.srs",
            "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/../geosite-cn.srs",
        ] {
            assert_eq!(bundled_name_for_url(url), None, "{url}");
        }
    }

    #[test]
    fn marker_keeps_the_provider_tag_and_requires_binary_remote() {
        let set = json!({
            "type": "remote", "tag": "cn-site", "format": "binary",
            "url": "https://cdn.jsdelivr.net/gh/SagerNet/sing-geosite@rule-set/geosite-cn.srs",
            "download_detour": "proxy"
        });
        let marker = bundled_marker_for_remote(&set).expect("marker");
        assert_eq!(
            marker,
            json!({ "type": "bundled", "tag": "cn-site", "name": "geosite-cn" })
        );
        assert_eq!(bundled_rule_set_name(&marker), Some("geosite-cn"));

        let mut source = set.clone();
        source["format"] = json!("source");
        assert_eq!(bundled_marker_for_remote(&source), None);
        let mut local = set;
        local["type"] = json!("local");
        assert_eq!(bundled_marker_for_remote(&local), None);
    }

    #[test]
    fn a_marker_with_an_unlisted_name_is_ignored() {
        let marker = json!({ "type": "bundled", "tag": "x", "name": "../../etc/passwd" });
        assert_eq!(bundled_rule_set_name(&marker), None);
        assert_eq!(portable_remote_for_bundled(&marker), None);
    }

    #[test]
    fn portable_form_points_at_the_canonical_upstream() {
        let marker = json!({ "type": "bundled", "tag": "cn-ip", "name": "geoip-cn" });
        assert_eq!(
            portable_remote_for_bundled(&marker).unwrap(),
            json!({
                "type": "remote", "tag": "cn-ip", "format": "binary",
                "url": "https://raw.githubusercontent.com/Loyalsoldier/geoip/release/srs/cn.srs"
            })
        );
    }

    #[test]
    fn the_list_matches_the_fetched_directory() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../third_party/sing-geoip/rule-set");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .expect("run scripts/fetch-geoip.sh")
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                name.strip_suffix(".srs").map(str::to_string)
            })
            .collect();
        on_disk.sort();
        let mut listed: Vec<String> = BUNDLED_RULE_SETS.iter().map(|s| s.to_string()).collect();
        listed.sort();
        assert_eq!(
            listed, on_disk,
            "update BUNDLED_RULE_SETS with fetch-geoip.sh"
        );
    }
}
