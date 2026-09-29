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
    }
  } catch {
    // Browser preview and unit tests are not inside Tauri.
  }
}
