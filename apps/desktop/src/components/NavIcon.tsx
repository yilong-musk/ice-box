// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * A 16px navigation glyph from docs/brand/nav-icons. One ink: the mark takes
 * the menu button's text color, so a selected item and an idle one differ the
 * way the label does. Shown at native size so the grid stays crisp.
 */

import { cn } from "@/lib/utils";
import { NAV_GLYPHS, type NavGlyphName } from "./navGlyphs";

export const NAV_ICON_NAMES = [
  "home",
  "nodes",
  "apps",
  "rules",
  "subs",
  "connections",
  "mcp",
  "settings",
] as const satisfies readonly NavGlyphName[];

export type NavIconName = (typeof NAV_ICON_NAMES)[number];

function glyphPath(rows: readonly string[]): string {
  let d = "";
  for (let y = 0; y < rows.length; y++) {
    const row = rows[y];
    for (let x = 0; x < row.length; x++) {
      if (row[x] === "#") d += `M${x} ${y}h1v1h-1z`;
    }
  }
  return d;
}

const PATHS: Record<NavIconName, string> = {
  home: glyphPath(NAV_GLYPHS.home),
  nodes: glyphPath(NAV_GLYPHS.nodes),
  apps: glyphPath(NAV_GLYPHS.apps),
  rules: glyphPath(NAV_GLYPHS.rules),
  subs: glyphPath(NAV_GLYPHS.subs),
  connections: glyphPath(NAV_GLYPHS.connections),
  mcp: glyphPath(NAV_GLYPHS.mcp),
  settings: glyphPath(NAV_GLYPHS.settings),
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
      viewBox="0 0 16 16"
      width={16}
      height={16}
      fill="currentColor"
      shapeRendering="crispEdges"
      aria-hidden="true"
      className={cn("nav-icon", className)}
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
