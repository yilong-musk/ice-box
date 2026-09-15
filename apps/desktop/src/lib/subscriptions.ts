// SPDX-License-Identifier: GPL-3.0-or-later

import {
  formatInvokeError,
  type SubscriptionMeta,
  type SubscriptionUserInfo,
} from "../api/tauri";
import { t } from "./i18n";
import { formatQuota } from "./traffic";

export type ApplyWarning = {
  code: string;
  message: string;
};

export type SubscriptionMutationResult = SubscriptionMeta & {
  apply_warning?: ApplyWarning;
};

export type UpdateAllResult = {
  results: Array<{ id: string; ok: boolean; error?: string }>;
  apply_warning?: ApplyWarning;
};

export type RemoveSubscriptionResult = {
  ok: boolean;
  apply_warning?: ApplyWarning;
};

export function formatApplyWarning(w: ApplyWarning): string {
  return formatInvokeError(w);
}

export function extractApplyWarning(payload: unknown): ApplyWarning | null {
  if (!payload || typeof payload !== "object") return null;
  const w = (payload as { apply_warning?: ApplyWarning }).apply_warning;
  if (!w || typeof w.code !== "string" || typeof w.message !== "string") {
    return null;
  }
  return w;
}

export function extractUpdateResults(
  payload: unknown,
): UpdateAllResult["results"] | null {
  if (!payload || typeof payload !== "object") return null;
  const results = (payload as UpdateAllResult).results;
  if (!Array.isArray(results)) return null;
  return results;
}

export function formatUpdateFailures(
  results: UpdateAllResult["results"],
): string | null {
  const failed = results.filter((r) => !r.ok);
  if (failed.length === 0) return null;
  return failed
    .map((r) => `${r.id.slice(0, 8)}…: ${r.error ?? t("subs.unknownError")}`)
    .join("; ");
}

/** True when URL uses plain http (not https). */
export function isInsecureSubscriptionUrl(url: string): boolean {
  const trimmed = url.trim().toLowerCase();
  return trimmed.startsWith("http://") && !trimmed.startsWith("https://");
}

export type SubscriptionTrafficView = {
  /** `11.84GB/150GB（7.89%）`, or a single-sided fallback when only one side is known. */
  usage: string | null;
  /** `到期 2026-09-26` / `长期有效` / `已过期 2026-01-01`; `null` when unknown. */
  expiry: string | null;
  expired: boolean;
};

/**
 * One subscription's usage and expiry, normalized from both shapes a provider
 * publishes them in: the `subscription-userinfo` counters and the info entries
 * embedded in the proxy list (`Traffic: 11.84 GB | 150 GB`,
 * `剩余流量：1023.64 GB`, `套餐到期：长期有效`). Providers word and scale those
 * entries differently, so the values are parsed and re-rendered in one format:
 * `<used>/<total>（<percent>%）` plus a dated or long-term expiry.
 *
 * `null` when neither side can be determined. An expiry in the past is flagged
 * so the list can mark the subscription as expired.
 */
export function subscriptionTrafficView(
  userinfo: SubscriptionUserInfo | null | undefined,
  providerInfo: readonly string[] | null | undefined,
  now: number = Date.now(),
): SubscriptionTrafficView | null {
  const lines = providerInfo ?? [];
  const body = parseProviderTraffic(lines);
  const bodyReported =
    body.used !== null || body.total !== null || body.remaining !== null;

  const headerCounters = userinfo
    ? (userinfo.upload ?? 0) + (userinfo.download ?? 0)
    : 0;
  const headerTotal =
    userinfo && (userinfo.total ?? 0) > 0 ? (userinfo.total ?? 0) : null;
  // A panel that reports a quota without usage counters sends `upload=0;
  // download=0`, which is indistinguishable from a real zero; when the list
  // entries carry amounts, derive the used side from them instead of a bare 0.
  const headerUsed =
    headerCounters > 0 || (headerTotal !== null && !bodyReported)
      ? headerCounters
      : null;

  let used = body.used ?? headerUsed;
  let total = body.total ?? headerTotal;
  const remaining =
    body.remaining ??
    (used !== null && total !== null ? Math.max(0, total - used) : null);
  if (total === null && used !== null && remaining !== null) {
    total = used + remaining;
  }
  if (used === null && total !== null && remaining !== null) {
    used = Math.max(0, total - remaining);
  }

  const usage = formatUsage(used, total, remaining);
  const expiry = formatExpiry(userinfo, lines, now);
  if (!usage && !expiry.text) return null;
  return { usage, expiry: expiry.text, expired: expiry.expired };
}

/** Tokens like `11.84 GB`, `1023.64GB`, `1TiB`; a unit is required so
 * percentages (`7.9%`) and dates (`2026-09-26`) are never read as amounts. */
const AMOUNT_RE = /(\d+(?:\.\d+)?)\s*(pib|tib|gib|mib|kib|pb|tb|gb|mb|kb|b|字节)/gi;

const BYTES_PER_UNIT: Record<string, number> = {
  b: 1,
  字节: 1,
  kb: 1024,
  mb: 1024 ** 2,
  gb: 1024 ** 3,
  tb: 1024 ** 4,
  pb: 1024 ** 5,
  kib: 1024,
  mib: 1024 ** 2,
  gib: 1024 ** 3,
  tib: 1024 ** 4,
  pib: 1024 ** 5,
};

type ProviderTraffic = {
  used: number | null;
  total: number | null;
  remaining: number | null;
};

const EMPTY_TRAFFIC: ProviderTraffic = { used: null, total: null, remaining: null };

/** `label: value` entries; the label decides what the amounts mean. */
function splitProviderLine(line: string): { label: string; value: string } {
  const separator = line.search(/[:：|｜]/);
  if (separator < 0) return { label: line.trim(), value: "" };
  return {
    label: line.slice(0, separator).trim(),
    value: line.slice(separator + 1).trim(),
  };
}

function scanAmounts(text: string): number[] {
  const amounts: number[] = [];
  for (const match of text.matchAll(AMOUNT_RE)) {
    const value = Number(match[1]);
    const multiplier = BYTES_PER_UNIT[match[2].toLowerCase()];
    if (Number.isFinite(value) && multiplier !== undefined) {
      amounts.push(value * multiplier);
    }
  }
  return amounts;
}

/** Read used / total / remaining out of the embedded usage entries. */
function parseProviderTraffic(lines: readonly string[]): ProviderTraffic {
  let traffic = EMPTY_TRAFFIC;
  for (const line of lines) {
    const { label, value } = splitProviderLine(line);
    const lower = label.toLowerCase();
    if (EXPIRY_LABEL_RE.test(lower)) continue;
    const amounts = scanAmounts(value);
    if (amounts.length === 0) continue;
    if (/剩余|remaining/.test(lower)) {
      traffic = { ...traffic, remaining: traffic.remaining ?? amounts[0] };
    } else if (/已用|used/.test(lower)) {
      // Some panels put both sides behind a used-style label
      // (`Used: 11.84 GB | 150 GB`).
      traffic = {
        ...traffic,
        used: traffic.used ?? amounts[0],
        total: traffic.total ?? amounts[1] ?? null,
      };
    } else if (amounts.length >= 2) {
      traffic = {
        ...traffic,
        used: traffic.used ?? amounts[0],
        total: traffic.total ?? amounts[1],
      };
    } else {
      traffic = { ...traffic, total: traffic.total ?? amounts[0] };
    }
  }
  return traffic;
}

const EXPIRY_LABEL_RE = /到期|过期|有效期|expire|expiry/;
const LONG_TERM_RE = /长期|永久|不过期|无限期|never|unlimited|lifetime|long[\s-]?term|forever|∞/i;

function formatUsage(
  used: number | null,
  total: number | null,
  remaining: number | null,
): string | null {
  if (used !== null && total !== null && total > 0) {
    return t("subs.trafficUsedTotal", {
      used: formatQuota(used),
      total: formatQuota(total),
      percent: formatPercent((used / total) * 100),
    });
  }
  if (used !== null) return t("subs.trafficUsed", { used: formatQuota(used) });
  if (remaining !== null) {
    return t("subs.trafficRemaining", { remaining: formatQuota(remaining) });
  }
  if (total !== null) return t("subs.trafficTotal", { total: formatQuota(total) });
  return null;
}

function formatExpiry(
  userinfo: SubscriptionUserInfo | null | undefined,
  lines: readonly string[],
  now: number,
): { text: string | null; expired: boolean } {
  const headerExpire = userinfo?.expire ?? null;
  if (headerExpire !== null) {
    const expired = headerExpire * 1000 < now;
    return {
      text: t(expired ? "subs.trafficExpired" : "subs.trafficExpire", {
        date: formatExpiryDate(headerExpire),
      }),
      expired,
    };
  }

  let never = false;
  for (const line of lines) {
    const { label, value } = splitProviderLine(line);
    if (!EXPIRY_LABEL_RE.test(label.toLowerCase())) continue;
    const date = parseDateText(value);
    if (date) {
      const expired = date < formatExpiryDate(now / 1000);
      return {
        text: t(expired ? "subs.trafficExpired" : "subs.trafficExpire", {
          date,
        }),
        expired,
      };
    }
    never ||= LONG_TERM_RE.test(value);
  }
  if (never) return { text: t("subs.trafficNeverExpires"), expired: false };
  return { text: null, expired: false };
}

/** `2026-09-26` inside the provider's expiry text, in any common spelling. */
function parseDateText(text: string): string | null {
  const separated = text.match(/(\d{4})[-/.](\d{1,2})[-/.](\d{1,2})/);
  if (separated) {
    return `${separated[1]}-${pad(Number(separated[2]))}-${pad(Number(separated[3]))}`;
  }
  const chinese = text.match(/(\d{4})\s*年\s*(\d{1,2})\s*月\s*(\d{1,2})\s*日/);
  if (chinese) {
    return `${chinese[1]}-${pad(Number(chinese[2]))}-${pad(Number(chinese[3]))}`;
  }
  return null;
}

/** `0.04%` / `7.89%` / `99.9%` — one decimal once the share is in double digits. */
function formatPercent(share: number): string {
  if (!Number.isFinite(share) || share <= 0) return "0";
  return trimNumber(share.toFixed(share >= 10 ? 1 : 2));
}

function trimNumber(formatted: string): string {
  return formatted.includes(".")
    ? formatted.replace(/\.?0+$/, "")
    : formatted;
}

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

/** `YYYY-MM-DD` in local time; locale-independent so the list stays stable. */
function formatExpiryDate(unixSeconds: number): string {
  const date = new Date(unixSeconds * 1000);
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}
