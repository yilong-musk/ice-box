// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { formatQuota, formatRate } from "./traffic";

describe("formatRate", () => {
  it("formats bytes per second", () => {
    expect(formatRate(512)).toBe("512 B/s");
    expect(formatRate(2048)).toBe("2.0 KB/s");
    expect(formatRate(5 * 1024 * 1024)).toBe("5.00 MB/s");
  });
});

describe("formatQuota", () => {
  it("scales byte counts to binary units without a space", () => {
    expect(formatQuota(0)).toBe("0B");
    expect(formatQuota(512)).toBe("512B");
    expect(formatQuota(390_596_257)).toBe("372.5MB");
    expect(formatQuota(11.84 * 1024 ** 3)).toBe("11.84GB");
    expect(formatQuota(150 * 1024 ** 3)).toBe("150GB");
    expect(formatQuota(1024 ** 4)).toBe("1TB");
  });

  it("rejects negative and non-finite counts", () => {
    expect(formatQuota(-1)).toBe("0B");
    expect(formatQuota(Number.NaN)).toBe("0B");
  });
});
