// SPDX-License-Identifier: GPL-3.0-or-later

/** A non-collapsed selection whose ends both sit inside the log view. */
export type LogSelection = {
  text: string;
  anchor: Node;
  anchorOffset: number;
  focus: Node;
  focusOffset: number;
};

/** Text the user has selected inside `box`, using the same string Copy would. */
export function readLogSelection(box: HTMLElement): LogSelection | null {
  const selection = window.getSelection();
  if (selection == null || selection.isCollapsed || selection.rangeCount === 0) return null;
  const anchor = selection.anchorNode;
  const focus = selection.focusNode;
  if (anchor == null || focus == null) return null;
  if (!box.contains(anchor) || !box.contains(focus)) return null;
  const text = selection.toString();
  if (text.length === 0) return null;
  return {
    text,
    anchor,
    anchorOffset: selection.anchorOffset,
    focus,
    focusOffset: selection.focusOffset,
  };
}

export function sameLogSelection(a: LogSelection | null, b: LogSelection | null): boolean {
  if (a == null || b == null) return false;
  return (
    a.anchor === b.anchor &&
    a.anchorOffset === b.anchorOffset &&
    a.focus === b.focus &&
    a.focusOffset === b.focusOffset
  );
}

/**
 * Copy `text` during a user gesture. The async clipboard API is rejected by
 * some webviews; fall back to `execCommand` and put the log selection back.
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
