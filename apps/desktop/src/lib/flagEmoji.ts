// SPDX-License-Identifier: GPL-3.0-or-later

import { polyfillCountryFlagEmojis } from "country-flag-emoji-polyfill";

/** Must match the name in `index.css` font stacks. */
export const FLAG_EMOJI_FONT = "Twemoji Country Flags";

/**
 * Load a flag-only color font when the engine draws regional-indicator pairs
 * as letters (typically Chromium on Windows; detected at runtime). Bundled
 * locally — no CDN.
 */
export function applyFlagEmojiPolyfill(fontUrl: string): boolean {
  if (typeof document === "undefined") return false;
  try {
    return polyfillCountryFlagEmojis(FLAG_EMOJI_FONT, fontUrl);
  } catch {
    return false;
  }
}

const REGIONAL_INDICATOR_A = 0x1f1e6;

/** Flag emoji for an ISO 3166-1 alpha-2 code, or `null` when the code is malformed. */
export function regionFlagEmoji(code: string): string | null {
  if (!/^[a-z]{2}$/i.test(code)) return null;
  const upper = code.toUpperCase();
  return String.fromCodePoint(
    REGIONAL_INDICATOR_A + upper.charCodeAt(0) - 65,
    REGIONAL_INDICATOR_A + upper.charCodeAt(1) - 65,
  );
}
