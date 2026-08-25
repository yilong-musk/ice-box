import { describe, expect, it } from "vitest";
import {
  getDisplacementFilter,
  getDisplacementMap,
  supportsBackdropFilterUrl,
} from "./liquidGlass";

describe("liquidGlass", () => {
  it("builds a displacement map data URI", () => {
    const map = getDisplacementMap({
      width: 240,
      height: 180,
      radius: 22,
      depth: 12,
    });
    expect(map.startsWith("data:image/svg+xml;utf8,")).toBe(true);
    expect(decodeURIComponent(map)).toContain("linearGradient");
    expect(decodeURIComponent(map)).toContain("#808080");
  });

  it("builds a filter data URI with displace anchor", () => {
    const filter = getDisplacementFilter({
      width: 240,
      height: 180,
      radius: 22,
      depth: 12,
      strength: 90,
      chromaticAberration: 2,
    });
    expect(filter.startsWith("data:image/svg+xml;utf8,")).toBe(true);
    expect(filter.endsWith("#displace")).toBe(true);
    expect(decodeURIComponent(filter)).toContain("feDisplacementMap");
    expect(decodeURIComponent(filter)).toContain("feBlend");
  });

  it("reports backdrop-filter url support as a boolean", () => {
    expect(typeof supportsBackdropFilterUrl()).toBe("boolean");
  });
});
