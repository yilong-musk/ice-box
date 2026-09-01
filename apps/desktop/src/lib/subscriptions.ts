import type { SubscriptionMeta } from "../api/tauri";
import { t } from "./i18n";

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
  return `${w.code}: ${w.message}`;
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
