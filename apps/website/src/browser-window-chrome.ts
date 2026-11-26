// SPDX-License-Identifier: GPL-3.0-or-later

import type { WindowChrome, WindowCommand } from "../../desktop/src/lib/windowChromeContract";
export type { WindowChrome, WindowCommand } from "../../desktop/src/lib/windowChromeContract";

/** Message the embedded desktop demo posts to the landing page, which draws the window frame. */
export const DEMO_RESIZE_MESSAGE = "ice-box:demo-resize";

/** Inside the landing page's window frame (an iframe), not opened on its own. */
const embedded = typeof window !== "undefined" && window.parent !== window;

/**
 * In the landing page's frame the demo is the app window, so it draws the
 * window buttons the Windows build draws (WindowControls: macOS-style lights
 * in the title bar). Same component, so the lights match the app exactly and
 * scale with the demo. Opened on its own there is no window to control.
 */
export function detectWindowChrome(_userAgent?: string, _platform?: string): WindowChrome {
  return embedded ? "windows-custom" : "plain";
}
export function isMacosHost(_userAgent?: string, _platform?: string): boolean { return false; }
/** The demo's iframe has focus only after a click into it; greying the lights
 * whenever the visitor is elsewhere on the page would make it look inactive. */
export function controlsFollowWindowFocus(): boolean { return false; }
export async function runWindowCommand(_command: WindowCommand): Promise<void> {}
/** The demo's frame is never maximized, so the zoom light keeps its plus. */
export function subscribeWindowMaximized(_onChange: (maximized: boolean) => void): () => void {
  return () => {};
}

/**
 * Expected iframe width while the host frame's 200ms CSS width transition is
 * still running, so a mid-transition toggle returns the same target `main.js`
 * computes instead of adding `delta` to an intermediate `window.innerWidth`.
 */
let pendingDemoWidth: { target: number; expiresAt: number } | null = null;

/**
 * The app grows or shrinks its window when the sidebar collapses or expands.
 * Inside the landing page the "window" is the frame around the iframe, so ask
 * the page to resize it by the same delta; the frame animates on its own, on
 * the sidebar's timeline (`transition: width 0.2s linear` in style.css), so
 * `animation` only tunes how long we remember the pending target width. Opened
 * on its own there is no frame, and the page keeps its size. Resolves to the
 * width the frame is heading to, or `null` when no resize was asked.
 */
export async function resizeWindowWidthBy(
  delta: number,
  min: { width: number; height: number },
  animation?: { durationMs: number; startedAt: number },
): Promise<number | null> {
  if (delta === 0 || window.parent === window) return null;
  const now = performance.now();
  const base =
    pendingDemoWidth && now < pendingDemoWidth.expiresAt
      ? pendingDemoWidth.target
      : window.innerWidth;
  const target = Math.max(min.width, base + delta);
  const settleMs = Math.max(250, (animation?.durationMs ?? 200) + 100);
  pendingDemoWidth = { target, expiresAt: now + settleMs };
  window.parent.postMessage(
    { type: DEMO_RESIZE_MESSAGE, delta, minWidth: min.width },
    window.location.origin,
  );
  return target;
}
