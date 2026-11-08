// SPDX-License-Identifier: GPL-3.0-or-later

import { useLayoutEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { usePaneVisible } from "@/lib/paneVisibility";

/** Page actions that belong in the window title bar, aligned with the content.
 * Nothing renders while the page's pane is hidden. */
export function TitlebarActions({ children }: { children: ReactNode }) {
  const visible = usePaneVisible();
  const [slot, setSlot] = useState<Element | null>(null);
  useLayoutEffect(() => {
    setSlot(document.querySelector("[data-titlebar-actions]"));
  }, []);
  if (slot == null || !visible) return null;
  return createPortal(children, slot);
}
