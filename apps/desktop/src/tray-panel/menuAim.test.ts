// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { aimingAt } from "./menuAim";

describe("aimingAt", () => {
  const rightBox = { left: 240, right: 500, top: 80, bottom: 320 };
  const leftBox = { left: 40, right: 300, top: 80, bottom: 320 };

  it("detects diagonal movement toward a submenu on the right", () => {
    expect(aimingAt({ x: 120, y: 140 }, { x: 180, y: 175 }, rightBox, "right")).toBe(true);
  });

  it("detects diagonal movement toward a submenu on the left", () => {
    expect(aimingAt({ x: 420, y: 140 }, { x: 360, y: 185 }, leftBox, "left")).toBe(true);
  });

  it("treats vertical movement across sibling rows as not aiming", () => {
    expect(aimingAt({ x: 420, y: 140 }, { x: 420, y: 180 }, leftBox, "left")).toBe(false);
    expect(aimingAt({ x: 120, y: 140 }, { x: 120, y: 180 }, rightBox, "right")).toBe(false);
  });

  it("returns false when moving away or stationary", () => {
    expect(aimingAt({ x: 360, y: 140 }, { x: 420, y: 160 }, leftBox, "left")).toBe(false);
    expect(aimingAt({ x: 420, y: 140 }, { x: 420, y: 140 }, leftBox, "left")).toBe(false);
  });
});
