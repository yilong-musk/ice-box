// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Copy `text` during a user gesture. The async clipboard API is rejected by
 * some webviews; fall back to `execCommand` and put the page selection back.
 */
export async function copyText(text: string): Promise<void> {
  const clipboard = navigator.clipboard;
  if (clipboard != null && typeof clipboard.writeText === "function") {
    try {
      await clipboard.writeText(text);
      return;
    } catch {
      // Fall through to the synchronous path while the gesture is still active.
    }
  }
  if (!copyWithExecCommand(text)) {
    throw new Error("clipboard unavailable");
  }
}

function copyWithExecCommand(text: string): boolean {
  if (typeof document.execCommand !== "function") return false;
  const selection = window.getSelection();
  const ranges: Range[] = [];
  if (selection != null) {
    for (let i = 0; i < selection.rangeCount; i += 1) {
      ranges.push(selection.getRangeAt(i).cloneRange());
    }
  }
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.top = "0";
  area.style.left = "0";
  area.style.opacity = "0";
  document.body.appendChild(area);
  area.focus();
  area.select();
  let copied = false;
  try {
    copied = document.execCommand("copy");
  } catch {
    copied = false;
  } finally {
    area.remove();
    if (selection != null) {
      selection.removeAllRanges();
      for (const range of ranges) selection.addRange(range);
    }
  }
  return copied;
}
