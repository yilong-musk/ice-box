// SPDX-License-Identifier: GPL-3.0-or-later

/** Presentation capabilities shared by native and browser window adapters. */
export type WindowChrome = "macos-overlay" | "windows-custom" | "plain";
export type WindowCommand = "minimize" | "toggleMaximize" | "close";
