//! Clash `rules` → sing-box `route.rules`.

use ice_config::{NormalizedRoute, ProfileParseStats};
use serde_json::{json, Value};

use super::names::normalize_clash_target;

pub const MAX_CLASH_RULES: usize = 10_000;

#[derive(Debug, Clone)]
pub struct RuleParseResult {
    pub route: NormalizedRoute,
    pub stats: ProfileParseStats,
}

pub fn parse_rules(doc: &Value, known_targets: &[String]) -> RuleParseResult {
    let mut stats = ProfileParseStats::default();
    let mut rules = Vec::new();
    let mut final_outbound = String::from("direct");

    let known: std::collections::HashSet<String> = known_targets.iter().cloned().collect();

    let Some(items) = doc.get("rules").and_then(|v| v.as_array()) else {
        final_outbound = guess_final_from_mode(doc);
        return RuleParseResult {
            route: NormalizedRoute {
                rules,
                final_outbound,
                rule_sets: Vec::new(),
            },
            stats,
        };
    };

    for item in items.iter().take(MAX_CLASH_RULES) {
        let Some(line) = item.as_str() else {
            stats.skipped_rules += 1;
            continue;
        };
        match parse_rule_line(line, &known) {
            Ok(RuleOutcome::Rule(rule)) => {
                if let Some(codes) = rule.get("geoip").and_then(|v| v.as_array()) {
                    for code in codes {
                        if let Some(s) = code.as_str() {
                            if !stats.geoip_codes.iter().any(|c| c == s) {
                                stats.geoip_codes.push(s.to_string());
                            }
                        }
                    }
                }
                rules.push(rule)
            }
            Ok(RuleOutcome::Final(outbound)) => final_outbound = outbound,
            Err(RuleSkip::Unsupported(ty)) => {
                stats.skipped_rules += 1;
                if !stats.unsupported_rule_types.contains(&ty) {
                    stats.unsupported_rule_types.push(ty);
                }
            }
            Err(RuleSkip::UnknownTarget(target)) => {
                stats.skipped_rules += 1;
                stats.warnings.push(format!(
                    "rule target {target} does not resolve to any outbound; rule dropped"
                ));
            }
            Err(RuleSkip::Invalid) => stats.skipped_rules += 1,
        }
    }

    if items.len() > MAX_CLASH_RULES {
        stats.warnings.push(format!(
            "rules count {} exceeds limit {MAX_CLASH_RULES}; truncated",
            items.len()
        ));
    }

    RuleParseResult {
        route: NormalizedRoute {
            rules,
            final_outbound,
            rule_sets: Vec::new(),
        },
        stats,
    }
}

#[derive(Debug)]
enum RuleOutcome {
    Rule(Value),
    Final(String),
}

#[derive(Debug)]
enum RuleSkip {
    Unsupported(String),
    UnknownTarget(String),
    Invalid,
}

fn guess_final_from_mode(doc: &Value) -> String {
    doc.get("mode")
        .and_then(|v| v.as_str())
        .map(|m| match m.to_ascii_lowercase().as_str() {
            "direct" => "direct".into(),
            _ => "Proxies".into(),
        })
        .unwrap_or_else(|| "direct".into())
}

fn parse_rule_line(
    line: &str,
    known: &std::collections::HashSet<String>,
) -> Result<RuleOutcome, RuleSkip> {
    let line = line.trim();
    if line.is_empty() {
        return Err(RuleSkip::Invalid);
    }

    let mut parts: Vec<&str> = line.split(',').map(str::trim).collect();
    if parts.len() < 2 {
        return Err(RuleSkip::Invalid);
    }

    if parts
        .last()
        .is_some_and(|p| p.eq_ignore_ascii_case("no-resolve"))
    {
        parts.pop();
    }

    if parts.len() < 2 {
        return Err(RuleSkip::Invalid);
    }

    let target_raw = parts.pop().unwrap();
    let ty = parts[0].to_ascii_uppercase();
    let payload = parts[1..].join(",");

    let target = resolve_target(target_raw, known)?;

    match ty.as_str() {
        "MATCH" => Ok(RuleOutcome::Final(target)),
        "DOMAIN" => Ok(RuleOutcome::Rule(json!({
            "domain": [payload],
            "outbound": target,
        }))),
        "DOMAIN-SUFFIX" => Ok(RuleOutcome::Rule(json!({
            "domain_suffix": [payload],
            "outbound": target,
        }))),
        "DOMAIN-KEYWORD" => Ok(RuleOutcome::Rule(json!({
            "domain_keyword": [payload],
            "outbound": target,
        }))),
        "IP-CIDR" | "IP-CIDR6" => Ok(RuleOutcome::Rule(json!({
            "ip_cidr": [payload],
            "outbound": target,
        }))),
        "GEOIP" => {
            let code = payload.trim().to_ascii_lowercase();
            if matches!(code.as_str(), "lan" | "private") {
                Ok(RuleOutcome::Rule(json!({
                    "ip_is_private": true,
                    "outbound": target,
                })))
            } else {
                Ok(RuleOutcome::Rule(json!({
                    "geoip": [code],
                    "outbound": target,
                })))
            }
        }
        "PROCESS-NAME" => Ok(RuleOutcome::Rule(json!({
            "process_name": [payload],
            "outbound": target,
        }))),
        "RULE-SET" | "GEOSITE" => Err(RuleSkip::Unsupported(ty)),
        _ => Err(RuleSkip::Unsupported(ty)),
    }
}

fn resolve_target(
    raw: &str,
    known: &std::collections::HashSet<String>,
) -> Result<String, RuleSkip> {
    let normalized = normalize_clash_target(raw);
    if normalized == "direct" || normalized == "block" {
        return Ok(normalized);
    }
    if known.contains(raw) {
        return Ok(raw.to_string());
    }
    if known.contains(&normalized) {
        return Ok(normalized);
    }
    Err(RuleSkip::UnknownTarget(raw.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_domain_suffix_rule() {
        let known = ["direct".into(), "YouTube".into()].into_iter().collect();
        let rule = parse_rule_line("DOMAIN-SUFFIX,ggpht.com,YouTube", &known).unwrap();
        match rule {
            RuleOutcome::Rule(v) => {
                assert_eq!(v["domain_suffix"][0], "ggpht.com");
                assert_eq!(v["outbound"], "YouTube");
            }
            _ => panic!("expected rule"),
        }
    }

    #[test]
    fn match_sets_final() {
        let known = ["Proxies".into()].into_iter().collect();
        let rule = parse_rule_line("MATCH,Proxies", &known).unwrap();
        match rule {
            RuleOutcome::Final(o) => assert_eq!(o, "Proxies"),
            _ => panic!("expected final"),
        }
    }

    #[test]
    fn unknown_target_is_skipped_not_kept() {
        let known = std::collections::HashSet::new();
        let err = parse_rule_line("DOMAIN-SUFFIX,example.com,GoneGroup", &known).unwrap_err();
        match err {
            RuleSkip::UnknownTarget(t) => assert_eq!(t, "GoneGroup"),
            other => panic!("expected unknown target, got {other:?}"),
        }
    }
}
