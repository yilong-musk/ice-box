// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { PhysicalSize, type LogicalSize } from "@tauri-apps/api/dpi";
import {
  detectWindowChrome,
  isMacosHost,
  resizeWindowWidthBy,
  runWindowCommand,
} from "./windowChrome";

const minimize = vi.fn().mockResolvedValue(undefined);
const toggleMaximize = vi.fn().mockResolvedValue(undefined);
const close = vi.fn().mockResolvedValue(undefined);

/** A fake window at scale 1 whose inner width follows `setSize`. */
const fakeWindow = vi.hoisted(() => ({
  width: 720,
  height: 620,
  maximized: false,
  calls: [] as string[],
}));
const setSize = vi.fn(async (size: LogicalSize) => {
  fakeWindow.width = size.width;
  fakeWindow.height = size.height;
  fakeWindow.calls.push(`size:${size.width}`);
});
const setMinSize = vi.fn(async (size: LogicalSize) => {
  fakeWindow.calls.push(`min:${size.width}`);
});

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    minimize,
    toggleMaximize,
    close,
    isMaximized: async () => fakeWindow.maximized,
    isFullscreen: async () => false,
    scaleFactor: async () => 1,
    innerSize: async () => new PhysicalSize(fakeWindow.width, fakeWindow.height),
    setSize,
    setMinSize,
  }),
}));

function resetFakeWindow(width: number) {
  fakeWindow.width = width;
  fakeWindow.height = 620;
  fakeWindow.maximized = false;
  fakeWindow.calls = [];
  setSize.mockClear();
  setMinSize.mockClear();
}

describe("detectWindowChrome", () => {
  it("uses overlay chrome on macOS and jsdom darwin", () => {
    expect(detectWindowChrome("Mozilla/5.0 (darwin) jsdom/26", "")).toBe(
      "macos-overlay",
    );
    expect(
      detectWindowChrome(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
        "MacIntel",
      ),
    ).toBe("macos-overlay");
  });

  it("uses custom caption buttons on Windows", () => {
    expect(
      detectWindowChrome(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
        "Win32",
      ),
    ).toBe("windows-custom");
  });

  it("leaves Linux on the default decorated window", () => {
    expect(detectWindowChrome("Mozilla/5.0 (X11; Linux x86_64)", "Linux x86_64")).toBe(
      "plain",
    );
  });
});

describe("isMacosHost", () => {
  it("gates host-specific settings on macOS only", () => {
    expect(
      isMacosHost("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", "MacIntel"),
    ).toBe(true);
    expect(
      isMacosHost("Mozilla/5.0 (Windows NT 10.0; Win64; x64)", "Win32"),
    ).toBe(false);
    expect(isMacosHost("Mozilla/5.0 (X11; Linux x86_64)", "Linux x86_64")).toBe(
      false,
    );
  });
});

describe("runWindowCommand", () => {
  it("forwards commands to the current Tauri window", async () => {
    await runWindowCommand("minimize");
    await runWindowCommand("toggleMaximize");
    await runWindowCommand("close");

    expect(minimize).toHaveBeenCalledTimes(1);
    expect(toggleMaximize).toHaveBeenCalledTimes(1);
    expect(close).toHaveBeenCalledTimes(1);
  });
});

describe("resizeWindowWidthBy", () => {
  it("grows in one step without an animation and raises the minimum afterwards", async () => {
    resetFakeWindow(720);
    await expect(resizeWindowWidthBy(120, { width: 720, height: 500 })).resolves.toBe(840);
    expect(fakeWindow.calls).toEqual(["size:840", "min:720"]);
  });

  it("lowers the minimum before shrinking and never goes below it", async () => {
    resetFakeWindow(700);
    await expect(resizeWindowWidthBy(-120, { width: 600, height: 500 })).resolves.toBe(600);
    expect(fakeWindow.calls).toEqual(["min:600", "size:600"]);
  });

  it("leaves a maximized window alone", async () => {
    resetFakeWindow(1280);
    fakeWindow.maximized = true;
    await expect(resizeWindowWidthBy(120, { width: 720, height: 500 })).resolves.toBeNull();
    expect(setSize).not.toHaveBeenCalled();
    expect(fakeWindow.calls).toEqual(["min:720"]);
  });

  it("steps the width along the transition timeline and ends on the target", async () => {
    resetFakeWindow(720);
    const target = await resizeWindowWidthBy(
      120,
      { width: 720, height: 500 },
      { durationMs: 80, startedAt: performance.now() },
    );
    expect(target).toBe(840);
    const widths = setSize.mock.calls.map(([size]) => size.width);
    expect(widths.length).toBeGreaterThan(1);
    expect(widths[widths.length - 1]).toBe(840);
    for (let i = 1; i < widths.length; i++) {
      expect(widths[i]).toBeGreaterThanOrEqual(widths[i - 1]);
    }
    // The minimum rises only once the window is wide enough.
    expect(fakeWindow.calls[fakeWindow.calls.length - 1]).toBe("min:720");
  });

  it("continues from the pending target when a newer resize takes over", async () => {
    resetFakeWindow(720);
    const grow = resizeWindowWidthBy(
      120,
      { width: 720, height: 500 },
      { durationMs: 200, startedAt: performance.now() },
    );
    // Let the grow start stepping, then toggle back mid-way.
    await new Promise((resolve) => setTimeout(resolve, 40));
    const shrink = resizeWindowWidthBy(
      -120,
      { width: 600, height: 500 },
      { durationMs: 60, startedAt: performance.now() },
    );
    await expect(grow).resolves.toBe(840);
    await expect(shrink).resolves.toBe(720);
    expect(fakeWindow.width).toBe(720);
    // The superseded grow never raised the minimum.
    expect(fakeWindow.calls).not.toContain("min:720");
  });
});
