import { describe, expect, it } from "vitest";
import { delaySortKey, formatDelay, resolveSelectedTag } from "./nodes";

describe("formatDelay", () => {
  it("renders known states", () => {
    expect(formatDelay(null)).toBe("—");
    expect(formatDelay("testing")).toBe("…");
    expect(formatDelay("error")).toBe("失败");
    expect(formatDelay(42)).toBe("42 ms");
  });
});

describe("delaySortKey", () => {
  it("sorts numeric delays before unknown states", () => {
    expect(delaySortKey(100)).toBeLessThan(delaySortKey("error"));
    expect(delaySortKey(null)).toBe(Number.POSITIVE_INFINITY);
  });
});

describe("resolveSelectedTag", () => {
  it("keeps valid settings tag", () => {
    const nodes = [{ tag: "a" }, { tag: "b" }];
    expect(resolveSelectedTag("b", nodes)).toBe("b");
  });

  it("falls back to first node when settings tag missing", () => {
    const nodes = [{ tag: "a" }, { tag: "b" }];
    expect(resolveSelectedTag(null, nodes)).toBe("a");
  });

  it("falls back when settings tag is stale", () => {
    const nodes = [{ tag: "a" }];
    expect(resolveSelectedTag("gone", nodes)).toBe("a");
  });
});
