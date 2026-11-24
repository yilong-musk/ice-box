// SPDX-License-Identifier: GPL-3.0-or-later

import type { WindowChrome, WindowCommand } from "./windowChromeContract";
export type { WindowChrome, WindowCommand } from "./windowChromeContract";

/** Classify the native chrome so the UI can inset traffic lights or draw caption buttons. */
export function detectWindowChrome(
  userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent,
  platform = typeof navigator === "undefined" ? "" : navigator.platform,
): WindowChrome {
  const haystack = `${platform} ${userAgent}`;
  if (/Windows|Win32|Win64/i.test(haystack)) return "windows-custom";
  if (/Mac|iPhone|iPad|darwin/i.test(haystack)) return "macos-overlay";
  return "plain";
}

/** True when the webview's chrome class is the macOS overlay. Product
 * settings use status flags (`tray_display_supported` and the other
 * `*_supported` fields); this classifier also matches iPhone and iPad. */
export function isMacosHost(
  userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent,
  platform = typeof navigator === "undefined" ? "" : navigator.platform,
): boolean {
  return detectWindowChrome(userAgent, platform) === "macos-overlay";
}

/**
 * Grow or shrink the window's width by `delta` CSS px, keeping its left edge,
 * and set its minimum size. A maximized or fullscreen window keeps its size
 * (only the minimum changes). The minimum is lowered before shrinking and
 * raised after growing, so it never blocks the resize.
 */
export async function resizeWindowWidthBy(
  delta: number,
  min: { width: number; height: number },
): Promise<void> {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const { LogicalSize } = await import("@tauri-apps/api/dpi");
    const current = getCurrentWindow();
    const minSize = new LogicalSize(min.width, min.height);
    const fixed = (await current.isMaximized()) || (await current.isFullscreen());
    if (fixed || delta === 0) {
      await current.setMinSize(minSize);
      return;
    }
    const scale = await current.scaleFactor();
    const inner = (await current.innerSize()).toLogical(scale);
    const next = new LogicalSize(Math.max(min.width, inner.width + delta), inner.height);
    if (delta < 0) {
      await current.setMinSize(minSize);
      await current.setSize(next);
    } else {
      await current.setSize(next);
      await current.setMinSize(minSize);
    }
  } catch {
    // Browser preview and unit tests are not inside Tauri.
  }
}

export async function runWindowCommand(command: WindowCommand): Promise<void> {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const current = getCurrentWindow();
    switch (command) {
      case "minimize":
        await current.minimize();
        return;
      case "toggleMaximize":
        await current.toggleMaximize();
        return;
      case "close":
        await current.close();
        return;
      case "startDragging":
        await current.startDragging();
        return;
    }
  } catch {
    // Browser preview and unit tests are not inside Tauri.
  }
}
