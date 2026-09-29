// SPDX-License-Identifier: GPL-3.0-or-later

import type { WindowChrome, WindowCommand } from "../../desktop/src/lib/windowChromeContract";
export type { WindowChrome, WindowCommand } from "../../desktop/src/lib/windowChromeContract";

export function detectWindowChrome(_userAgent?: string, _platform?: string): WindowChrome { return "plain"; }
export function isMacosHost(_userAgent?: string, _platform?: string): boolean { return false; }
export async function runWindowCommand(_command: WindowCommand): Promise<void> {}
