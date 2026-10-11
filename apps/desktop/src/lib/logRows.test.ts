// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { alignLogRows, type LogRow } from "./logRows";

function rows(texts: string[], start = 1): LogRow[] {
  return texts.map((text, i) => ({ id: start + i, text }));
}

describe("alignLogRows", () => {
  it("keeps existing rows when lines are only appended", () => {
    const prev = rows(["a", "b"]);
    const aligned = alignLogRows(prev, ["a", "b", "c"], 3);
    expect(aligned.rows[0]).toBe(prev[0]);
    expect(aligned.rows[1]).toBe(prev[1]);
    expect(aligned.rows[2]).toEqual({ id: 3, text: "c" });
    expect(aligned.nextId).toBe(4);
  });

  it("keeps the overlap when the window drops lines from the front", () => {
    const prev = rows(["a", "b", "c"]);
    const aligned = alignLogRows(prev, ["b", "c", "d"], 4);
    expect(aligned.rows[0]).toBe(prev[1]);
    expect(aligned.rows[1]).toBe(prev[2]);
    expect(aligned.rows[2]).toEqual({ id: 4, text: "d" });
    expect(aligned.nextId).toBe(5);
  });

  it("aligns repeated lines to the longest continuing prefix", () => {
    const prev = rows(["a", "a", "b"]);
    const aligned = alignLogRows(prev, ["a", "b", "c"], 4);
    expect(aligned.rows[0]).toBe(prev[1]);
    expect(aligned.rows[1]).toBe(prev[2]);
    expect(aligned.rows[2]).toEqual({ id: 4, text: "c" });
  });

  it("assigns fresh ids when the tail does not continue the previous window", () => {
    const prev = rows(["a", "b"]);
    const aligned = alignLogRows(prev, ["x", "y"], 3);
    expect(aligned.rows).toEqual([
      { id: 3, text: "x" },
      { id: 4, text: "y" },
    ]);
    expect(aligned.rows[0]).not.toBe(prev[0]);
    expect(aligned.nextId).toBe(5);
  });

  it("clears the window when the tail is empty", () => {
    const aligned = alignLogRows(rows(["a"]), [], 8);
    expect(aligned.rows).toEqual([]);
    expect(aligned.nextId).toBe(8);
  });
});
