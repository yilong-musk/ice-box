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
