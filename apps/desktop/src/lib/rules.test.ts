import { describe, expect, it } from "vitest";
import type { RuleRow } from "../api/tauri";
import {
  MATCH_KEY_ORDER,
  RULE_TYPE_LABELS,
  buildCustomRule,
  pageCount,
  ruleMatchSummary,
  ruleOutbound,
  ruleTypeLabel,
} from "./rules";

/**
 * Backend classifier keys, mirrored from
 * `crates/ice-config/src/rule_overrides.rs` (`RULE_TYPE_KEYS`).
 * Update both files together; the parity tests below pin them.
 */
const BACKEND_RULE_TYPE_KEYS = [
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

function row(rule: Record<string, unknown>): RuleRow {
  return {
    index: 0,
    fingerprint: "fp",
    rule,
    custom: false,
    disabled: false,
    rule_type: "other",
  };
}

describe("rule helpers", () => {
  it("labels known and unknown rule types", () => {
    expect(ruleTypeLabel("domain_suffix")).toBe("域名后缀");
    expect(ruleTypeLabel("geoip")).toBe("GEOIP");
    expect(ruleTypeLabel("unknown_thing")).toBe("unknown_thing");
  });

  it("summarizes first matcher by priority", () => {
    expect(
      ruleMatchSummary(row({ domain_suffix: ["a.com", "b.com"], outbound: "direct" })),
    ).toBe("a.com, b.com");
    expect(ruleMatchSummary(row({ geoip: ["cn"], outbound: "direct" }))).toBe("cn");
    expect(ruleMatchSummary(row({ ip_is_private: true, outbound: "direct" }))).toBe("true");
    expect(ruleMatchSummary(row({ outbound: "direct" }))).toBe("");
  });

  it("extracts outbound target", () => {
    expect(ruleOutbound(row({ domain: ["x.com"], outbound: "Proxies" }))).toBe("Proxies");
    expect(ruleOutbound(row({ domain: ["x.com"] }))).toBe("");
  });

  it("computes page counts", () => {
    expect(pageCount(0, 50)).toBe(1);
    expect(pageCount(4270, 50)).toBe(86);
    expect(pageCount(50, 50)).toBe(1);
  });

  it("builds array matcher rules with trimmed comma values", () => {
    expect(buildCustomRule("domain_suffix", " x.io, y.io ", "direct")).toEqual({
      domain_suffix: ["x.io", "y.io"],
      outbound: "direct",
    });
    expect(buildCustomRule("rule_set", "geoip-cn", "block")).toEqual({
      rule_set: ["geoip-cn"],
      outbound: "block",
    });
  });

  it("builds boolean matcher rules", () => {
    expect(buildCustomRule("ip_is_private", true, "direct")).toEqual({
      ip_is_private: true,
      outbound: "direct",
    });
    expect(buildCustomRule("source_ip_is_private", false, "direct")).toEqual({
      source_ip_is_private: false,
      outbound: "direct",
    });
  });

  it("rejects empty or unknown matchers", () => {
    expect(buildCustomRule("domain_suffix", "  ,  ", "direct")).toBeNull();
    expect(buildCustomRule("nope", "x", "direct")).toBeNull();
    expect(buildCustomRule("domain_suffix", true, "direct")).toBeNull();
    // sing-box 1.13 removed geoip/geosite rule options; not offered by the editor.
    expect(buildCustomRule("geoip", "cn", "direct")).toBeNull();
    expect(buildCustomRule("geosite", "google", "direct")).toBeNull();
  });

  it("keeps matcher keys in parity with backend RULE_TYPE_KEYS", () => {
    expect([...MATCH_KEY_ORDER].sort()).toEqual([...BACKEND_RULE_TYPE_KEYS].sort());
  });

  it("labels every backend rule type", () => {
    for (const key of BACKEND_RULE_TYPE_KEYS) {
      expect(RULE_TYPE_LABELS[key], `label for ${key}`).toBeTruthy();
    }
  });
});
