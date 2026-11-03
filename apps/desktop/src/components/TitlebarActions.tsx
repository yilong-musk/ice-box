// SPDX-License-Identifier: GPL-3.0-or-later

import {
  createContext,
  useContext,
  useLayoutEffect,
  useState,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";

/**
 * Whether the tab pane around a page is on screen. The shell sets it on the
 * click itself, while a page's own `active` prop may turn false a frame later
 * (deactivation renders at low priority). Title-bar actions follow the pane so
 * a page that is leaving never shows its buttons over the next page.
 */
export const PaneVisibleContext = createContext(true);

/** Page actions that belong in the window title bar, aligned with the content. */
export function TitlebarActions({ children }: { children: ReactNode }) {
  const paneVisible = useContext(PaneVisibleContext);
  const [slot, setSlot] = useState<Element | null>(null);
  useLayoutEffect(() => {
    setSlot(document.querySelector("[data-titlebar-actions]"));
  }, []);
  if (slot == null || !paneVisible) return null;
  return createPortal(children, slot);
}
