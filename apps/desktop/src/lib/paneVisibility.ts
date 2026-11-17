// SPDX-License-Identifier: GPL-3.0-or-later

import { createContext, useContext } from "react";

/**
 * Whether the tab pane around a page is on screen. It follows the tab at
 * once, while the page's own `active` prop may catch up a render later, so
 * page chrome outside the pane (title-bar actions) never shows for a page
 * that is not there.
 */
export const PaneVisibleContext = createContext(true);

export function usePaneVisible(): boolean {
  return useContext(PaneVisibleContext);
}

/**
 * Tab of the pane around a page; null when a page renders on its own. Titles
 * the row of title-bar slots the page's actions portal into.
 */
export const PaneTabContext = createContext<string | null>(null);

export function usePaneTab(): string | null {
  return useContext(PaneTabContext);
}
