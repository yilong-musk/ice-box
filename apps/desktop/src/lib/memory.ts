// SPDX-License-Identifier: GPL-3.0-or-later

import type { MemoryUsage } from "../api/tauri";
import { t } from "./i18n";

export type MemoryTone = "ok" | "warn" | "bad";

/** Rounded whole-MB figure. The label and the color band both use this value,
 * so the color always matches the number the user sees (plan v0.1.14 §9). */
export function memoryMb(bytes: number): number {
  return Math.round(bytes / (1024 * 1024));
}

/** `128 MB`. */
export function formatMemory(bytes: number): string {
  return `${memoryMb(bytes)} MB`;
}

/** One side of the breakdown: `N MB`, or `—` when that part is unreadable. */
export function formatMemoryPart(bytes: number | null): string {
  return bytes == null ? t("common.dash") : formatMemory(bytes);
}

/** Whether at least one part was read; otherwise the row shows `—`. */
export function memoryAvailable(
  memory: MemoryUsage | null | undefined,
): memory is MemoryUsage {
  return (
    memory != null && (memory.app_bytes != null || memory.core_bytes != null)
  );
}

/** Row value: the rounded total, or `—` when nothing could be read. */
export function memoryLabel(memory: MemoryUsage | null | undefined): string {
  return memoryAvailable(memory) ? formatMemory(memory.total_bytes) : t("common.dash");
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
  return memoryAvailable(memory) ? memoryTone(memoryMb(memory.total_bytes)) : null;
}
