// SPDX-License-Identifier: GPL-3.0-or-later

//! Download, sanitize and cache remote rule-sets.
//!
//! Runs in the unprivileged app. Each URL is fetched with the subscription
//! fetcher (SSRF checks on every hop, deadline, size cap), sanitized by
//! `ice-rule-set` into canonical source JSON, and written atomically to
//! `<cache>/<sha256(url)>.json` with a `.meta.json` sidecar. A failed fetch
//! or a rejected rule-set never removes an existing cached copy.

use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::fetch::HttpFetcher;

/// How long a cached rule-set is used before it is checked again.
pub const RULE_SET_REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// How long an unused cached rule-set is kept.
pub const RULE_SET_CACHE_MAX_IDLE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Sidecar metadata of one cached rule-set.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuleSetCacheMeta {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    /// Unix seconds of the last successful download of new content.
    pub updated_at: u64,
    /// Unix seconds of the last successful check (200 or 304).
    pub checked_at: u64,
    pub rules: usize,
    pub items: usize,
    /// Why the last attempt failed, cleared by the next success. A sidecar
    /// can exist with only this set, before anything was ever downloaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Unix seconds of the last failed attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_at: Option<u64>,
}

/// Result of refreshing one URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleSetRefresh {
    /// New content was downloaded and cached.
    Updated,
    /// The server answered 304; the cached copy stays.
    NotModified,
    /// The cached copy is younger than the refresh interval; not fetched.
    Fresh,
    /// The fetch or sanitization failed; any cached copy stays.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleSetRefreshReport {
    pub url: String,
    pub outcome: RuleSetRefresh,
}

impl RuleSetRefreshReport {
    /// Whether the cache content changed, so configs should be rebuilt.
    pub fn changed(&self) -> bool {
        self.outcome == RuleSetRefresh::Updated
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Read the sidecar of a cached URL, when both files exist.
pub fn read_rule_set_cache_meta(cache_dir: &Path, url: &str) -> Option<RuleSetCacheMeta> {
    if !ice_config::rule_set_cache_path(cache_dir, url).is_file() {
        return None;
    }
    read_meta_file(cache_dir, url)
}

/// Read the sidecar alone; it may describe only a failed attempt.
fn read_meta_file(cache_dir: &Path, url: &str) -> Option<RuleSetCacheMeta> {
    let raw = std::fs::read(ice_config::rule_set_cache_meta_path(cache_dir, url)).ok()?;
    let meta: RuleSetCacheMeta = serde_json::from_slice(&raw).ok()?;
    (meta.url == url).then_some(meta)
}

/// Note a failed attempt in the sidecar, keeping any cached copy's fields.
fn record_failure(cache_dir: &Path, url: &str, message: &str) {
    let mut meta = read_meta_file(cache_dir, url).unwrap_or_else(|| RuleSetCacheMeta {
        url: url.to_string(),
        ..RuleSetCacheMeta::default()
    });
    // Bound what a server can make us persist.
    meta.last_error = Some(message.chars().take(300).collect());
    meta.failed_at = Some(now_secs());
    if std::fs::create_dir_all(cache_dir).is_ok() {
        let _ = write_meta(cache_dir, url, &meta);
    }
}

/// Refresh every URL in `urls`. Cached copies checked within `max_age` are
/// skipped unless `force`.
pub fn refresh_rule_sets<F: HttpFetcher + ?Sized>(
    fetcher: &F,
    cache_dir: &Path,
    urls: &[String],
    max_age: Duration,
    force: bool,
) -> Vec<RuleSetRefreshReport> {
    urls.iter()
        .map(|url| RuleSetRefreshReport {
            url: url.clone(),
            outcome: refresh_one(fetcher, cache_dir, url, max_age, force),
        })
        .collect()
}

fn refresh_one<F: HttpFetcher + ?Sized>(
    fetcher: &F,
    cache_dir: &Path,
    url: &str,
    max_age: Duration,
    force: bool,
) -> RuleSetRefresh {
    if !ice_config::is_downloadable_rule_set_url(url) {
        return RuleSetRefresh::Failed("rule-set URL must be https".into());
    }
    let outcome = refresh_downloadable(fetcher, cache_dir, url, max_age, force);
    if let RuleSetRefresh::Failed(message) = &outcome {
        record_failure(cache_dir, url, message);
    }
    outcome
}

fn refresh_downloadable<F: HttpFetcher + ?Sized>(
    fetcher: &F,
    cache_dir: &Path,
    url: &str,
    max_age: Duration,
    force: bool,
) -> RuleSetRefresh {
    let cached = read_rule_set_cache_meta(cache_dir, url);
    let now = now_secs();
    if let Some(meta) = &cached {
        if !force && now.saturating_sub(meta.checked_at) < max_age.as_secs() {
            return RuleSetRefresh::Fresh;
        }
    }
    let (etag, last_modified) = cached.as_ref().map_or((None, None), |m| {
        (m.etag.as_deref(), m.last_modified.as_deref())
    });
    let redacted = crate::redact_subscription_url_for_log(url);
    let resp = match fetcher.get_bytes(url, etag, last_modified) {
        Ok(resp) => resp,
        Err(err) => {
            let msg = err.redacted_display().to_string();
            tracing::warn!(url = %redacted, error = %msg, "rule-set download failed");
            return RuleSetRefresh::Failed(msg);
        }
    };
    if resp.not_modified {
        if let Some(mut meta) = cached {
            meta.checked_at = now;
            meta.last_error = None;
            meta.failed_at = None;
            if let Err(err) = write_meta(cache_dir, url, &meta) {
                return RuleSetRefresh::Failed(err);
            }
            return RuleSetRefresh::NotModified;
        }
        // A 304 without a cached copy (lost files): nothing usable.
        return RuleSetRefresh::Failed("server answered 304 but nothing is cached".into());
    }
    let sanitized = match ice_rule_set::sanitize(&resp.body, &ice_rule_set::Limits::default()) {
        Ok(sanitized) => sanitized,
        Err(err) => {
            tracing::warn!(url = %redacted, error = %err, "downloaded rule-set rejected");
            return RuleSetRefresh::Failed(err.to_string());
        }
    };
    if let Err(err) = std::fs::create_dir_all(cache_dir) {
        return RuleSetRefresh::Failed(format!("create rule-set cache dir: {err}"));
    }
    let path = ice_config::rule_set_cache_path(cache_dir, url);
    let unchanged = std::fs::read(&path).is_ok_and(|old| old == sanitized.json);
    if !unchanged {
        if let Err(err) = ice_config::write_bytes_atomic(&path, &sanitized.json) {
            return RuleSetRefresh::Failed(format!("write rule-set cache: {err}"));
        }
    }
    let meta = RuleSetCacheMeta {
        url: url.to_string(),
        etag: resp.etag,
        last_modified: resp.last_modified,
        updated_at: if unchanged {
            cached.map_or(now, |m| m.updated_at)
        } else {
            now
        },
        checked_at: now,
        rules: sanitized.stats.rules,
        items: sanitized.stats.items,
        last_error: None,
        failed_at: None,
    };
    if let Err(err) = write_meta(cache_dir, url, &meta) {
        return RuleSetRefresh::Failed(err);
    }
    tracing::info!(url = %redacted, items = meta.items, changed = !unchanged, "rule-set cached");
    if unchanged {
        RuleSetRefresh::NotModified
    } else {
        RuleSetRefresh::Updated
    }
}

/// Where a rule-set the profile needs currently comes from, for the UI.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RuleSetStatus {
    pub tag: String,
    /// `download` (a subscription's remote set), `geo` (a `geoip` /
    /// `geosite` code), `bundled` or `local`.
    pub source: &'static str,
    /// Download URL without query or credentials.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// `downloaded` (cached copy in use), `bundled` (bundled file in use),
    /// `pending` (nothing usable yet; its rules are skipped) or `local`.
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Status of every rule-set a profile needs: its `route.rule_set` entries
/// plus the `geoip` / `geosite` codes its route, DNS and custom rules use.
/// `rules` should include custom rules. Sorted by tag.
pub fn rule_set_statuses(
    rule_sets: &[serde_json::Value],
    rules: &[serde_json::Value],
    dns: Option<&serde_json::Value>,
    bundled_dir: Option<&Path>,
    cache_dir: &Path,
) -> Vec<RuleSetStatus> {
    use serde_json::Value;
    let bundled_present = |name: &str| {
        ice_config::is_bundled_rule_set(name)
            && bundled_dir.is_some_and(|dir| dir.join(format!("{name}.srs")).is_file())
    };
    let status = |tag: &str, source: &'static str, url: Option<&str>, fallback: Option<&str>| {
        let cached = url.and_then(|url| read_rule_set_cache_meta(cache_dir, url));
        let meta = url.and_then(|url| read_meta_file(cache_dir, url));
        let state = if cached.is_some() {
            "downloaded"
        } else if fallback.is_some_and(bundled_present) {
            "bundled"
        } else {
            "pending"
        };
        RuleSetStatus {
            tag: tag.to_string(),
            source,
            url: url.map(crate::redact_subscription_url_for_ui),
            state,
            updated_at: cached.as_ref().map(|m| m.updated_at),
            checked_at: cached.as_ref().map(|m| m.checked_at),
            items: cached.as_ref().map(|m| m.items),
            last_error: meta.and_then(|m| m.last_error),
        }
    };
    let mut out: Vec<RuleSetStatus> = Vec::new();
    for set in rule_sets {
        let Some(tag) = set.get("tag").and_then(Value::as_str) else {
            continue;
        };
        let entry = match set.get("type").and_then(Value::as_str) {
            Some(ice_config::DOWNLOAD_RULE_SET_TYPE) => status(
                tag,
                "download",
                ice_config::download_rule_set_url(set),
                ice_config::download_rule_set_fallback(set),
            ),
            Some(ice_config::BUNDLED_RULE_SET_TYPE) => {
                let name = ice_config::bundled_rule_set_name(set);
                RuleSetStatus {
                    tag: tag.to_string(),
                    source: "bundled",
                    url: None,
                    state: if name.is_some_and(bundled_present) {
                        "bundled"
                    } else {
                        "pending"
                    },
                    updated_at: None,
                    checked_at: None,
                    items: None,
                    last_error: None,
                }
            }
            _ => RuleSetStatus {
                tag: tag.to_string(),
                source: "local",
                url: None,
                state: "local",
                updated_at: None,
                checked_at: None,
                items: None,
                last_error: None,
            },
        };
        out.push(entry);
    }
    let dns_rules = dns
        .and_then(|d| d.get("rules"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut names: Vec<String> = Vec::new();
    for rule in rules.iter().chain(dns_rules) {
        for kind in ["geoip", "geosite"] {
            let codes: Vec<&str> = match rule.get(kind) {
                Some(Value::String(code)) => vec![code.as_str()],
                Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
                _ => Vec::new(),
            };
            names.extend(codes.into_iter().map(|code| format!("{kind}-{code}")));
        }
    }
    names.sort();
    names.dedup();
    for name in names {
        if out.iter().any(|s| s.tag == name) {
            continue;
        }
        if bundled_present(&name) {
            out.push(status(&name, "geo", None, Some(&name)));
            continue;
        }
        let url = ice_config::upstream_url_for_name(&name);
        out.push(status(&name, "geo", url.as_deref(), None));
    }
    out.sort_by(|a, b| a.tag.cmp(&b.tag));
    out
}

fn write_meta(cache_dir: &Path, url: &str, meta: &RuleSetCacheMeta) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(meta).map_err(|e| e.to_string())?;
    ice_config::write_bytes_atomic(
        &ice_config::rule_set_cache_meta_path(cache_dir, url),
        &bytes,
    )
    .map_err(|e| format!("write rule-set cache meta: {e}"))
}

/// Remove cached rule-sets that are not in `keep` and were not checked
/// within `max_idle` (sets of an inactive subscription survive a while, so
/// switching back does not download them again). Only files named like
/// cache entries (`<64 hex>.json` / `<64 hex>.meta.json`) are touched.
pub fn prune_rule_set_cache(cache_dir: &Path, keep: &[String], max_idle: Duration) -> usize {
    let keep: HashSet<String> = keep
        .iter()
        .filter_map(|url| {
            ice_config::rule_set_cache_path(cache_dir, url)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
        })
        .collect();
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return 0;
    };
    let now = now_secs();
    let idle = |stem: &str| {
        let meta = std::fs::read(cache_dir.join(format!("{stem}.meta.json")))
            .ok()
            .and_then(|raw| serde_json::from_slice::<RuleSetCacheMeta>(&raw).ok());
        meta.is_none_or(|m| now.saturating_sub(m.checked_at) >= max_idle.as_secs())
    };
    let mut stale = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let stem = name
            .strip_suffix(".meta.json")
            .or_else(|| name.strip_suffix(".json"));
        let Some(stem) = stem else {
            continue;
        };
        if stem.len() != 64 || !stem.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        if !keep.contains(stem) && idle(stem) {
            stale.push(entry.path());
        }
    }
    // Decide first, then delete, so a removed sidecar cannot make its data
    // file look idle halfway through.
    stale
        .into_iter()
        .filter(|path| std::fs::remove_file(path).is_ok())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::{FetchBytes, FetchClient, FetchResponse};
    use crate::SubscriptionError;
    use std::sync::Mutex;

    /// Serves fixed bytes, or 304 when the request carries the served etag.
    struct BytesFetcher {
        body: Vec<u8>,
        etag: Option<String>,
        calls: Mutex<usize>,
        fail: bool,
    }

    impl BytesFetcher {
        fn new(body: &[u8]) -> Self {
            Self {
                body: body.to_vec(),
                etag: Some("\"v1\"".into()),
                calls: Mutex::new(0),
                fail: false,
            }
        }
    }

    impl HttpFetcher for BytesFetcher {
        fn bypasses_system_proxy(&self) -> bool {
            true
        }

        fn get_as(
            &self,
            _url: &str,
            _etag: Option<&str>,
            _last_modified: Option<&str>,
            _client: FetchClient,
        ) -> Result<FetchResponse, SubscriptionError> {
            unreachable!("rule-set downloads use get_bytes")
        }

        fn get_bytes(
            &self,
            _url: &str,
            etag: Option<&str>,
            _last_modified: Option<&str>,
        ) -> Result<FetchBytes, SubscriptionError> {
            *self.calls.lock().unwrap() += 1;
            if self.fail {
                return Err(SubscriptionError::FetchFailed("offline".into()));
            }
            let not_modified = etag.is_some() && etag == self.etag.as_deref();
            Ok(FetchBytes {
                body: if not_modified {
                    Vec::new()
                } else {
                    self.body.clone()
                },
                not_modified,
                etag: self.etag.clone(),
                last_modified: None,
            })
        }
    }

    const URL: &str = "https://example.org/rules/a.srs";
    const SET: &[u8] = br#"{"version":1,"rules":[{"domain_suffix":["b.com","a.com"]}]}"#;

    #[test]
    fn downloads_sanitizes_and_caches() {
        let dir = tempfile::tempdir().unwrap();
        let fetcher = BytesFetcher::new(SET);
        let urls = vec![URL.to_string()];
        let reports = refresh_rule_sets(
            &fetcher,
            dir.path(),
            &urls,
            RULE_SET_REFRESH_INTERVAL,
            false,
        );
        assert_eq!(reports[0].outcome, RuleSetRefresh::Updated);
        let cached = std::fs::read(ice_config::rule_set_cache_path(dir.path(), URL)).unwrap();
        assert_eq!(
            cached,
            br#"{"rules":[{"domain_suffix":["a.com","b.com"]}],"version":1}"#
        );
        let meta = read_rule_set_cache_meta(dir.path(), URL).unwrap();
        assert_eq!(meta.items, 2);
        assert_eq!(meta.etag.as_deref(), Some("\"v1\""));

        // Within the interval: not fetched again.
        let reports = refresh_rule_sets(
            &fetcher,
            dir.path(),
            &urls,
            RULE_SET_REFRESH_INTERVAL,
            false,
        );
        assert_eq!(reports[0].outcome, RuleSetRefresh::Fresh);
        assert_eq!(*fetcher.calls.lock().unwrap(), 1);

        // Forced: conditional request answered with 304.
        let reports =
            refresh_rule_sets(&fetcher, dir.path(), &urls, RULE_SET_REFRESH_INTERVAL, true);
        assert_eq!(reports[0].outcome, RuleSetRefresh::NotModified);
    }

    #[test]
    fn rejected_content_keeps_the_old_copy() {
        let dir = tempfile::tempdir().unwrap();
        let urls = vec![URL.to_string()];
        refresh_rule_sets(
            &BytesFetcher::new(SET),
            dir.path(),
            &urls,
            Duration::ZERO,
            true,
        );
        let before = std::fs::read(ice_config::rule_set_cache_path(dir.path(), URL)).unwrap();

        let mut hostile = BytesFetcher::new(br#"{"version":1,"rules":[{"ip_cidr":"0.0.0.0/0"}]}"#);
        hostile.etag = Some("\"v2\"".into());
        let reports = refresh_rule_sets(&hostile, dir.path(), &urls, Duration::ZERO, true);
        assert!(matches!(reports[0].outcome, RuleSetRefresh::Failed(_)));

        let mut offline = BytesFetcher::new(SET);
        offline.fail = true;
        let reports = refresh_rule_sets(&offline, dir.path(), &urls, Duration::ZERO, true);
        assert!(matches!(reports[0].outcome, RuleSetRefresh::Failed(_)));

        let after = std::fs::read(ice_config::rule_set_cache_path(dir.path(), URL)).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn non_https_urls_are_not_fetched() {
        let dir = tempfile::tempdir().unwrap();
        let fetcher = BytesFetcher::new(SET);
        let urls = vec!["http://example.org/a.srs".to_string()];
        let reports = refresh_rule_sets(&fetcher, dir.path(), &urls, Duration::ZERO, true);
        assert!(matches!(reports[0].outcome, RuleSetRefresh::Failed(_)));
        assert_eq!(*fetcher.calls.lock().unwrap(), 0);
    }

    #[test]
    fn prune_removes_only_unlisted_cache_entries() {
        let dir = tempfile::tempdir().unwrap();
        let fetcher = BytesFetcher::new(SET);
        let a = URL.to_string();
        let b = "https://example.org/rules/b.srs".to_string();
        refresh_rule_sets(
            &fetcher,
            dir.path(),
            &[a.clone(), b.clone()],
            Duration::ZERO,
            true,
        );
        std::fs::write(dir.path().join("notes.json"), b"{}").unwrap();
        // Recently checked: kept even though unlisted.
        assert_eq!(
            prune_rule_set_cache(
                dir.path(),
                std::slice::from_ref(&a),
                Duration::from_secs(3600)
            ),
            0
        );
        assert_eq!(
            prune_rule_set_cache(dir.path(), std::slice::from_ref(&a), Duration::ZERO),
            2
        );
        assert!(read_rule_set_cache_meta(dir.path(), &a).is_some());
        assert!(read_rule_set_cache_meta(dir.path(), &b).is_none());
        assert!(dir.path().join("notes.json").exists());
    }

    #[test]
    fn statuses_report_where_each_set_comes_from() {
        let cache = tempfile::tempdir().unwrap();
        let bundled = tempfile::tempdir().unwrap();
        std::fs::write(bundled.path().join("geosite-cn.srs"), b"srs").unwrap();
        let urls = vec![URL.to_string()];
        refresh_rule_sets(
            &BytesFetcher::new(SET),
            cache.path(),
            &urls,
            Duration::ZERO,
            true,
        );
        let mut offline = BytesFetcher::new(SET);
        offline.fail = true;
        let failing = "https://example.org/rules/b.srs".to_string();
        refresh_rule_sets(
            &offline,
            cache.path(),
            std::slice::from_ref(&failing),
            Duration::ZERO,
            true,
        );

        let sets = vec![
            serde_json::json!({"type": "download", "tag": "a", "url": URL}),
            serde_json::json!({"type": "download", "tag": "b", "url": failing}),
            serde_json::json!({"type": "download", "tag": "c", "url": "https://x.org/geosite-cn.srs?token=s3cret", "bundled": "geosite-cn"}),
            serde_json::json!({"type": "local", "tag": "d", "path": "x.srs"}),
        ];
        let rules =
            vec![serde_json::json!({"geosite": ["cn", "category-ru"], "outbound": "direct"})];
        let statuses = rule_set_statuses(&sets, &rules, None, Some(bundled.path()), cache.path());
        let by_tag = |tag: &str| statuses.iter().find(|s| s.tag == tag).unwrap();
        assert_eq!(by_tag("a").state, "downloaded");
        assert_eq!(by_tag("a").items, Some(2));
        assert_eq!(by_tag("b").state, "pending");
        assert!(by_tag("b")
            .last_error
            .as_deref()
            .unwrap()
            .contains("offline"));
        assert_eq!(by_tag("c").state, "bundled");
        assert!(!by_tag("c").url.as_deref().unwrap().contains("s3cret"));
        assert_eq!(by_tag("d").state, "local");
        assert_eq!(by_tag("geosite-cn").state, "bundled");
        assert_eq!(by_tag("geosite-category-ru").state, "pending");
        assert_eq!(statuses.len(), 6);

        // A later success clears the recorded error.
        refresh_rule_sets(
            &BytesFetcher::new(SET),
            cache.path(),
            std::slice::from_ref(&failing),
            Duration::ZERO,
            true,
        );
        let statuses = rule_set_statuses(&sets, &rules, None, Some(bundled.path()), cache.path());
        let b = statuses.iter().find(|s| s.tag == "b").unwrap();
        assert_eq!(b.state, "downloaded");
        assert!(b.last_error.is_none());
    }

    /// Real network: `cargo test -p ice-subscription live_rule_set_download -- --ignored`.
    #[test]
    #[ignore = "network"]
    fn live_rule_set_download() {
        let dir = tempfile::tempdir().unwrap();
        let url = ice_config::upstream_url_for_name("geosite-category-ru").unwrap();
        let reports = refresh_rule_sets(
            &crate::DirectFetcher,
            dir.path(),
            std::slice::from_ref(&url),
            Duration::ZERO,
            true,
        );
        assert_eq!(reports[0].outcome, RuleSetRefresh::Updated, "{reports:?}");
        let meta = read_rule_set_cache_meta(dir.path(), &url).unwrap();
        assert!(meta.items > 100, "{meta:?}");
    }
}
