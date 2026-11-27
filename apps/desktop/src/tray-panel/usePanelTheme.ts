// SPDX-License-Identifier: GPL-3.0-or-later

import { useEffect, useState } from "react";
import {
  readThemePreference,
  resolveTheme,
  THEME_STORAGE_KEY,
} from "../lib/theme";

export interface PanelTheme {
  dark: boolean;
  /** Explicit dark/light preference, or `null` when following the OS theme. */
  explicit: boolean | null;
}

function readPanelTheme(): PanelTheme {
  const preference = readThemePreference();
  const dark = resolveTheme(preference) === "dark";
  if (preference === "system") {
    return { dark, explicit: null };
  }
  return { dark, explicit: dark };
}

/**
 * Sync the standalone tray panel WebView with the main window's theme choice
 * stored in localStorage and with OS colour-scheme changes.
 */
export function usePanelTheme(refreshKey: unknown): PanelTheme {
  const [theme, setTheme] = useState<PanelTheme>(readPanelTheme);

  useEffect(() => {
    setTheme(readPanelTheme());
  }, [refreshKey]);

  useEffect(() => {
    const update = () => setTheme(readPanelTheme());
    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    media?.addEventListener("change", update);
    const onStorage = (event: StorageEvent) => {
      if (event.key === null || event.key === THEME_STORAGE_KEY) {
        update();
      }
    };
    window.addEventListener("storage", onStorage);
    return () => {
      media?.removeEventListener("change", update);
      window.removeEventListener("storage", onStorage);
    };
  }, []);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme.dark);
    document.documentElement.style.colorScheme = theme.dark ? "dark" : "light";
  }, [theme.dark]);

  return theme;
}
