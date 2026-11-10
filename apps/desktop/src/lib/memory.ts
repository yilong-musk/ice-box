// SPDX-License-Identifier: GPL-3.0-or-later

import type { MemoryBands, MemoryUsage } from "../api/client";
import { t } from "./i18n";
import type { Tone } from "./tone";

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

/** Bands for a card with no sample yet. A real sample always carries the
 * bands of the platform that measured it; the UI never picks them itself. */
export const DEFAULT_MEMORY_BANDS: Readonly<MemoryBands> = Object.freeze({
  warn_mb: 60,
  bad_mb: 100,
});

/** Color band by the displayed value: green below `warn_mb`, yellow up to
 * `bad_mb`, red from there. */
export function memoryTone(mb: number, bands: MemoryBands): Tone {
  if (mb < bands.warn_mb) return "ok";
  if (mb < bands.bad_mb) return "warn";
  return "bad";
}

/** Band for the row against the sample's own bands, `null` while the row
 * shows `—` (no color claim). */
export function memoryToneFor(
  memory: MemoryUsage | null | undefined,
): Tone | null {
  return memoryAvailable(memory)
    ? memoryTone(displayMemoryMb(memory), memory.bands)
    : null;
}
