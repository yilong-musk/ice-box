// SPDX-License-Identifier: GPL-3.0-or-later

/** Browser demo composition root. */
export function isPhoneShell(): boolean {
  return false;
}

export function isDesktopShell(): boolean {
  // The Live Demo mirrors the desktop shell so desktop-only pages such as MCP
  // are visible; the backend behind them is simulated.
  return true;
}
