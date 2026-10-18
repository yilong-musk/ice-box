// SPDX-License-Identifier: GPL-3.0-or-later

/** Desktop and the website demo. The mobile build aliases this module to one that returns true. */
export function isPhoneShell(): boolean {
  return false;
}

/** True only in the desktop shell; the website demo also uses `isPhoneShell=false`. */
export function isDesktopShell(): boolean {
  return true;
}
