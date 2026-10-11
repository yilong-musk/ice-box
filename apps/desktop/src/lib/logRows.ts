// SPDX-License-Identifier: GPL-3.0-or-later

export type LogRow = {
  id: number;
  text: string;
};

/**
 * Align a new log tail with the previous window.
 *
 * The view is the latest N lines, so an update drops from the front and
 * appends at the end. The browser selection points at DOM nodes; rows that
 * are still present must keep their ids or the highlight slides onto later lines.
 */
export function alignLogRows(
  prev: readonly LogRow[],
  next: readonly string[],
  startId: number,
): { rows: LogRow[]; nextId: number } {
  if (next.length === 0) return { rows: [], nextId: startId };
  const overlap = continuingOverlap(prev, next);
  const rows = prev.slice(prev.length - overlap);
  let nextId = startId;
  for (let i = overlap; i < next.length; i++) {
    rows.push({ id: nextId, text: next[i] });
    nextId += 1;
  }
  return { rows, nextId };
}

/** Longest suffix of `prev` that is a prefix of `next` (0 when the tail was replaced). */
function continuingOverlap(prev: readonly LogRow[], next: readonly string[]): number {
  const max = Math.min(prev.length, next.length);
  for (let drop = 0; drop <= prev.length; drop++) {
    const overlap = prev.length - drop;
    if (overlap > max) continue;
    let matches = true;
    for (let i = 0; i < overlap; i++) {
      if (prev[drop + i]?.text !== next[i]) {
        matches = false;
        break;
      }
    }
    if (matches) return overlap;
  }
  return 0;
}
