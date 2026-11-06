// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::Write;
use std::net::IpAddr;
use std::path::PathBuf;

use super::*;

fn bundled_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../third_party/bundled-rule-sets/rule-set")
}

fn bundled(name: &str) -> Vec<u8> {
    std::fs::read(bundled_dir().join(format!("{name}.srs"))).expect("bundled rule-set")
}

fn sanitize_ok(bytes: &[u8]) -> Sanitized {
    sanitize(bytes, &Limits::default()).expect("sanitize")
}

fn sanitize_err(bytes: &[u8]) -> RuleSetError {
    sanitize(bytes, &Limits::default()).expect_err("must be rejected")
}

/// Build an `.srs` file from an uncompressed payload.
fn srs(version: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = b"SRS".to_vec();
    out.push(version);
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(payload).unwrap();
    out.extend(z.finish().unwrap());
    out
}

#[test]
fn binary_matches_sing_box_decompile() {
    // Reference: `sing-box rule-set decompile geosite-openai.srs` (1.14.4).
    let reference = include_bytes!("testdata/geosite-openai.decompiled.json");
    let ours = sanitize_ok(&bundled("geosite-openai"));
    let theirs = sanitize_ok(reference);
    assert_eq!(ours.input_format, InputFormat::Binary);
    assert_eq!(theirs.input_format, InputFormat::Source);
    assert_eq!(ours.rule_set, theirs.rule_set);
    assert_eq!(ours.json, theirs.json);
    assert_eq!(ours.stats.domains, 24);
    assert_eq!(ours.stats.regexes, 1);
}

/// Optional cross-check against a directory of `<name>.srs` files and their
/// `sing-box rule-set decompile` output `<name>.json`.
#[test]
fn binary_matches_reference_dir_when_provided() {
    let Some(dir) = std::env::var_os("ICE_RULE_SET_REFERENCE_DIR") else {
        return;
    };
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("srs") {
            continue;
        }
        let reference = std::fs::read(path.with_extension("json")).unwrap();
        let ours = sanitize_ok(&std::fs::read(&path).unwrap());
        let theirs = sanitize_ok(&reference);
        assert_eq!(ours.json, theirs.json, "{}", path.display());
        checked += 1;
    }
    assert!(checked > 0);
}

#[test]
fn every_bundled_rule_set_passes() {
    let mut count = 0;
    for entry in std::fs::read_dir(bundled_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("srs") {
            continue;
        }
        let out = sanitize(&std::fs::read(&path).unwrap(), &Limits::default())
            .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        assert!(out.stats.items > 0, "{}", path.display());
        count += 1;
    }
    assert!(count >= 50);
}

#[test]
fn canonical_output_is_a_fixed_point() {
    for name in ["geosite-cn", "geoip-jp", "geosite-category-ads-all"] {
        let first = sanitize_ok(&bundled(name));
        let second = sanitize_ok(&first.json);
        assert_eq!(first.json, second.json, "{name}");
    }
}

#[test]
fn source_json_is_normalized() {
    let input = br#"{"version":2,"rules":[{"domain_suffix":["b.com","a.com","a.com"],
        "ip_cidr":["10.1.2.3/8","1.1.1.1"],"port_range":"1000:2000","query_type":["AAAA",1]}]}"#;
    let out = sanitize_ok(input);
    assert_eq!(
        String::from_utf8(out.json).unwrap(),
        r#"{"rules":[{"domain_suffix":["a.com","b.com"],"ip_cidr":["1.1.1.1/32","10.0.0.0/8"],"port_range":["1000:2000"],"query_type":[1,28]}],"version":2}"#
    );
}

#[test]
fn logical_rules_round_trip() {
    let input = br#"{"version":3,"rules":[{"type":"logical","mode":"and","invert":true,
        "rules":[{"domain":"x.com"},{"network":"udp","invert":true}]}]}"#;
    let out = sanitize_ok(input);
    let again = sanitize_ok(&out.json);
    assert_eq!(out.rule_set, again.rule_set);
    let Rule::Logical(logical) = &out.rule_set.rules[0] else {
        panic!("logical")
    };
    assert!(logical.invert);
    assert_eq!(logical.mode, LogicalMode::And);
}

#[test]
fn rejects_unknown_and_unsupported_fields() {
    for input in [
        &br#"{"version":1,"rules":[{"domain":"a.com","outbound":"direct"}]}"#[..],
        br#"{"version":1,"rules":[],"extra":1}"#,
        br#"{"version":1,"rules":[{"network_type":"wifi"}]}"#,
        br#"{"version":9,"rules":[]}"#,
        br#"{"version":1,"rules":[{"type":"remote"}]}"#,
    ] {
        assert!(
            matches!(sanitize_err(input), RuleSetError::Unsupported(_)),
            "{}",
            String::from_utf8_lossy(input)
        );
    }
}

#[test]
fn rejects_invalid_values() {
    for input in [
        &br#"{"version":1,"rules":[{"ip_cidr":"0.0.0.0/0"}]}"#[..],
        br#"{"version":1,"rules":[{"ip_cidr":"2000::/3"}]}"#,
        br#"{"version":1,"rules":[{"ip_cidr":"10.0.0.0/33"}]}"#,
        br#"{"version":1,"rules":[{"ip_cidr":"10.0.0.0/+8"}]}"#,
        br#"{"version":1,"rules":[{"domain_regex":"(unclosed"}]}"#,
        br#"{"version":1,"rules":[{"domain":"a b.com"}]}"#,
        br#"{"version":1,"rules":[{"domain":"a\u0000.com"}]}"#,
        br#"{"version":1,"rules":[{"domain":""}]}"#,
        br#"{"version":1,"rules":[{"port_range":"2000:1000"}]}"#,
        br#"{"version":1,"rules":[{"port_range":":"}]}"#,
        br#"{"version":1,"rules":[{"port":70000}]}"#,
        br#"{"version":1,"rules":[{"network":"sctp"}]}"#,
        br#"{"version":1,"rules":[{}]}"#,
        br#"{"version":1,"rules":[{"type":"logical","mode":"or","rules":[]}]}"#,
    ] {
        assert!(
            matches!(sanitize_err(input), RuleSetError::Invalid(_)),
            "{}",
            String::from_utf8_lossy(input)
        );
    }
}

#[test]
fn accepts_real_geoip_breadth() {
    sanitize_ok(br#"{"version":1,"rules":[{"ip_cidr":["214.0.0.0/7","2630::/16"]}]}"#);
}

#[test]
fn rejects_deep_nesting() {
    let mut rule = r#"{"domain":"a.com"}"#.to_string();
    for _ in 0..20 {
        rule = format!(r#"{{"type":"logical","mode":"or","rules":[{rule}]}}"#);
    }
    let input = format!(r#"{{"version":1,"rules":[{rule}]}}"#);
    assert!(matches!(
        sanitize_err(input.as_bytes()),
        RuleSetError::TooLarge(_)
    ));
}

#[test]
fn rejects_malformed_binary() {
    let good = bundled("geosite-openai");
    // Bad version byte.
    let mut bad_version = good.clone();
    bad_version[3] = 9;
    assert!(matches!(
        sanitize_err(&bad_version),
        RuleSetError::Unsupported(_)
    ));
    // Truncated zlib stream.
    assert!(matches!(
        sanitize_err(&good[..good.len() / 2]),
        RuleSetError::Malformed(_)
    ));
    // A count far beyond the payload must fail before allocating.
    assert!(matches!(
        sanitize_err(&srs(1, &[0xff, 0xff, 0xff, 0xff, 0x0f])),
        RuleSetError::TooLarge(_) | RuleSetError::Malformed(_)
    ));
    // A string list claiming more entries than bytes remain.
    let payload = [1, 0, ITEM_DOMAIN_KEYWORD_FOR_TEST, 0xff, 0xff, 0x03];
    assert!(matches!(
        sanitize_err(&srs(1, &payload)),
        RuleSetError::Malformed(_)
    ));
    // Trailing bytes after the last rule.
    let payload = [
        1,
        0,
        ITEM_DOMAIN_KEYWORD_FOR_TEST,
        1,
        1,
        b'a',
        0xff,
        0,
        0x42,
    ];
    assert!(matches!(
        sanitize_err(&srs(1, &payload)),
        RuleSetError::Malformed(_)
    ));
    // Unknown rule item.
    let payload = [1, 0, 0x77, 0xff, 0];
    assert!(matches!(
        sanitize_err(&srs(1, &payload)),
        RuleSetError::Malformed(_)
    ));
}

#[test]
fn rejects_unsupported_binary_items() {
    // AdGuard domain item.
    let payload = [1, 0, 16, 0xff, 0];
    assert!(matches!(
        sanitize_err(&srs(1, &payload)),
        RuleSetError::Unsupported(_)
    ));
}

#[test]
fn binary_minimal_rule_decodes() {
    // One default rule: domain_keyword ["ab"], invert.
    let payload = [
        1,
        0,
        ITEM_DOMAIN_KEYWORD_FOR_TEST,
        1,
        2,
        b'a',
        b'b',
        0xff,
        1,
    ];
    let out = sanitize_ok(&srs(2, &payload));
    assert_eq!(
        String::from_utf8(out.json).unwrap(),
        r#"{"rules":[{"domain_keyword":["ab"],"invert":true}],"version":2}"#
    );
}

#[test]
fn rejects_zlib_bomb() {
    let limits = Limits {
        max_decompressed_bytes: 1024,
        ..Limits::default()
    };
    let bomb = srs(1, &vec![0u8; 1 << 20]);
    assert!(bomb.len() < 4096);
    assert!(matches!(
        sanitize(&bomb, &limits),
        Err(RuleSetError::TooLarge(_))
    ));
}

#[test]
fn rejects_oversized_input() {
    let limits = Limits {
        max_input_bytes: 16,
        ..Limits::default()
    };
    let input = br#"{"version":1,"rules":[{"domain":"a.com"}]}"#;
    assert!(matches!(
        sanitize(input, &limits),
        Err(RuleSetError::TooLarge(_))
    ));
}

#[test]
fn item_limit_applies() {
    let limits = Limits {
        max_items: 3,
        ..Limits::default()
    };
    let input = br#"{"version":1,"rules":[{"domain":["a","b","c","d"]}]}"#;
    assert!(matches!(
        sanitize(input, &limits),
        Err(RuleSetError::TooLarge(_))
    ));
    assert!(matches!(
        sanitize(&bundled("geosite-openai"), &limits),
        Err(RuleSetError::TooLarge(_))
    ));
}

/// A logical rule with a hostile nested count is rejected against the rule
/// budget before its `Vec<Rule>` is reserved. The nested bytes are garbage,
/// so reaching them would report `Malformed` instead of `TooLarge`.
#[test]
fn nested_rule_count_is_charged_before_allocating() {
    fn uvarint(mut n: u64, out: &mut Vec<u8>) {
        loop {
            let byte = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }
    fn logical_payload(nested: u64) -> Vec<u8> {
        let mut payload = Vec::new();
        uvarint(1, &mut payload); // one top-level rule
        payload.extend([1, 0]); // logical rule, mode AND
        uvarint(nested, &mut payload);
        payload.resize(payload.len() + 2 * nested as usize, 0xEE);
        payload
    }

    // 4M nested rules would reserve ~1.8 GB at `size_of::<Rule>()` each.
    let err = sanitize_err(&srs(1, &logical_payload(4_000_000)));
    assert!(matches!(err, RuleSetError::TooLarge(_)), "{err:?}");

    // The top-level rule already used one slot of the budget.
    let limits = Limits {
        max_rules: 10,
        ..Limits::default()
    };
    let err = sanitize(&srs(1, &logical_payload(10)), &limits).expect_err("over budget");
    assert!(matches!(err, RuleSetError::TooLarge(_)), "{err:?}");
    let err = sanitize(&srs(1, &logical_payload(9)), &limits).expect_err("garbage rules");
    assert!(matches!(err, RuleSetError::Malformed(_)), "{err:?}");
}

const ITEM_DOMAIN_KEYWORD_FOR_TEST: u8 = 3;

#[test]
fn ranges_split_into_minimal_prefixes() {
    let ip = |s: &str| s.parse::<IpAddr>().unwrap();
    let show = |v: Vec<Cidr>| v.iter().map(|c| c.to_string()).collect::<Vec<_>>();
    assert_eq!(
        show(srs::range_to_cidrs_for_test(
            ip("10.0.0.0"),
            ip("10.255.255.255")
        )),
        ["10.0.0.0/8"]
    );
    assert_eq!(
        show(srs::range_to_cidrs_for_test(ip("10.0.0.1"), ip("10.0.0.6"))),
        ["10.0.0.1/32", "10.0.0.2/31", "10.0.0.4/31", "10.0.0.6/32"]
    );
    assert_eq!(
        show(srs::range_to_cidrs_for_test(
            ip("0.0.0.0"),
            ip("255.255.255.255")
        )),
        ["0.0.0.0/0"]
    );
    assert_eq!(
        show(srs::range_to_cidrs_for_test(
            ip("::"),
            ip("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff")
        )),
        ["::/0"]
    );
    assert_eq!(
        show(srs::range_to_cidrs_for_test(
            ip("2001:db8::"),
            ip("2001:db8::1")
        )),
        ["2001:db8::/127"]
    );
}
