// SPDX-License-Identifier: GPL-3.0-or-later

import { afterEach, describe, expect, it, vi } from "vitest";
import { copyText } from "./clipboard";

function selectText(node: Text, start: number, end: number): void {
  const range = document.createRange();
  range.setStart(node, start);
  range.setEnd(node, end);
  const selection = window.getSelection();
  if (selection == null) throw new Error("missing selection");
  selection.removeAllRanges();
  selection.addRange(range);
}

describe("copyText", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    window.getSelection()?.removeAllRanges();
    document.body.replaceChildren();
  });

  it("writes through the async clipboard API", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    await copyText("INFO ready");
    expect(writeText).toHaveBeenCalledWith("INFO ready");
  });

  it("falls back to execCommand and restores the page selection", async () => {
    const writeText = vi.fn().mockRejectedValue(new Error("denied"));
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    const node = document.createTextNode("INFO ready");
    document.body.appendChild(node);
    selectText(node, 0, 4);
    const exec = vi.fn(() => true);
    document.execCommand = exec;

    await copyText("INFO");

    expect(exec).toHaveBeenCalledWith("copy");
    expect(window.getSelection()?.toString()).toBe("INFO");
  });

  it("rejects when neither clipboard path works", async () => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
    });
    document.execCommand = () => false;
    await expect(copyText("INFO")).rejects.toThrow(/clipboard unavailable/);
  });
});
