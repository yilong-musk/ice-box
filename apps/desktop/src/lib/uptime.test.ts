// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { formatUptime } from "./uptime";

describe("formatUptime", () => {
  it("shows minutes and seconds under an hour", () => {
    expect(formatUptime(0)).toBe("00:00");
    expect(formatUptime(9_999)).toBe("00:09");
    expect(formatUptime(3_599_000)).toBe("59:59");
  });

  it("adds the hour field from one hour on", () => {
    expect(formatUptime(3_600_000)).toBe("1:00:00");
    expect(formatUptime(36_610_000)).toBe("10:10:10");
  });

  it("keeps counting hours past a day without rolling over", () => {
    expect(formatUptime(86_399_000)).toBe("23:59:59");
    expect(formatUptime(86_400_000)).toBe("24:00:00");
    expect(formatUptime(90_061_000)).toBe("25:01:01");
    expect(formatUptime(10 * 86_400_000 + 3 * 3_600_000 + 5 * 60_000 + 59_000)).toBe("243:05:59");
  });

  it("clamps negative elapsed time to zero", () => {
    expect(formatUptime(-5_000)).toBe("00:00");
  });
});
