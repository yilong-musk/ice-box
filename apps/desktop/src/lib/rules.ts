import type { RuleRow } from "../api/tauri";

export const RULE_TYPE_LABELS: Record<string, string> = {
  domain: "域名",
  domain_suffix: "域名后缀",
  domain_keyword: "域名关键词",
  domain_regex: "域名正则",
  ip_cidr: "IP 段",
  ip_is_private: "私网 IP",
  source_ip_cidr: "源 IP 段",
  source_ip_is_private: "源私网 IP",
  rule_set: "规则集",
  geoip: "GEOIP",
  geosite: "GEOSITE",
  port: "端口",
  source_port: "源端口",
  network: "网络",
  protocol: "协议",
  process_name: "进程名",
  process_path: "进程路径",
  package_name: "应用包名",
  inbound: "入站",
  wifi_ssid: "WiFi SSID",
  wifi_bssid: "WiFi BSSID",
  clash_mode: "Clash 模式",
  user: "用户",
  other: "其他",
};

/** Matcher key priority, mirrored from the backend classifier. */
const MATCH_KEY_ORDER = [
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
  return RULE_TYPE_LABELS[ruleType] ?? ruleType;
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
  label: string;
  kind: "array" | "boolean";
  placeholder: string;
};

/**
 * Matchers offered by the interactive custom-rule editor. GEOIP / GEOSITE are omitted:
 * sing-box 1.13 removed those rule options, and only subscription rules get the
 * geoip → rule-set expansion at build time. Use `rule_set` instead.
 */
export const RULE_MATCHER_DEFS: RuleMatcherDef[] = [
  { key: "domain", label: "域名", kind: "array", placeholder: "example.com" },
  { key: "domain_suffix", label: "域名后缀", kind: "array", placeholder: "google.com" },
  { key: "domain_keyword", label: "域名关键词", kind: "array", placeholder: "youtube" },
  { key: "domain_regex", label: "域名正则", kind: "array", placeholder: ".*\\.cn$" },
  { key: "ip_cidr", label: "IP 段", kind: "array", placeholder: "10.0.0.0/8" },
  { key: "ip_is_private", label: "私网 IP", kind: "boolean", placeholder: "" },
  { key: "source_ip_cidr", label: "源 IP 段", kind: "array", placeholder: "192.168.1.0/24" },
  { key: "source_ip_is_private", label: "源私网 IP", kind: "boolean", placeholder: "" },
  { key: "rule_set", label: "规则集", kind: "array", placeholder: "geoip-cn" },
  { key: "port", label: "端口", kind: "array", placeholder: "443, 8443" },
  { key: "source_port", label: "源端口", kind: "array", placeholder: "53" },
  { key: "network", label: "网络", kind: "array", placeholder: "tcp" },
  { key: "protocol", label: "协议", kind: "array", placeholder: "http" },
  { key: "process_name", label: "进程名", kind: "array", placeholder: "curl" },
  { key: "process_path", label: "进程路径", kind: "array", placeholder: "/usr/bin/curl" },
  { key: "package_name", label: "应用包名", kind: "array", placeholder: "com.example.app" },
  { key: "inbound", label: "入站", kind: "array", placeholder: "mixed-in" },
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