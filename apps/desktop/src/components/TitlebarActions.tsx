// SPDX-License-Identifier: GPL-3.0-or-later

import { useLayoutEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

/** Page actions that belong in the window title bar, aligned with the content. */
export function TitlebarActions({ children }: { children: ReactNode }) {
  const [slot, setSlot] = useState<Element | null>(null);
  useLayoutEffect(() => {
    setSlot(document.querySelector("[data-titlebar-actions]"));
  }, []);
  if (slot == null) return null;
  return createPortal(children, slot);
}
