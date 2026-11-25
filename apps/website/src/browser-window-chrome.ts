// SPDX-License-Identifier: GPL-3.0-or-later

import type { WindowChrome, WindowCommand } from "../../desktop/src/lib/windowChromeContract";
export type { WindowChrome, WindowCommand } from "../../desktop/src/lib/windowChromeContract";

/** Message the embedded desktop demo posts to the landing page, which draws the window frame. */
export const DEMO_RESIZE_MESSAGE = "ice-box:demo-resize";

export function detectWindowChrome(_userAgent?: string, _platform?: string): WindowChrome { return "plain"; }
export function isMacosHost(_userAgent?: string, _platform?: string): boolean { return false; }
export async function runWindowCommand(_command: WindowCommand): Promise<void> {}

/**
 * The app grows or shrinks its window when the sidebar collapses or expands.
 * Inside the landing page the "window" is the frame around the iframe, so ask
 * the page to resize it by the same delta; the frame animates on its own, on
 * the sidebar's timeline (`transition: width 0.2s linear` in style.css), so
 * `_animation` is not needed. Opened on its own there is no frame, and the
 * page keeps its size. Resolves to the width the frame is heading to, or
 * `null` when no resize was asked.
 */
export async function resizeWindowWidthBy(
  delta: number,
  min: { width: number; height: number },
  _animation?: { durationMs: number; startedAt: number },
): Promise<number | null> {
  if (delta === 0 || window.parent === window) return null;
  window.parent.postMessage(
    { type: DEMO_RESIZE_MESSAGE, delta, minWidth: min.width },
    window.location.origin,
  );
  return Math.max(min.width, window.innerWidth + delta);
}
