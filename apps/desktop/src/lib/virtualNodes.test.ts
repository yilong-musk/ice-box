// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import {
  CARD_GAP,
  CARD_HEIGHT,
  NODE_OVERSCAN,
  SECTION_GAP,
  SECTION_HEADER_HEIGHT,
  cardWindow,
  cardsHeight,
  gridColumns,
  sectionCardsOffset,
  sectionLayout,
  visibleSections,
} from "./virtualNodes";

const STRIDE = CARD_HEIGHT + CARD_GAP;

function section(cardCount: number, collapsed = false, header = true) {
  return { header, collapsed, cardCount };
}

describe("grid geometry", () => {
  it("fits as many columns as the minimum card width allows", () => {
    expect(gridColumns(0, 176)).toBe(1);
    expect(gridColumns(100, 176)).toBe(1);
    expect(gridColumns(176 * 2 + CARD_GAP, 176)).toBe(2);
    expect(gridColumns(176 * 2 + CARD_GAP - 1, 176)).toBe(1);
    expect(gridColumns(1200, 176)).toBe(6);
  });

  it("sizes card rows without a trailing gap", () => {
    expect(cardsHeight(0, 3)).toBe(0);
    expect(cardsHeight(1, 3)).toBe(CARD_HEIGHT);
    expect(cardsHeight(4, 3)).toBe(2 * STRIDE - CARD_GAP);
  });

  it("stacks headers, cards and section gaps", () => {
    const layout = sectionLayout(
      [section(5), section(500, true), section(3, false, false)],
      2,
    );
    const open = SECTION_HEADER_HEIGHT + CARD_GAP + cardsHeight(5, 2) + SECTION_GAP;
    const collapsed = SECTION_HEADER_HEIGHT + SECTION_GAP;
    const headerless = cardsHeight(3, 2) + SECTION_GAP;
    expect(layout.rows.map((row) => [row.top, row.height])).toEqual([
      [0, open],
      [open, collapsed],
      [open + collapsed, headerless],
    ]);
    expect(layout.height).toBe(open + collapsed + headerless);
    expect(sectionCardsOffset(section(5))).toBe(SECTION_HEADER_HEIGHT + CARD_GAP);
    expect(sectionCardsOffset(section(5, true))).toBe(SECTION_HEADER_HEIGHT);
    expect(sectionCardsOffset(section(5, false, false))).toBe(0);
  });

  it("handles an empty list without producing phantom rows", () => {
    const layout = sectionLayout([], 3);
    expect(layout.height).toBe(0);
    expect(visibleSections(layout.rows, 0, 600)).toEqual([]);
    expect(cardWindow(0, 3, 0, 0, 600)).toEqual({ start: 0, end: 0, offset: 0 });
  });

  it("renders only the card rows near the viewport of a huge section", () => {
    const columns = 4;
    const count = 10_000;
    const height = 600;
    const maxCards =
      (Math.ceil((height + 2 * NODE_OVERSCAN) / STRIDE) + 1) * columns;

    const first = cardWindow(count, columns, 0, 0, height);
    expect(first.start).toBe(0);
    expect(first.offset).toBe(0);
    expect(first.end - first.start).toBeLessThanOrEqual(maxCards);

    const total = cardsHeight(count, columns);
    const last = cardWindow(count, columns, 0, total - height, height);
    expect(last.end).toBe(count);
    expect(last.start % columns).toBe(0);
    expect(last.offset).toBe((last.start / columns) * STRIDE);
    expect(last.end - last.start).toBeLessThanOrEqual(maxCards);
  });

  it("finds the sections that overlap the viewport", () => {
    const layout = sectionLayout(
      Array.from({ length: 200 }, (_, index) => section(index % 7, index % 3 === 0)),
      3,
    );
    const height = 600;
    for (const top of [0, 4_000, layout.height - height]) {
      const visible = visibleSections(layout.rows, top, height);
      expect(visible).toEqual(
        layout.rows.filter(
          (row) =>
            row.top + row.height > top - NODE_OVERSCAN &&
            row.top < top + height + NODE_OVERSCAN,
        ),
      );
      expect(visible.length).toBeGreaterThan(0);
    }
  });
});
