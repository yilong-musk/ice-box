//! Built-in default routing rules for rule-less (URI list) subscriptions.
//!
//! A share-link subscription carries only nodes, so routing is attached here:
//! private IPs and Chinese destinations go direct, everything else follows the
//! selected node. The domain list is a hand-curated static tier; swapping it
//! for a bundled `geosite-cn` rule-set later only changes `default_rules()`.

use ice_config::NormalizedProfile;
use serde_json::{json, Value};

/// Common Chinese service domain suffixes routed direct.
///
/// `domain_suffix` matches label boundaries, so a bare domain also covers all
/// of its subdomains. `.cn` covers every `*.cn` / `*.com.cn` host.
const CN_DOMAIN_SUFFIXES: &[&str] = &[
    // TLD and public infrastructure
    "cn",
    // Tencent
    "qq.com",
    "qzone.com",
    "wechat.com",
    "weixin.qq.com",
    "tencent.com",
    "weiyun.com",
    "gtimg.cn",
    "qpic.cn",
    "qlogo.cn",
    "tcdn.qq.com",
    // Baidu
    "baidu.com",
    "bdstatic.com",
    "bdimg.com",
    "hao123.com",
    // Alibaba
    "taobao.com",
    "tmall.com",
    "1688.com",
    "alicdn.com",
    "alibaba.com",
    "aliyun.com",
    "aliyuncs.com",
    "alipay.com",
    "amap.com",
    "dingtalk.com",
    "cainiao.com",
    // JD
    "jd.com",
    "360buyimg.com",
    // NetEase
    "163.com",
    "126.com",
    "netease.com",
    // Sina / Weibo
    "sina.com",
    "sina.com.cn",
    "sinaimg.cn",
    "weibo.com",
    "weibo.cn",
    // Sohu / Phoenix
    "sohu.com",
    "sohucs.com",
    "ifeng.com",
    // Bilibili
    "bilibili.com",
    "biliimg.com",
    "hdslb.com",
    "biligame.com",
    // ByteDance
    "bytedance.com",
    "toutiao.com",
    "douyin.com",
    "douyinpic.com",
    "iesdouyin.com",
    "feishu.cn",
    "larkoffice.com",
    // Kuaishou
    "kuaishou.com",
    "gifshow.com",
    // Zhihu / Douban / JianShu
    "zhihu.com",
    "zhimg.com",
    "douban.com",
    "jianshu.com",
    // News / media / government
    "xinhuanet.com",
    "people.com.cn",
    "cctv.com",
    "cntv.cn",
    "china.com.cn",
    "thepaper.cn",
    "guancha.cn",
    "caixin.com",
    "yicai.com",
    "eastmoney.com",
    "xueqiu.com",
    "jrj.com.cn",
    "cnstock.com",
    "stcn.com",
    "12377.cn",
    // Video / live streaming
    "iqiyi.com",
    "qiyi.com",
    "iqiyipic.com",
    "youku.com",
    "tudou.com",
    "mgtv.com",
    "pptv.com",
    "douyu.com",
    "huya.com",
    "zhanqi.tv",
    "hupu.com",
    // Music
    "kugou.com",
    "kuwo.cn",
    "xiami.com",
    "changba.com",
    // Travel
    "12306.cn",
    "ctrip.com",
    "qunar.com",
    "mafengwo.cn",
    "maoyan.com",
    "damai.cn",
    // Lifestyle / local services
    "meituan.com",
    "dianping.com",
    "ele.me",
    "didi.com",
    "xiaojukeji.com",
    "hellobike.com",
    "xiaohongshu.com",
    "xhscdn.com",
    "smzdm.com",
    // Finance / banking
    "unionpay.com",
    "icbc.com.cn",
    "ccb.com",
    "cmbchina.com",
    "boc.cn",
    "abchina.com",
    "bankcomm.com",
    "citicbank.com",
    "spdb.com.cn",
    "cebbank.com",
    "cib.com.cn",
    "pingan.com",
    // Devices / vendors
    "mi.com",
    "xiaomi.com",
    "miui.com",
    "oppo.com",
    "vivo.com",
    "oneplus.com",
    "meizu.com",
    "huawei.com",
    "hicloud.com",
    "vmall.com",
    "honor.cn",
    "lenovo.com.cn",
    "zte.com.cn",
    // Security / search / portal
    "360.cn",
    "qihoo.com",
    "sogou.com",
    "so.com",
    "2345.com",
    // Software / developer community
    "csdn.net",
    "cnblogs.com",
    "juejin.cn",
    "51cto.com",
    "oschina.net",
    "gitee.com",
    "wps.cn",
    "kingsoft.com",
    // Cloud / CDN
    "myqcloud.com",
    "qcloud.com",
    "jdcloud.com",
    "ksyun.com",
    "upyun.com",
    "qiniu.com",
    "qiniucdn.com",
    "wangsu.com",
    "chinacache.com",
    // Car / IT media
    "autohome.com.cn",
    "yiche.com",
    "zol.com.cn",
    "ithome.com",
    "mydrivers.com",
    // Jobs / enterprise info
    "zhipin.com",
    "liepin.com",
    "lagou.com",
    "51job.com",
    "qichacha.com",
    "tianyancha.com",
    // Other common services
    "xunlei.com",
    "sandai.net",
    "yy.com",
    "kuaidi100.com",
    "sf-express.com",
    "sto.cn",
    "yto.net.cn",
    "zto.com",
    "dxy.cn",
    "weather.com.cn",
    "cnzz.com",
];

/// Default routing rules for rule-less subscriptions, in match order.
pub fn default_uri_list_rules() -> Vec<Value> {
    vec![
        json!({
            "ip_is_private": true,
            "outbound": "direct",
        }),
        json!({
            "geoip": ["cn"],
            "outbound": "direct",
        }),
        json!({
            "domain_suffix": CN_DOMAIN_SUFFIXES,
            "outbound": "direct",
        }),
    ]
}

/// Default DNS block for rule-less subscriptions: Chinese domains resolve via
/// a domestic server, everything else via a remote DoH through the given
/// `detour` (anti-pollution). The `local` server backs
/// `route.default_domain_resolver`.
///
/// Windows (design note tun-windows-t0 §1.2): no `local` server (it re-enters
/// the TUN via the adapter DNS), UDP upstreams rewritten to DoT (the core's
/// UDP outbound is captured by its own TUN), and `ipv4_only` (the IPv6 path
/// is broken, #4178). `route.default_domain_resolver` then resolves via the
/// `remote-dns` final tag (wired by ice-config).
pub fn default_uri_list_dns(detour: &str) -> Value {
    #[cfg(target_os = "windows")]
    {
        json!({
            "servers": [
                { "type": "tls", "tag": "cn-dns", "server": "223.5.5.5", "server_port": 853 },
                {
                    "type": "https",
                    "tag": "remote-dns",
                    "server": "1.1.1.1",
                    "server_port": 443,
                    "path": "/dns-query",
                    "detour": detour,
                },
            ],
            "rules": [
                { "domain_suffix": CN_DOMAIN_SUFFIXES, "server": "cn-dns" },
            ],
            "final": "remote-dns",
            "strategy": "ipv4_only",
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        json!({
            "servers": [
                { "type": "local", "tag": "local" },
                { "type": "udp", "tag": "cn-dns", "server": "223.5.5.5", "server_port": 53 },
                {
                    "type": "https",
                    "tag": "remote-dns",
                    "server": "1.1.1.1",
                    "server_port": 443,
                    "path": "/dns-query",
                    "detour": detour,
                },
            ],
            "rules": [
                { "domain_suffix": CN_DOMAIN_SUFFIXES, "server": "cn-dns" },
            ],
            "final": "remote-dns",
            "strategy": "prefer_ipv4",
        })
    }
}

/// Attach the built-in split-routing defaults to a profile that carries no
/// rules of its own. No-op when the profile already has rules; the profile's
/// own DNS block (if any) is kept.
///
/// Flat (group-less) profiles route through the injected `proxy` selector so
/// node selection takes effect; grouped profiles keep their top group as the
/// fallback.
pub fn apply_builtin_default_rules(profile: &mut NormalizedProfile) {
    if !profile.route.rules.is_empty() {
        return;
    }

    let fallback = profile
        .default_outbound
        .clone()
        .or_else(|| profile.groups.first().map(|g| g.tag.clone()))
        .unwrap_or_else(|| "proxy".into());
    let final_tag = if profile.groups.is_empty() {
        "proxy".to_string()
    } else {
        fallback
    };

    profile.route.rules = default_uri_list_rules();
    profile.route.final_outbound = final_tag.clone();
    if profile.dns.is_none() {
        profile.dns = Some(default_uri_list_dns(&final_tag));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ice_config::{NormalizedOutbound, ProfileParseStats};

    #[test]
    fn default_rules_shape() {
        let rules = default_uri_list_rules();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0]["ip_is_private"], true);
        assert_eq!(rules[0]["outbound"], "direct");
        assert_eq!(rules[1]["geoip"][0], "cn");
        assert_eq!(rules[2]["outbound"], "direct");
        let domains = rules[2]["domain_suffix"].as_array().unwrap();
        assert!(domains.len() > 100, "static list must be meaningful");
        assert!(domains.iter().any(|d| d == "qq.com"));
    }

    #[test]
    fn default_dns_shape() {
        let dns = default_uri_list_dns("proxy");
        let servers = dns["servers"].as_array().unwrap();
        let tags: Vec<&str> = servers.iter().filter_map(|s| s["tag"].as_str()).collect();
        assert!(tags.contains(&"local"));
        assert!(tags.contains(&"cn-dns"));
        assert!(tags.contains(&"remote-dns"));
        assert_eq!(dns["final"], "remote-dns");
        let rules = dns["rules"].as_array().unwrap();
        assert_eq!(rules[0]["server"], "cn-dns");
        let remote = servers.iter().find(|s| s["tag"] == "remote-dns").unwrap();
        assert_eq!(remote["detour"], "proxy");
    }

    fn profile(nodes: usize, groups: usize, rules: usize, dns: bool) -> NormalizedProfile {
        let mut p = NormalizedProfile {
            nodes: (0..nodes)
                .map(|i| NormalizedOutbound {
                    tag: format!("n{i}"),
                    outbound: json!({"type": "socks", "tag": format!("n{i}")}),
                })
                .collect(),
            groups: (0..groups)
                .map(|i| NormalizedOutbound {
                    tag: format!("g{i}"),
                    outbound: json!({"type": "selector", "tag": format!("g{i}")}),
                })
                .collect(),
            route: Default::default(),
            dns: if dns {
                Some(json!({"servers": []}))
            } else {
                None
            },
            default_outbound: (groups > 0).then(|| "g0".into()),
            parse_stats: ProfileParseStats::default(),
        };
        for _ in 0..rules {
            p.route
                .rules
                .push(json!({"domain": ["x"], "outbound": "direct"}));
        }
        p
    }

    #[test]
    fn apply_defaults_to_flat_ruleless_profile() {
        let mut p = profile(2, 0, 0, false);
        apply_builtin_default_rules(&mut p);
        assert_eq!(p.route.rules.len(), 3);
        assert_eq!(p.route.final_outbound, "proxy");
        assert_eq!(p.dns.unwrap()["servers"][2]["detour"], "proxy");
    }

    #[test]
    fn apply_defaults_keeps_existing_rules_and_dns() {
        let mut p = profile(2, 0, 1, true);
        apply_builtin_default_rules(&mut p);
        assert_eq!(p.route.rules.len(), 1, "existing rules win");
        assert!(p.dns.is_some());
    }

    #[test]
    fn apply_defaults_grouped_profile_uses_top_group() {
        let mut p = profile(2, 1, 0, false);
        apply_builtin_default_rules(&mut p);
        assert_eq!(p.route.rules.len(), 3);
        assert_eq!(p.route.final_outbound, "g0");
        assert_eq!(p.dns.unwrap()["servers"][2]["detour"], "g0");
    }
}
