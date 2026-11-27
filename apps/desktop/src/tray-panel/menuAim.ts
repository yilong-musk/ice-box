// SPDX-License-Identifier: GPL-3.0-or-later

export interface Point {
  x: number;
  y: number;
}

export interface Box {
  top: number;
  bottom: number;
  left: number;
  right: number;
}

/** Vertical grace added to the near edge of the child submenu so diagonal
 * movement toward its top or bottom corners stays inside the corridor. */
const TOLERANCE = 20;

/**
 * Determine whether pointer movement from `from` to `to` aims toward an open
 * cascading submenu `box` on `side` (classic triangle corridor heuristic used
 * by native macOS menus). Vertical movement between sibling rows returns false
 * so switching rows stays immediate.
 */
export function aimingAt(
  from: Point,
  to: Point,
  box: Box,
  side: "left" | "right",
): boolean {
  if (from.x === to.x && from.y === to.y) return false;
  const edgeX = side === "left" ? box.right : box.left;
  const ahead = (p: Point) => (side === "left" ? p.x - edgeX : edgeX - p.x);
  if (ahead(from) <= 0 || ahead(to) <= 0) return false;
  const slope = (p: Point, y: number) => (y - p.y) / ahead(p);
  const top = box.top - TOLERANCE;
  const bottom = box.bottom + TOLERANCE;
  return (
    slope(to, top) < slope(from, top) && slope(to, bottom) > slope(from, bottom)
  );
}
