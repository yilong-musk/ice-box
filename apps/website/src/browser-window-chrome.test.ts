// SPDX-License-Identifier: GPL-3.0-or-later

import assert from "node:assert/strict";
import test from "node:test";

test("resizeWindowWidthBy tracks the pending target width across mid-transition toggles", async () => {
  const posted: Array<{ msg: unknown; origin: string }> = [];
  let innerWidth = 740;
  const parent = {
    postMessage(msg: unknown, origin: string) {
      posted.push({ msg, origin });
    },
  };
  const fakeWindow = {
    parent,
    location: { origin: "https://example.test" },
    get innerWidth() {
      return innerWidth;
    },
  };
  const prevWindow = globalThis.window;
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: fakeWindow,
  });
  try {
    const { DEMO_RESIZE_MESSAGE, resizeWindowWidthBy } = await import(
      "./browser-window-chrome.ts"
    );
    const first = await resizeWindowWidthBy(
      120,
      { width: 720, height: 500 },
      { durationMs: 200, startedAt: performance.now() },
    );
    assert.equal(first, 860);

    // Mid-transition: the iframe is partway to 860 when the user collapses again.
    innerWidth = 800;
    const second = await resizeWindowWidthBy(
      -120,
      { width: 600, height: 500 },
      { durationMs: 200, startedAt: performance.now() },
    );
    assert.equal(second, 740);
    assert.deepEqual(posted, [
      {
        msg: { type: DEMO_RESIZE_MESSAGE, delta: 120, minWidth: 720 },
        origin: "https://example.test",
      },
      {
        msg: { type: DEMO_RESIZE_MESSAGE, delta: -120, minWidth: 600 },
        origin: "https://example.test",
      },
    ]);
  } finally {
    if (prevWindow === undefined) {
      delete (globalThis as { window?: unknown }).window;
    } else {
      Object.defineProperty(globalThis, "window", {
        configurable: true,
        value: prevWindow,
      });
    }
  }
});
