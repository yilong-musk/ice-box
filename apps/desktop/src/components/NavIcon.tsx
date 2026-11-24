// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Navigation icons: rounded line glyphs on a 24-unit grid, in the spirit of
 * SF Symbols. One ink: strokes take the menu button's text color, so a
 * selected item and an idle one differ the way the label does. Drawn at
 * 16px in the expanded sidebar and the phone tab bar, 24px on the collapsed
 * icon rail (see `.nav-icon` in index.css); the stroke scales with the size.
 */

import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export const NAV_ICON_NAMES = [
  "home",
  "nodes",
  "apps",
  "rules",
  "subs",
  "connections",
  "mcp",
  "settings",
] as const;

export type NavIconName = (typeof NAV_ICON_NAMES)[number];

const GLYPHS: Record<NavIconName, ReactNode> = {
  // House with a door.
  home: (
    <>
      <path d="M4 10.2 12 4l8 6.2V19a1.5 1.5 0 0 1-1.5 1.5H15v-5.25a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1V20.5H5.5A1.5 1.5 0 0 1 4 19z" />
    </>
  ),
  // Three linked nodes.
  nodes: (
    <>
      <circle cx="12" cy="5.5" r="2.25" />
      <circle cx="5.5" cy="17.5" r="2.25" />
      <circle cx="18.5" cy="17.5" r="2.25" />
      <path d="M10.9 7.5 6.6 15.5M13.1 7.5l4.3 8M7.75 17.5h8.5" />
    </>
  ),
  // Four app tiles.
  apps: (
    <>
      <rect x="4" y="4" width="6.5" height="6.5" rx="1.75" />
      <rect x="13.5" y="4" width="6.5" height="6.5" rx="1.75" />
      <rect x="4" y="13.5" width="6.5" height="6.5" rx="1.75" />
      <rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.75" />
    </>
  ),
  // A route that branches off: routing rules.
  rules: (
    <>
      <path d="M7 3.5v17" />
      <path d="M7 14c0-4 2.5-7 7-7h5.5" />
      <path d="m16.75 4.25 2.75 2.75-2.75 2.75" />
    </>
  ),
  // Stacked layers: a feed of nodes.
  subs: (
    <>
      <path d="m12 3.75 8.5 4.5-8.5 4.5-8.5-4.5z" />
      <path d="m3.5 12.25 8.5 4.5 8.5-4.5" />
      <path d="m3.5 16 8.5 4.5 8.5-4.5" />
    </>
  ),
  // Up and down arrows: live traffic.
  connections: (
    <>
      <path d="M8 19.5v-15M4.5 8 8 4.5 11.5 8" />
      <path d="M16 4.5v15M12.5 16l3.5 3.5 3.5-3.5" />
    </>
  ),
  // A robot: the MCP server that agents talk to. Same shape as the Bot icon
  // on the MCP page (wide head, bent antenna, side ears, two upright eyes).
  mcp: (
    <>
      <path d="M12 8V4H8" />
      <rect x="4" y="8" width="16" height="12" rx="2.5" />
      <path d="M2 14h2M20 14h2" />
      <path d="M9 13v2M15 13v2" />
    </>
  ),
  // Two sliders.
  settings: (
    <>
      <path d="M4 7.5h8.5M17.5 7.5H20" />
      <circle cx="15" cy="7.5" r="2.5" />
      <path d="M4 16.5h2.5M11.5 16.5H20" />
      <circle cx="9" cy="16.5" r="2.5" />
    </>
  ),
};

export function NavIcon({
  name,
  className,
}: {
  name: NavIconName;
  className?: string;
}) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={16}
      height={16}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      data-icon={name}
      className={cn("nav-icon", className)}
    >
      {GLYPHS[name]}
    </svg>
  );
}
