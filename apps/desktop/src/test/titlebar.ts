// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Stand-in for the window title-bar action slot that `TitlebarActions`
 * portals a page's buttons into. `App` renders the real slot; a page rendered
 * on its own in a test needs one in the document or its actions go nowhere.
 */
export function installTitlebarActionsSlot(): void {
  const slot = document.createElement("div");
  slot.dataset.titlebarActions = "";
  document.body.appendChild(slot);
}

export function removeTitlebarActionsSlot(): void {
  document.querySelector("[data-titlebar-actions]")?.remove();
}

/** The slot the rendered page currently portals into. */
export function titlebarActionsSlot(): HTMLElement | null {
  return document.querySelector("[data-titlebar-actions]");
}
