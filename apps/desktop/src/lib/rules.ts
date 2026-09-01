import type { RuleRow } from "../api/tauri";
import { t, type MessageKey } from "./i18n";

export const RULE_TYPE_LABEL_KEYS: Record<string, MessageKey> = {
  domain: "ruleType.domain",
  domain_suffix: "ruleType.domainSuffix",
  domain_keyword: "ruleType.domainKeyword",
  domain_regex: "ruleType.domainRegex",
  ip_cidr: "ruleType.ipCidr",
  ip_is_private: "ruleType.ipIsPrivate",
  source_ip_cidr: "ruleType.sourceIpCidr",
  source_ip_is_private: "ruleType.sourceIpIsPrivate",
  rule_set: "ruleType.ruleSet",
  geoip: "ruleType.geoip",
  geosite: "ruleType.geosite",
  port: "ruleType.port",
  source_port: "ruleType.sourcePort",
  network: "ruleType.network",
  protocol: "ruleType.protocol",
  process_name: "ruleType.processName",
  process_path: "ruleType.processPath",
  package_name: "ruleType.packageName",
  inbound: "ruleType.inbound",
  wifi_ssid: "ruleType.wifiSsid",
  wifi_bssid: "ruleType.wifiBssid",
  clash_mode: "ruleType.clashMode",
  user: "ruleType.user",
  other: "ruleType.other",
};

/** Backwards-compatible alias kept for callers/tests that import the old map. */
export const RULE_TYPE_LABELS: Record<string, string> =
  RULE_TYPE_LABEL_KEYS as Record<string, string>;

/** Matcher key priority, mirrored from the backend classifier. */
export const MATCH_KEY_ORDER = [
  "domain",
  "domain_suffix",
  "domain_keyword",
  "domain_regex",
  "ip_cidr",
  "ip_is_private",
  "source_ip_cidr",
  "source_ip_is_private",
  "rule_set",
  "geoip",
  "geosite",
  "port",
  "source_port",
  "network",
  "protocol",
  "process_name",
  "process_path",
  "package_name",
  "inbound",
  "wifi_ssid",
  "wifi_bssid",
  "clash_mode",
  "user",
];

export function ruleTypeLabel(ruleType: string): string {
  const key = RULE_TYPE_LABEL_KEYS[ruleType];
  return key ? t(key) : ruleType;
}

/** Human-readable match payload of a rule, e.g. `youtube.com, google.com`. */
export function ruleMatchSummary(row: RuleRow): string {
  for (const key of MATCH_KEY_ORDER) {
    const v = row.rule[key];
    if (v === undefined) continue;
    if (Array.isArray(v)) return v.map(String).join(", ");
    return String(v);
  }
  return "";
}

/** Outbound target of a rule, or empty when missing. */
export function ruleOutbound(row: RuleRow): string {
  const v = row.rule.outbound;
  return typeof v === "string" ? v : "";
}

export function pageCount(total: number, limit: number): number {
  return Math.max(1, Math.ceil(total / limit));
}

/** Strategy-group outbound types (mirrors the Nodes page / backend classifier). */
export const STRATEGY_GROUP_TYPES = [
  "selector",
  "urltest",
  "fallback",
  "loadbalance",
];

export type RuleMatcherDef = {
  key: string;
  /** i18n message key; resolve the display label with `t(def.label)`. */
  label: MessageKey;
  kind: "array" | "boolean";
  placeholder: string;
};

/**
 * Matchers offered by the interactive custom-rule editor. GEOIP / GEOSITE are omitted:
 * sing-box 1.13 removed those rule options, and only subscription rules get the
 * geoip → rule-set expansion at build time. Use `rule_set` instead.
 */
export const RULE_MATCHER_DEFS: RuleMatcherDef[] = [
  { key: "domain", label: "ruleType.domain", kind: "array", placeholder: "example.com" },
  { key: "domain_suffix", label: "ruleType.domainSuffix", kind: "array", placeholder: "google.com" },
  { key: "domain_keyword", label: "ruleType.domainKeyword", kind: "array", placeholder: "youtube" },
  { key: "domain_regex", label: "ruleType.domainRegex", kind: "array", placeholder: ".*\\.cn$" },
  { key: "ip_cidr", label: "ruleType.ipCidr", kind: "array", placeholder: "10.0.0.0/8" },
  { key: "ip_is_private", label: "ruleType.ipIsPrivate", kind: "boolean", placeholder: "" },
  { key: "source_ip_cidr", label: "ruleType.sourceIpCidr", kind: "array", placeholder: "192.168.1.0/24" },
  { key: "source_ip_is_private", label: "ruleType.sourceIpIsPrivate", kind: "boolean", placeholder: "" },
  { key: "rule_set", label: "ruleType.ruleSet", kind: "array", placeholder: "geoip-cn" },
  { key: "port", label: "ruleType.port", kind: "array", placeholder: "443, 8443" },
  { key: "source_port", label: "ruleType.sourcePort", kind: "array", placeholder: "53" },
  { key: "network", label: "ruleType.network", kind: "array", placeholder: "tcp" },
  { key: "protocol", label: "ruleType.protocol", kind: "array", placeholder: "http" },
  { key: "process_name", label: "ruleType.processName", kind: "array", placeholder: "curl" },
  { key: "process_path", label: "ruleType.processPath", kind: "array", placeholder: "/usr/bin/curl" },
  { key: "package_name", label: "ruleType.packageName", kind: "array", placeholder: "com.example.app" },
  { key: "inbound", label: "ruleType.inbound", kind: "array", placeholder: "mixed-in" },
];

/**
 * Compose a sing-box route rule object from interactive form fields.
 * `array` matchers take comma-separated values; returns null when the value is empty.
 */
export function buildCustomRule(
  matcherKey: string,
  rawValue: string | boolean,
  outbound: string,
): Record<string, unknown> | null {
  const def = RULE_MATCHER_DEFS.find((d) => d.key === matcherKey);
  if (!def) return null;
  if (def.kind === "boolean") {
    return { [def.key]: rawValue === true, outbound };
  }
  if (typeof rawValue !== "string") return null;
  const values = rawValue
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
  if (values.length === 0) return null;
  return { [def.key]: values, outbound };
}