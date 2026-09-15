// SPDX-License-Identifier: GPL-3.0-or-later

//! Provider-reported subscription info: the `subscription-userinfo` response
//! header, and the usage / expiry entries a provider embeds in the proxy list.

use ice_config::NormalizedOutbound;
use serde::{Deserialize, Serialize};

/// Traffic and expiry metadata a provider reports for a subscription:
/// `subscription-userinfo: upload=1; download=2; total=3; expire=1735689600`.
///
/// `total == 0` means the provider reports no quota (unlimited); the same
/// convention is used by other clients reading this header. Counters are byte
/// values. `expire` is `None` when the header omits it, sends it empty, or
/// sends `0` — panels that use `0` mean "no expiry", not the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SubscriptionUserInfo {
    #[serde(default)]
    pub upload: u64,
    #[serde(default)]
    pub download: u64,
    #[serde(default)]
    pub total: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expire: Option<i64>,
}

impl SubscriptionUserInfo {
    /// Bytes counted against the quota: `upload + download`.
    pub fn used(&self) -> u64 {
        self.upload.saturating_add(self.download)
    }

    /// Remaining quota, `None` when the provider reports no quota.
    pub fn remaining(&self) -> Option<u64> {
        (self.total > 0).then(|| self.total.saturating_sub(self.used()))
    }
}

/// Expiry values above this are read as milliseconds: a Unix timestamp in
/// seconds would be a date past year 5138, so a panel sending milliseconds
/// (13 digits instead of 10) is normalized instead of shown as a year-57000
/// expiry.
const MILLISECOND_EPOCH_THRESHOLD: u64 = 100_000_000_000;

/// Parse the `subscription-userinfo` header. Unknown keys, malformed pairs and
/// unusable values are ignored; `None` when no field carries a usable value
/// (an empty header, or only `expire=0`).
pub fn parse_userinfo_header(raw: &str) -> Option<SubscriptionUserInfo> {
    let mut info = SubscriptionUserInfo::default();
    let mut seen = false;

    for pair in raw.split(';') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "upload" => seen |= assign_counter(&mut info.upload, value),
            "download" => seen |= assign_counter(&mut info.download, value),
            "total" => seen |= assign_counter(&mut info.total, value),
            "expire" => {
                if let Some(expire) = parse_expire(value) {
                    info.expire = Some(expire);
                    seen = true;
                }
            }
            _ => {}
        }
    }

    seen.then_some(info)
}

fn assign_counter(slot: &mut u64, raw: &str) -> bool {
    match raw.parse::<u64>() {
        Ok(value) => {
            *slot = value;
            true
        }
        Err(_) => false,
    }
}

fn parse_expire(raw: &str) -> Option<i64> {
    let value = raw.parse::<u64>().ok()?;
    if value == 0 {
        return None;
    }
    let seconds = if value > MILLISECOND_EPOCH_THRESHOLD {
        value / 1000
    } else {
        value
    };
    i64::try_from(seconds).ok()
}

/// Longest name kept as provider info; longer ones are treated as node names.
const MAX_INFO_LINE_CHARS: usize = 160;

/// Most info entries kept per subscription.
const MAX_INFO_LINES: usize = 6;

/// Label/value separators providers use in info entries.
const INFO_SEPARATORS: [char; 4] = [':', '：', '|', '｜'];

/// Labels that mark an entry as provider info rather than a proxy. They are
/// matched case-insensitively against the text before the first separator, so
/// ordinary node names (`🇺🇸美国01|流媒体`) are not mistaken for info.
const INFO_LABELS: [&str; 25] = [
    "剩余流量",
    "流量",
    "剩余",
    "到期",
    "过期",
    "套餐",
    "重置",
    "距离下次",
    "官网",
    "客服",
    "订阅",
    "续费",
    "traffic",
    "expire",
    "expires",
    "expiry",
    "used",
    "remaining",
    "reset",
    "quota",
    "available",
    "website",
    "official",
    "subscribe",
    "subscription",
];

/// Info entries a provider embeds in the proxy list, in list order.
///
/// Panels publish quota and expiry as extra "proxies" whose names carry the
/// text (`Traffic: 11.84 GB | 150 GB`, `剩余流量：1023.64 GB`). Each panel words
/// and scales them its own way, so the names are stored as provider input for
/// consumers to parse into a usage / expiry readout.
pub fn provider_info_lines(nodes: &[NormalizedOutbound]) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for node in nodes {
        if lines.len() == MAX_INFO_LINES {
            break;
        }
        let name = node.tag.trim();
        if name.is_empty()
            || name.chars().count() > MAX_INFO_LINE_CHARS
            || !is_provider_info_name(name)
            || lines.iter().any(|line| line == name)
        {
            continue;
        }
        lines.push(name.to_string());
    }
    lines
}

fn is_provider_info_name(name: &str) -> bool {
    let mut parts = name.split(INFO_SEPARATORS);
    let label = parts.next().unwrap_or_default().trim();
    // Info entries are `label: value`; a name without a separator is a proxy
    // that merely mentions a keyword ("美国-流量专线 1").
    if parts.next().is_none() {
        return false;
    }
    // A label is one short word ("剩余流量", "Expire"); a proxy name that
    // happens to contain a keyword before a separator is longer.
    if label.is_empty() || label.chars().count() > 24 {
        return false;
    }
    let label = label.to_lowercase();
    INFO_LABELS.iter().any(|keyword| label.contains(keyword))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_fields() {
        let info = parse_userinfo_header(
            "upload=7118713; download=383477544; total=1099511627776; expire=1790000000",
        )
        .expect("header with counters");
        assert_eq!(info.upload, 7_118_713);
        assert_eq!(info.download, 383_477_544);
        assert_eq!(info.total, 1_099_511_627_776);
        assert_eq!(info.expire, Some(1_790_000_000));
        assert_eq!(info.used(), 390_596_257);
        assert_eq!(info.remaining(), Some(1_099_511_627_776 - 390_596_257));
    }

    #[test]
    fn empty_expire_means_no_expiry() {
        let info = parse_userinfo_header("upload=1; download=2; total=3; expire=")
            .expect("counters are usable");
        assert_eq!(info.expire, None);
    }

    #[test]
    fn zero_expire_means_no_expiry() {
        let info = parse_userinfo_header("upload=1; total=0; expire=0").expect("upload=1");
        assert_eq!(info.expire, None);
        assert_eq!(info.total, 0);
        assert_eq!(
            info.remaining(),
            None,
            "total=0 is unlimited, not a 0 quota"
        );
    }

    #[test]
    fn tolerates_spacing_case_and_unknown_keys() {
        let info =
            parse_userinfo_header(" Upload = 10 ;foo=bar; DOWNLOAD=20 ;total = 30 ;expire=40")
                .expect("pairs");
        assert_eq!(info.upload, 10);
        assert_eq!(info.download, 20);
        assert_eq!(info.total, 30);
        assert_eq!(info.expire, Some(40));
    }

    #[test]
    fn malformed_values_are_ignored() {
        let info = parse_userinfo_header("upload=abc; download=7; total=-5; expire=soon")
            .expect("download is usable");
        assert_eq!(info.upload, 0);
        assert_eq!(info.download, 7);
        assert_eq!(info.total, 0);
        assert_eq!(info.expire, None);
    }

    #[test]
    fn unusable_headers_are_none() {
        assert!(parse_userinfo_header("").is_none());
        assert!(parse_userinfo_header("upload=abc; download=xyz").is_none());
        assert!(
            parse_userinfo_header("expire=0").is_none(),
            "expire=0 carries no usable field"
        );
    }

    #[test]
    fn millisecond_expire_is_normalized() {
        let info = parse_userinfo_header("expire=1790000000000").expect("expire in ms");
        assert_eq!(info.expire, Some(1_790_000_000));
    }

    fn node(tag: &str) -> NormalizedOutbound {
        NormalizedOutbound {
            tag: tag.into(),
            outbound: std::sync::Arc::new(serde_json::json!({
                "type": "trojan",
                "tag": tag,
                "server": "example.com",
                "server_port": 443,
            })),
        }
    }

    #[test]
    fn collects_embedded_info_entries_in_order() {
        let nodes = vec![
            node("Traffic: 11.84 GB | 150 GB"),
            node("Expire: 2026-09-26"),
            node("🇭🇰 香港实验性 IEPL 专线 1"),
            node("剩余流量：1023.64 GB"),
            node("套餐到期：长期有效"),
        ];
        assert_eq!(
            provider_info_lines(&nodes),
            vec![
                "Traffic: 11.84 GB | 150 GB",
                "Expire: 2026-09-26",
                "剩余流量：1023.64 GB",
                "套餐到期：长期有效",
            ]
        );
    }

    #[test]
    fn keeps_ordinary_node_names_out_of_provider_info() {
        let nodes = vec![
            node("🇺🇸美国01|流媒体|0.1x"),
            node("美国-流量专线 1"),
            node("🇯🇵 日本标准 IEPL 专线 1"),
            node("Expire-Me-Not"),
        ];
        assert!(provider_info_lines(&nodes).is_empty());
    }

    #[test]
    fn provider_info_is_deduped_and_capped() {
        let nodes: Vec<_> = (0..MAX_INFO_LINES + 2)
            .map(|i| node(&format!("剩余流量：{i} GB")))
            .chain(std::iter::once(node("剩余流量：0 GB")))
            .collect();
        let lines = provider_info_lines(&nodes);
        assert_eq!(lines.len(), MAX_INFO_LINES);
        assert_eq!(lines[0], "剩余流量：0 GB");
        let long = node(&format!("Traffic: {}", "9".repeat(MAX_INFO_LINE_CHARS)));
        assert!(provider_info_lines(&[long]).is_empty());
    }
}
