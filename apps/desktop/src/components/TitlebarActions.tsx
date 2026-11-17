// SPDX-License-Identifier: GPL-3.0-or-later

import { useLayoutEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { usePaneTab, usePaneVisible } from "@/lib/paneVisibility";

/** Page actions that belong in the window title bar, aligned with the content.
 * Nothing renders while the page's pane is hidden.
 *
 * Inside the shell each page portals into the slot of its own tab. The shell
 * hides that slot the moment its tab leaves, even while the page's hidden
 * subtree still awaits a deferred render, so an outgoing page's buttons never
 * linger in the bar next to the incoming page's. A page rendered on its own
 * (tests) falls back to the shared slot. */
export function TitlebarActions({ children }: { children: ReactNode }) {
  const visible = usePaneVisible();
  const pane = usePaneTab();
  const [slot, setSlot] = useState<Element | null>(null);
  useLayoutEffect(() => {
    const own = pane
      ? document.querySelector(`[data-titlebar-slot="${pane}"]`)
      : null;
    setSlot(own ?? document.querySelector("[data-titlebar-actions]"));
  }, [pane]);
  if (slot == null || !visible) return null;
  return createPortal(children, slot);
}
