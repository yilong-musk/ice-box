// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Browser demo composition root. `mobile-demo.html` marks the document with
 * `<html data-shell="phone">`; every other entry keeps the desktop shell.
 */
function demoShell(): "desktop" | "phone" {
  return document.documentElement.dataset.shell === "phone"
    ? "phone"
    : "desktop";
}

export function isPhoneShell(): boolean {
  return demoShell() === "phone";
}

export function isDesktopShell(): boolean {
  // The Live Demo mirrors the desktop shell so desktop-only pages such as MCP
  // are visible; the backend behind them is simulated.
  return demoShell() === "desktop";
}
