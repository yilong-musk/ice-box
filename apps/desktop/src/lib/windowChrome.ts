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

/** Whether the drawn window buttons grey out while the window is not
 * focused, as macOS does. The app's webview is the window, so yes. */
export function controlsFollowWindowFocus(): boolean {
  return true;
}

/** Timing of an animated window resize, matched to a CSS transition. */
export interface WindowResizeAnimation {
  /** Length of the (linear) animation in ms. */
  durationMs: number;
  /** `performance.now()` at which the matching CSS transition started. */
  startedAt: number;
}

/** Cached Tauri window APIs so repeated or concurrent toggles reuse one import. */
let tauriApisPromise: Promise<{
  getCurrentWindow: typeof import("@tauri-apps/api/window").getCurrentWindow;
  LogicalSize: typeof import("@tauri-apps/api/dpi").LogicalSize;
}> | null = null;

function loadTauriApis() {
  if (!tauriApisPromise) {
    tauriApisPromise = Promise.all([
      import("@tauri-apps/api/window"),
      import("@tauri-apps/api/dpi"),
    ])
      .then(([win, dpi]) => ({
        getCurrentWindow: win.getCurrentWindow,
        LogicalSize: dpi.LogicalSize,
      }))
      .catch((err) => {
        tauriApisPromise = null;
        throw err;
      });
  }
  return tauriApisPromise;
}

/** The resize in flight; a newer call cancels it and takes over. */
let activeResize: { cancelled: boolean } | null = null;
/** Logical width the in-flight resize is heading to, once measured. */
let pendingTargetWidth: number | null = null;
/** Unapplied width delta queued synchronously before `innerSize()` resolves. */
let pendingDelta = 0;

function nextFrame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()));
}

/**
 * Grow or shrink the window's width by `delta` CSS px, keeping its left edge,
 * and set its minimum size. A maximized or fullscreen window keeps its size
 * (only the minimum changes). The minimum is lowered before shrinking and
 * raised after growing, so it never blocks the resize.
 *
 * With `animation`, the width is stepped once per frame along the same linear
 * timeline as the CSS transition it accompanies, so the window edge moves with
 * the content instead of jumping when the transition is over. Progress is
 * measured from `startedAt`, so time spent querying the window is caught up
 * on the first frame. A call made while another resize is still running
 * cancels it and continues from the width that one was heading to (or
 * accumulates the queued delta if the earlier call has not measured the window
 * yet), so a quick double toggle always lands back on the original width.
 *
 * Resolves to the logical width the window was resized to, or `null` when its
 * size was left alone (maximized, fullscreen, no net delta, not inside Tauri,
 * or superseded before it started resizing).
 */
export async function resizeWindowWidthBy(
  delta: number,
  min: { width: number; height: number },
  animation?: WindowResizeAnimation,
): Promise<number | null> {
  if (activeResize) activeResize.cancelled = true;
  const run = { cancelled: false };
  activeResize = run;
  pendingDelta += delta;
  const finish = () => {
    if (activeResize !== run) return;
    activeResize = null;
    pendingTargetWidth = null;
    pendingDelta = 0;
  };
  try {
    const { getCurrentWindow, LogicalSize } = await loadTauriApis();
    const current = getCurrentWindow();
    const minSize = new LogicalSize(min.width, min.height);
    const [maximized, fullscreen] = await Promise.all([
      current.isMaximized(),
      current.isFullscreen(),
    ]);
    if (run.cancelled) return null;
    if (maximized || fullscreen || (pendingTargetWidth == null && pendingDelta === 0)) {
      await current.setMinSize(minSize);
      finish();
      return null;
    }
    const scale = await current.scaleFactor();
    const inner = (await current.innerSize()).toLogical(scale);
    if (run.cancelled) return null;
    const from = inner.width;
    const to = Math.max(min.width, (pendingTargetWidth ?? from) + pendingDelta);
    pendingTargetWidth = to;
    pendingDelta = 0;
    const shrinking = to < from;
    if (shrinking) await current.setMinSize(minSize);
    const duration = animation?.durationMs ?? 0;
    const startedAt = animation?.startedAt ?? performance.now();
    for (;;) {
      if (run.cancelled) return to;
      const progress =
        duration > 0 ? Math.min(1, (performance.now() - startedAt) / duration) : 1;
      const width = progress >= 1 ? to : Math.round(from + (to - from) * progress);
      // Awaited, so at most one resize is queued and the window never lags
      // behind a backlog of stale sizes.
      await current.setSize(new LogicalSize(width, inner.height));
      if (progress >= 1) break;
      await nextFrame();
    }
    if (run.cancelled) return to;
    if (!shrinking) await current.setMinSize(minSize);
    finish();
    return to;
  } catch {
    // Browser preview and unit tests are not inside Tauri.
    finish();
    return null;
  }
}

/**
 * Report whether the window is maximized: once at start, then whenever it is
 * resized (the zoom light, a title-bar double-click, Win+Up, snapping and
 * restoring all resize it). Only changes are reported. Resize events are
 * coalesced: at most one query runs per animation frame, and queries never
 * overlap, so dragging an edge or an animated resize does not cost an IPC
 * round trip per event. Returns an unsubscribe function; outside Tauri
 * nothing is reported.
 */
export function subscribeWindowMaximized(
  onChange: (maximized: boolean) => void,
): () => void {
  let disposed = false;
  let unlisten: (() => void) | null = null;
  void (async () => {
    try {
      const { getCurrentWindow } = await loadTauriApis();
      const current = getCurrentWindow();
      let last: boolean | null = null;
      // A check is queued or running; resizes meanwhile only mark it stale.
      let busy = false;
      let stale = false;
      const refresh = async () => {
        busy = true;
        try {
          do {
            await nextFrame();
            if (disposed) return;
            // Resizes from here on happen after this query starts, so they
            // need another one.
            stale = false;
            const maximized = await current.isMaximized();
            if (disposed) return;
            if (maximized !== last) {
              last = maximized;
              onChange(maximized);
            }
          } while (stale);
        } catch {
          // The window is going away; keep the last known state.
        } finally {
          busy = false;
        }
      };
      const request = () => {
        if (busy) stale = true;
        else void refresh();
      };
      const off = await current.onResized(request);
      if (disposed) {
        off();
        return;
      }
      unlisten = off;
      // The start state; a resize that already queued a check covers it.
      request();
    } catch {
      // Browser preview and unit tests are not inside Tauri.
    }
  })();
  return () => {
    disposed = true;
    unlisten?.();
  };
}

export async function runWindowCommand(command: WindowCommand): Promise<void> {
  try {
    const { getCurrentWindow } = await loadTauriApis();
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
