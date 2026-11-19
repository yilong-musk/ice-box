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

/** Regional-indicator flag for an ISO 3166-1 alpha-2 code. `XX` is
 * Cloudflare's unknown country and has no flag. */
export function countryFlagEmoji(code: string | null | undefined): string | null {
  if (!code) return null;
  const upper = code.trim().toUpperCase();
  if (!/^[A-Z]{2}$/.test(upper) || upper === "XX") return null;
  const base = 0x1f1e6;
  return String.fromCodePoint(
    base + upper.charCodeAt(0) - 65,
    base + upper.charCodeAt(1) - 65,
  );
}
