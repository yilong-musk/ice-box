import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { applyFlagEmojiPolyfill, FLAG_EMOJI_FONT } from "./flagEmoji";

const polyfillCountryFlagEmojis = vi.fn();

vi.mock("country-flag-emoji-polyfill", () => ({
  polyfillCountryFlagEmojis: (...args: unknown[]) =>
    polyfillCountryFlagEmojis(...args),
}));

describe("applyFlagEmojiPolyfill", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    polyfillCountryFlagEmojis.mockReturnValue(true);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("loads the bundled font under the CSS stack name", () => {
    expect(applyFlagEmojiPolyfill("/TwemojiCountryFlags.woff2")).toBe(true);
    expect(polyfillCountryFlagEmojis).toHaveBeenCalledWith(
      FLAG_EMOJI_FONT,
      "/TwemojiCountryFlags.woff2",
    );
  });

  it("swallows detector failures so startup cannot crash", () => {
    polyfillCountryFlagEmojis.mockImplementation(() => {
      throw new Error("canvas unavailable");
    });
    expect(applyFlagEmojiPolyfill("/TwemojiCountryFlags.woff2")).toBe(false);
  });

  it("skips injection when document is unavailable", () => {
    vi.stubGlobal("document", undefined);
    expect(applyFlagEmojiPolyfill("/TwemojiCountryFlags.woff2")).toBe(false);
    expect(polyfillCountryFlagEmojis).not.toHaveBeenCalled();
  });
});
