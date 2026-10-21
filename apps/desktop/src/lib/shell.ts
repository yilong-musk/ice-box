// SPDX-License-Identifier: GPL-3.0-or-later

/** Desktop and the desktop website demo. The mobile build and the phone demo
 * alias this module. */
export function isPhoneShell(): boolean {
  return false;
}

/** True only in the desktop shell; the desktop website demo also uses `isPhoneShell=false`. */
export function isDesktopShell(): boolean {
  return true;
}
