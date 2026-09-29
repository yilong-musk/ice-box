// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import {
  memberWindow,
  nodeLayout,
  visibleNodeRows,
  MEMBER_ROW_HEIGHT,
  NODE_OVERSCAN,
  NODE_ROW_HEIGHT,
} from "./virtualNodes";

describe("virtual node geometry", () => {
  it("counts expanded members without creating rendered rows", () => {
    const nodes = [{ tag: "group", outbound_type: "selector", group_now: null,
      group_all: Array.from({ length: 500 }, (_, i) => `node-${i}`) }];
    expect(nodeLayout(nodes, new Set()).height).toBe(NODE_ROW_HEIGHT);
    expect(nodeLayout(nodes, new Set(["group"])).height).toBe(NODE_ROW_HEIGHT + 500 * 32);
    const first = memberWindow(500, 0, 0, 600);
    expect(first.end - first.start).toBeLessThan(30);
    const last = memberWindow(500, 0, 15_456, 600);
    expect(last.end).toBe(500);
    expect(last.start).toBeGreaterThan(470);
  });

  it("handles an empty list without producing phantom rows", () => {
    const layout = nodeLayout([], new Set());
    expect(layout.height).toBe(0);
    expect(visibleNodeRows(layout.rows, 0, 600)).toEqual([]);
    expect(memberWindow(0, 0, 0, 600)).toEqual({ start: 0, end: 0 });
  });

  it("bounds the visible window across a ten-thousand-node list", () => {
    const nodes = Array.from({ length: 10_000 }, (_, i) => ({
      tag: `node-${i}`,
      outbound_type: "socks",
      group_now: null,
      group_all: null,
    }));
    const layout = nodeLayout(nodes, new Set());
    const height = 600;
    const maxRows = Math.ceil((height + 2 * NODE_OVERSCAN) / NODE_ROW_HEIGHT) + 1;

    for (const top of [0, 123_456, layout.height - height]) {
      const visible = visibleNodeRows(layout.rows, top, height);
      expect(visible).toEqual(layout.rows.filter(row =>
        row.top + row.height > top - NODE_OVERSCAN &&
        row.top < top + height + NODE_OVERSCAN,
      ));
      expect(visible.length).toBeGreaterThan(0);
      expect(visible.length).toBeLessThanOrEqual(maxRows);
    }
    const lastWindow = visibleNodeRows(layout.rows, layout.height - height, height);
    expect(lastWindow[lastWindow.length - 1].node.tag).toBe("node-9999");
  });

  it("keeps a tall expanded group visible and repositions rows on collapse", () => {
    const nodes = [
      { tag: "group", outbound_type: "selector", group_now: null,
        group_all: Array.from({ length: 500 }, (_, i) => `member-${i}`) },
      { tag: "after", outbound_type: "socks", group_now: null, group_all: null },
    ];
    const expanded = nodeLayout(nodes, new Set(["group"]));
    expect(expanded.rows[1].top).toBe(NODE_ROW_HEIGHT + 500 * MEMBER_ROW_HEIGHT);
    expect(visibleNodeRows(expanded.rows, 10_000, 600).map(row => row.node.tag))
      .toEqual(["group"]);

    const collapsed = nodeLayout(nodes, new Set());
    expect(collapsed.rows[1].top).toBe(NODE_ROW_HEIGHT);
    expect(collapsed.height).toBe(2 * NODE_ROW_HEIGHT);
    expect(visibleNodeRows(collapsed.rows, 0, 600).map(row => row.node.tag))
      .toEqual(["group", "after"]);
  });

  it("clamps member windows before and after a group", () => {
    expect(memberWindow(500, 20_000, 0, 600)).toEqual({ start: 0, end: 0 });
    expect(memberWindow(500, 0, 20_000, 600)).toEqual({ start: 500, end: 500 });
    const groupTop = 20_000;
    const shifted = memberWindow(500, groupTop, groupTop + 10_000, 600);
    expect(shifted).toEqual(memberWindow(500, 0, 10_000, 600));
  });
});
