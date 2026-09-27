// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { t } from "./i18n";
import {
  displayMemoryMb,
  formatMemory,
  formatMemoryPart,
  memoryAvailable,
  memoryLabel,
  memoryMb,
  memoryTone,
  memoryToneFor,
} from "./memory";

const MB = 1024 * 1024;

describe("formatMemory", () => {
  it("rounds to whole MB", () => {
    expect(formatMemory(0)).toBe("0 MB");
    expect(formatMemory(1.4 * MB)).toBe("1 MB");
    expect(formatMemory(52 * MB)).toBe("52 MB");
    expect(formatMemory(59.6 * MB)).toBe("60 MB");
    expect(formatMemory(128 * MB)).toBe("128 MB");
  });

  it("keeps the label and the band on the same rounded value", () => {
    // 59.6 MB renders as `60 MB`, so it must take the yellow band.
    expect(memoryMb(59.6 * MB)).toBe(60);
    expect(memoryTone(memoryMb(59.6 * MB))).toBe("warn");
  });
});

describe("memoryTone", () => {
  it("bands <60 green, 60–99 yellow, ≥100 red", () => {
    expect(memoryTone(0)).toBe("ok");
    expect(memoryTone(59)).toBe("ok");
    expect(memoryTone(60)).toBe("warn");
    expect(memoryTone(99)).toBe("warn");
    expect(memoryTone(100)).toBe("bad");
    expect(memoryTone(1024)).toBe("bad");
  });
});

describe("memoryLabel / memoryToneFor", () => {
  it("shows a dash when nothing could be read", () => {
    expect(memoryLabel(null)).toBe(t("common.dash"));
    expect(memoryLabel(undefined)).toBe(t("common.dash"));
    const nothing = { app_bytes: null, core_bytes: null, total_bytes: 0 };
    expect(memoryAvailable(nothing)).toBe(false);
    expect(memoryLabel(nothing)).toBe(t("common.dash"));
    expect(memoryToneFor(nothing)).toBeNull();
  });

  it("uses the readable parts' total", () => {
    const appOnly = { app_bytes: 52 * MB, core_bytes: null, total_bytes: 52 * MB };
    expect(memoryAvailable(appOnly)).toBe(true);
    expect(memoryLabel(appOnly)).toBe("52 MB");
    expect(memoryToneFor(appOnly)).toBe("ok");

    const both = { app_bytes: 38 * MB, core_bytes: 24 * MB, total_bytes: 62 * MB };
    expect(memoryLabel(both)).toBe("62 MB");
    expect(memoryToneFor(both)).toBe("warn");

    const coreOnly = { app_bytes: null, core_bytes: 101 * MB, total_bytes: 101 * MB };
    expect(memoryLabel(coreOnly)).toBe("101 MB");
    expect(memoryToneFor(coreOnly)).toBe("bad");
  });

  it("sums the rounded parts the tooltip prints", () => {
    // 30.4 MB per side rounds to 30 MB each while the byte total (60.8 MB)
    // rounds to 61: the row shows the parts' sum, so the tooltip's
    // `core + app` breakdown adds up to the label, and the band follows that
    // same figure.
    const parts = {
      app_bytes: 30.4 * MB,
      core_bytes: 30.4 * MB,
      total_bytes: 60.8 * MB,
    };
    expect(displayMemoryMb(parts)).toBe(60);
    expect(memoryLabel(parts)).toBe("60 MB");
    expect(memoryToneFor(parts)).toBe("warn");
    expect(formatMemoryPart(parts.app_bytes)).toBe("30 MB");
    expect(formatMemoryPart(parts.core_bytes)).toBe("30 MB");
  });
});

describe("formatMemoryPart", () => {
  it("dashes only the unreadable side", () => {
    expect(formatMemoryPart(24 * MB)).toBe("24 MB");
    expect(formatMemoryPart(null)).toBe(t("common.dash"));
  });
});
