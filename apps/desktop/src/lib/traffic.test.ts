import { describe, expect, it } from "vitest";
import { formatRate } from "./traffic";

describe("formatRate", () => {
  it("formats bytes per second", () => {
    expect(formatRate(512)).toBe("512 B/s");
    expect(formatRate(2048)).toBe("2.0 KB/s");
    expect(formatRate(5 * 1024 * 1024)).toBe("5.00 MB/s");
  });
});
