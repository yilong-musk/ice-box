// SPDX-License-Identifier: GPL-3.0-or-later

import type { WindowChrome, WindowCommand } from "../../desktop/src/lib/windowChromeContract";

export type { WindowChrome, WindowCommand } from "../../desktop/src/lib/windowChromeContract";

/** The phone shell draws neither traffic lights nor caption buttons. */
export function detectWindowChrome(): WindowChrome {
  return "plain";
}

export function isMacosHost(): boolean {
  return false;
}

export async function runWindowCommand(_command: WindowCommand): Promise<void> {}

/** The phone window is sized by the system; nothing is resized. */
export async function resizeWindowWidthBy(
  _delta: number,
  _min: { width: number; height: number },
  _animation?: { durationMs: number; startedAt: number },
): Promise<number | null> {
  return null;
}
