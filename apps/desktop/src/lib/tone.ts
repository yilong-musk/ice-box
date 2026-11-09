// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Shared three-step health tone (green / yellow / red) and its colour classes.
 * Each domain keeps its own thresholds (e.g. `memoryTone`, `trafficTone`) and
 * maps the result through these tables, so the palette lives in one place.
 */
export type Tone = "ok" | "warn" | "bad";

export const TONE_TEXT: Record<Tone, string> = {
  ok: "text-ok",
  warn: "text-warn",
  bad: "text-destructive",
};

export const TONE_BG: Record<Tone, string> = {
  ok: "bg-ok",
  warn: "bg-warn",
  bad: "bg-destructive",
};
