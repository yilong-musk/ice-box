// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Geometry of the node grid: a stack of sections (one per strategy group, or a
 * single headerless section for a flat profile), each a header plus a grid of
 * fixed-size cards. Sections are placed absolutely; only the sections near the
 * viewport mount, and inside a tall section only the card rows near the
 * viewport render.
 */

export const SECTION_HEADER_HEIGHT = 48;
export const CARD_HEIGHT = 60;
export const CARD_GAP = 8;
/** Space below every section, collapsed or not. */
export const SECTION_GAP = 12;
export const NODE_OVERSCAN = 160;
/** Narrowest card before the grid drops a column. */
export const CARD_MIN_WIDTH = 176;

const CARD_STRIDE = CARD_HEIGHT + CARD_GAP;

/** Columns that fit `width` with cards no narrower than `minWidth`. */
export function gridColumns(width: number, minWidth: number): number {
  if (!(width > 0)) return 1;
  return Math.max(1, Math.floor((width + CARD_GAP) / (minWidth + CARD_GAP)));
}

/** Height of `count` cards laid out in `columns` columns. */
export function cardsHeight(count: number, columns: number): number {
  const rows = Math.ceil(count / Math.max(1, columns));
  return rows === 0 ? 0 : rows * CARD_STRIDE - CARD_GAP;
}

export type SectionShape = {
  /** Whether the section draws a header row. */
  header: boolean;
  /** Collapsed sections show only their header. */
  collapsed: boolean;
  /** The section's cards; only the count matters for layout. */
  cards: readonly unknown[];
};

/** Distance from a section's top to its first card row: the header, plus a
 * gap under it while the cards show. */
export function sectionCardsOffset(section: SectionShape): number {
  if (!section.header) return 0;
  return SECTION_HEADER_HEIGHT + (section.collapsed ? 0 : CARD_GAP);
}

export type SectionRow<S> = {
  section: S;
  index: number;
  top: number;
  height: number;
};

export function sectionLayout<S extends SectionShape>(
  sections: readonly S[],
  columns: number,
): { rows: SectionRow<S>[]; height: number } {
  let top = 0;
  const rows = sections.map((section, index) => {
    const cards = section.collapsed ? 0 : cardsHeight(section.cards.length, columns);
    const height = sectionCardsOffset(section) + cards + SECTION_GAP;
    const row = { section, index, top, height };
    top += height;
    return row;
  });
  return { rows, height: top };
}

/** Find the visible slice without scanning every section on each scroll event. */
export function visibleSections<R extends { top: number; height: number }>(
  rows: readonly R[],
  top: number,
  height: number,
): R[] {
  let low = 0;
  let high = rows.length;
  const start = top - NODE_OVERSCAN;
  const end = top + height + NODE_OVERSCAN;
  while (low < high) {
    const mid = Math.floor((low + high) / 2);
    if (rows[mid].top + rows[mid].height <= start) low = mid + 1;
    else high = mid;
  }
  const first = low;
  while (low < rows.length && rows[low].top < end) low += 1;
  return rows.slice(first, low);
}

/**
 * Card index range of one section to render, aligned to whole grid rows.
 * `cardsTop` is where the section's first card row starts in the shared
 * scroll viewport; `top`/`height` describe that viewport.
 */
export function cardWindow(
  count: number,
  columns: number,
  cardsTop: number,
  top: number,
  height: number,
): { start: number; end: number; offset: number } {
  const cols = Math.max(1, columns);
  const rowCount = Math.ceil(count / cols);
  const firstRow = Math.max(
    0,
    Math.min(rowCount, Math.floor((top - cardsTop - NODE_OVERSCAN) / CARD_STRIDE)),
  );
  const lastRow = Math.max(
    firstRow,
    Math.min(rowCount, Math.ceil((top + height - cardsTop + NODE_OVERSCAN) / CARD_STRIDE)),
  );
  return {
    start: Math.min(count, firstRow * cols),
    end: Math.min(count, lastRow * cols),
    offset: firstRow * CARD_STRIDE,
  };
}
