// SPDX-License-Identifier: GPL-3.0-or-later

import type { MemoryUsage } from "../api/client";
import { isMacosHost } from "@platform/windowChrome";
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

/** Where the yellow and red bands start, in whole MB. */
export interface MemoryBands {
  warnMb: number;
  badMb: number;
}

/** Baseline bands: <60 MB green, 60–99 yellow, ≥100 red. */
const BASE_BANDS: MemoryBands = { warnMb: 60, badMb: 100 };

/** macOS shifts both bands up by this much. Its app figure is the physical
 * footprint of a process that also hosts AppKit and the WebKit UI side, which
 * idles around 50 MB where the Windows private working set idles around 15. */
const MACOS_BAND_SHIFT_MB = 20;

/** Bands for this host: the baseline, shifted up on macOS (<80 green,
 * 80–119 yellow, ≥120 red). */
export function memoryBands(macos: boolean = isMacosHost()): MemoryBands {
  return macos
    ? {
        warnMb: BASE_BANDS.warnMb + MACOS_BAND_SHIFT_MB,
        badMb: BASE_BANDS.badMb + MACOS_BAND_SHIFT_MB,
      }
    : BASE_BANDS;
}

/** Color band by the displayed value, against this host's bands. */
export function memoryTone(
  mb: number,
  bands: MemoryBands = memoryBands(),
): Tone {
  if (mb < bands.warnMb) return "ok";
  if (mb < bands.badMb) return "warn";
  return "bad";
}

/** Band for the row, `null` while the row shows `—` (no color claim). */
export function memoryToneFor(
  memory: MemoryUsage | null | undefined,
): Tone | null {
  return memoryAvailable(memory) ? memoryTone(displayMemoryMb(memory)) : null;
}
