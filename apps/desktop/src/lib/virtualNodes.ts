// SPDX-License-Identifier: GPL-3.0-or-later

import type { NodeInfo } from "../api/tauri";
import { isGroupType } from "./nodes";

export const NODE_ROW_HEIGHT = 56;
export const MEMBER_ROW_HEIGHT = 32;
export const NODE_OVERSCAN = 160;

export function nodeLayout(nodes: NodeInfo[], expanded: ReadonlySet<string>) {
  let top = 0;
  const rows = nodes.map((node, index) => {
    const members = isGroupType(node.outbound_type) && expanded.has(node.tag)
      ? node.group_all?.length ?? 0 : 0;
    const height = NODE_ROW_HEIGHT + members * MEMBER_ROW_HEIGHT;
    const row = { node, index, top, height, members };
    top += height;
    return row;
  });
  return { rows, height: top };
}

/** Find the visible slice without scanning every node on each scroll event. */
export function visibleNodeRows(rows: ReturnType<typeof nodeLayout>["rows"], top: number, height: number) {
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

/** Bounds are in the shared outer viewport, including expanded group rows. */
export function memberWindow(count: number, groupTop: number, top: number, height: number) {
  const origin = groupTop + NODE_ROW_HEIGHT;
  const start = Math.max(0, Math.min(count, Math.floor((top - origin - NODE_OVERSCAN) / MEMBER_ROW_HEIGHT)));
  const end = Math.max(start, Math.min(count, Math.ceil((top + height - origin + NODE_OVERSCAN) / MEMBER_ROW_HEIGHT)));
  return { start, end };
}
