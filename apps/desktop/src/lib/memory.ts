// SPDX-License-Identifier: GPL-3.0-or-later

import type { MemoryUsage } from "../api/client";
import { t } from "./i18n";

export type MemoryTone = "ok" | "warn" | "bad";

/** Rounded whole-MB figure. */
export function memoryMb(bytes: number): number {
  return Math.round(bytes / (1024 * 1024));
}

/** `N MB` for a whole-MB figure. */
function formatMemoryMb(mb: number): string {
  return `${mb} MB`;
}

/** `128 MB`. */
export function formatMemory(bytes: number): string {
  return formatMemoryMb(memoryMb(bytes));
}

/** One side of the breakdown: `N MB`, or `—` when that part is unreadable. */
export function formatMemoryPart(bytes: number | null): string {
  return bytes == null ? t("common.dash") : formatMemory(bytes);
}

/** Whole-MB figure the row shows for `memory`: the readable parts, each
 * rounded, summed. The tooltip prints those same per-process figures, so
 * summing them keeps the breakdown adding up to the label, where rounding the
 * byte total instead can differ from the two rounded parts by one MB. The
 * label and the color band both use this value, so the color always matches
 * the number the user sees. */
export function displayMemoryMb(memory: MemoryUsage): number {
  let mb = 0;
  if (memory.app_bytes != null) mb += memoryMb(memory.app_bytes);
  if (memory.core_bytes != null) mb += memoryMb(memory.core_bytes);
  return mb;
}

/** Whether at least one part was read; otherwise the row shows `—`. */
export function memoryAvailable(
  memory: MemoryUsage | null | undefined,
): memory is MemoryUsage {
  return (
    memory != null && (memory.app_bytes != null || memory.core_bytes != null)
  );
}

/** Row value: the rounded breakdown, or `—` when nothing could be read. */
export function memoryLabel(memory: MemoryUsage | null | undefined): string {
  return memoryAvailable(memory)
    ? formatMemoryMb(displayMemoryMb(memory))
    : t("common.dash");
}

/** Color band by the displayed value: <60 MB green, 60–99 yellow, ≥100 red. */
export function memoryTone(mb: number): MemoryTone {
  if (mb < 60) return "ok";
  if (mb < 100) return "warn";
  return "bad";
}

/** Band for the row, `null` while the row shows `—` (no color claim). */
export function memoryToneFor(
  memory: MemoryUsage | null | undefined,
): MemoryTone | null {
  return memoryAvailable(memory) ? memoryTone(displayMemoryMb(memory)) : null;
}
