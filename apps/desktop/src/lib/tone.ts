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

/** Left-to-right fade for a row wash; pair it with `bg-linear-to-r`. */
export const TONE_WASH: Record<Tone, string> = {
  ok: "from-ok/5 to-ok/20",
  warn: "from-warn/5 to-warn/20",
  bad: "from-destructive/5 to-destructive/20",
};
