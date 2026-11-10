// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { t } from "./i18n";
import {
  DEFAULT_MEMORY_BANDS,
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

/** Bands as the backend sends them: the baseline, and the macOS shift. */
const BASE = { warn_mb: 60, bad_mb: 100 };
const MACOS = { warn_mb: 80, bad_mb: 120 };

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
    expect(memoryTone(memoryMb(59.6 * MB), BASE)).toBe("warn");
  });
});

describe("memoryTone", () => {
  it("bands <60 green, 60–99 yellow, ≥100 red", () => {
    expect(memoryTone(0, BASE)).toBe("ok");
    expect(memoryTone(59, BASE)).toBe("ok");
    expect(memoryTone(60, BASE)).toBe("warn");
    expect(memoryTone(99, BASE)).toBe("warn");
    expect(memoryTone(100, BASE)).toBe("bad");
    expect(memoryTone(1024, BASE)).toBe("bad");
  });

  it("follows the bands it is given", () => {
    expect(memoryTone(79, MACOS)).toBe("ok");
    expect(memoryTone(80, MACOS)).toBe("warn");
    expect(memoryTone(119, MACOS)).toBe("warn");
    expect(memoryTone(120, MACOS)).toBe("bad");
  });

  it("keeps baseline bands for a card with no sample", () => {
    expect(DEFAULT_MEMORY_BANDS).toEqual(BASE);
    expect(Object.isFrozen(DEFAULT_MEMORY_BANDS)).toBe(true);
  });
});

describe("memoryLabel / memoryToneFor", () => {
  it("shows a dash when nothing could be read", () => {
    expect(memoryLabel(null)).toBe(t("common.dash"));
    expect(memoryLabel(undefined)).toBe(t("common.dash"));
    const nothing = { app_bytes: null, core_bytes: null, total_bytes: 0, bands: BASE };
    expect(memoryAvailable(nothing)).toBe(false);
    expect(memoryLabel(nothing)).toBe(t("common.dash"));
    expect(memoryToneFor(nothing)).toBeNull();
  });

  it("uses the readable parts' total", () => {
    const appOnly = { app_bytes: 52 * MB, core_bytes: null, total_bytes: 52 * MB, bands: BASE };
    expect(memoryAvailable(appOnly)).toBe(true);
    expect(memoryLabel(appOnly)).toBe("52 MB");
    expect(memoryToneFor(appOnly)).toBe("ok");

    const both = { app_bytes: 38 * MB, core_bytes: 24 * MB, total_bytes: 62 * MB, bands: BASE };
    expect(memoryLabel(both)).toBe("62 MB");
    expect(memoryToneFor(both)).toBe("warn");

    const coreOnly = { app_bytes: null, core_bytes: 101 * MB, total_bytes: 101 * MB, bands: BASE };
    expect(memoryLabel(coreOnly)).toBe("101 MB");
    expect(memoryToneFor(coreOnly)).toBe("bad");
  });

  it("bands the row with the sample's own bands", () => {
    // 72 MB: yellow against the baseline, green against the macOS bands.
    const sample = { app_bytes: 50 * MB, core_bytes: 22 * MB, total_bytes: 72 * MB };
    expect(memoryToneFor({ ...sample, bands: BASE })).toBe("warn");
    expect(memoryToneFor({ ...sample, bands: MACOS })).toBe("ok");
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
      bands: BASE,
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
