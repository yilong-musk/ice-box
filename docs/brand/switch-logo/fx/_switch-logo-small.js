// SPDX-License-Identifier: GPL-3.0-or-later
//
// Shared drawing for the small ice-box switch logos. The 48 px effect keeps the
// full detail; these variants redraw the same mark on a 24 or 32 px grid, with
// fewer, larger ports and two-pixel lights so the state still reads.

import { fit, hex, mix, rand, TAU, WHITE } from "../../core/index.js";

const DURATION = 3;
// The error red is deep enough that the additive glow does not wash it out pink.
const GREEN = hex("#5dff9a"), GREEN_HI = hex("#0f7a3d"), AMBER = hex("#ffc94d"), RED = hex("#ff1a1a");

const GEOMETRY = {
  24: { X0: 1, X1: 17, Y0: 11, Y1: 19, D: 4, cols: 4, rows: 2, pw: 3, ph: 2, rgap: 2, portTop: 1, light: 2 },
  32: { X0: 2, X1: 23, Y0: 14, Y1: 25, D: 6, cols: 5, rows: 2, pw: 3, ph: 3, rgap: 2, portTop: 2, light: 2.5 },
};

export function makeMeta(size) {
  return {
    name: `ice-box switch logo ${size}px`,
    desc: "The small ice-box switch logo: a network switch cut from ice crystal, its port lights showing the core state.",
    tags: ["logo", "switch", "network", "ice", "crystal", "loop"],
    size: [size, size],
    origin: [0, 0],
    duration: DURATION,
    loop: true,
    fps: 30,
    bg: "dark",
    options: {
      palette: ["#151a3d", "#212f6b", "#2c58aa", "#3a8bd8", "#62c8f4", "#a6ecff", "#dcfbff", "#ffffff"],
      state: "",
      still: -1,
      seed: 21,
    },
  };
}


export function drawLogo(p, t, o, size) {
  const G = GEOMETRY[size];
  const { X0, X1, Y0, Y1, D } = G;
  const C = fit(o.palette, 8), ph = o.still >= 0 ? o.still : t / DURATION;
  const put = (x, y, c) => p.px(x, y, c);

  // Cold light behind it, scaled with the mark; stills stay flat.
  if (o.still < 0) p.glow((X0 + X1) / 2, (Y0 + Y1) / 2, size * 0.62, C[3], 0.18);

  const band = (v) => {
    const q = Math.max(1, Math.min(7, Math.round(v * 2) / 2)), lo = Math.floor(q);
    return q === lo ? C[lo] : mix(C[lo], C[lo + 1], 0.5);
  };
  // Lid.
  for (let r = 0; r < D; r++) {
    const y = Y0 - 1 - r;
    for (let x = X0 + 1 + r; x <= X1 + 1 + r; x++) {
      const u = (x - X0 - 1 - r) / (X1 - X0), w = r / (D - 1);
      put(x, y, band(6.1 - (u * 0.8 + w * 0.4)));
    }
  }
  // Right side.
  for (let c = 0; c < D; c++) {
    const x = X1 + 1 + c;
    for (let y = Y0 - c; y <= Y1 - c; y++) put(x, y, band(3.0 - ((c / D) * 0.6 + ((y - Y0 + c) / (Y1 - Y0)) * 0.6)));
  }
  // Front: one even pane of ice with a glassy sheen. The ports are cut after
  // this, so the sheen only shows on the panel between them.
  const sheen = o.still >= 0 ? 0 : Math.min(1, Math.max(0, (ph - 0.15) / 0.35));
  const sweep = -18 + 50 * sheen;
  for (let y = Y0; y <= Y1; y++)
    for (let x = X0; x <= X1; x++) {
      const d = (x - X0) + (y - Y0) * 0.8;
      let c = mix(C[3], C[4], 0.5);
      if (Math.abs(d - sweep) < 0.6) c = C[6];
      put(x, y, c);
    }

  // Light through the ice, up the near edge of the side.
  for (let y = Y0 + 2; y < Y1 - 1; y++) put(X1 + 1, y, mix(p.get(X1 + 1, y), C[4], 0.5));

  // Ridges.
  for (let x = X0; x <= X1; x++) put(x, Y0 - 1, WHITE);
  for (let r = 0; r < D; r++) {
    put(X0 + 1 + r, Y0 - 1 - r, r < 2 ? WHITE : C[7]);
    put(X1 + 1 + r, Y0 - 1 - r, C[6]);
    put(X1 + 1 + r, Y1 - r, C[2]);
  }
  for (let x = X0 + 1 + D; x <= X1 + D; x++) put(x, Y0 - D, C[6]);
  for (let y = Y0; y <= Y1; y++) put(X0, y, C[5]), put(X1, y, C[4]);
  for (let x = X0; x <= X1; x++) put(x, Y1, C[2]);

  // Dark rim round the silhouette.
  const rim = [];
  for (let y = 0; y < size; y++)
    for (let x = 0; x < size; x++) {
      if (p.get(x, y)) continue;
      if (p.get(x + 1, y) || p.get(x - 1, y) || p.get(x, y + 1) || p.get(x, y - 1)) rim.push([x, y]);
    }
  for (const [x, y] of rim) put(x, y, C[1]);

  // Ports: fewer columns, wider pixels, two-pixel lights on the top edge.
  const { cols, rows, pw, ph: portH, rgap } = G;
  const areaW = cols * pw + (cols - 1);
  const px0 = X0 + Math.floor((X1 - X0 + 1 - areaW) / 2);
  const py0 = Y0 + G.portTop;
  // Two adjacent pixels per lamp, like the 48 px set. The link lamp starts at
  // the left edge and the activity lamp one pixel in, so the port's top edge
  // has no dark gap when both are lit.
  const lamp = (x, y, c, g = 0.16, span = 2) => {
    for (let k = 0; k < span; k++) put(x + k, y, c);
    if (o.still < 0) p.glow(x + (span - 1) / 2 + 0.5, y + 0.5, G.light, c, g);
  };
  for (let r = 0; r < rows; r++)
    for (let c = 0; c < cols; c++) {
      const X = px0 + c * (pw + 1), Y = py0 + r * (portH + rgap);
      for (let y = 0; y < portH; y++)
        for (let x = 0; x < pw; x++) put(X + x, Y + y, y === 0 || x === 0 ? C[0] : C[1]);
      // The lower lip is 3 px wide; its middle is the dark notch.
      put(X + 1, Y + portH, C[0]);
      const i = r * cols + c;
      const n = 3 + Math.floor(rand(i, 2, o.seed) * 6), off = rand(i, 3, o.seed);
      const traffic = o.still >= 0 ? rand(i, 4, o.seed) > 0.45 : Math.sin((ph * n + off) * TAU) > 0.2;

      if (o.state === "stopped") continue;
      if (o.state === "starting") {
        const head = ((ph * 2) % 1) * (cols + 2) - 1, d = head - c;
        if (d >= 0 && d < 1.5) lamp(X, Y, AMBER);
        else if (d >= 1.5 && d < 2.5) put(X, Y, mix(AMBER, C[1], 0.5));
        continue;
      }
      if (o.state === "stopping") {
        const left = cols - Math.floor((ph % 1) * (cols + 2));
        if (c < left) lamp(X, Y, AMBER, 0.14);
        continue;
      }
      if (o.state === "error") {
        // Every port flashes red together. No glow: the front panel keeps its
        // even shading instead of picking up a patchwork of red light.
        if (o.still >= 0 || Math.sin(ph * 4 * TAU) > 0) lamp(X, Y, RED, 0);
        continue;
      }
      if (o.state === "running") {
        lamp(X, Y, GREEN, 0.14);
        if (traffic) lamp(X + pw - 2, Y, GREEN_HI, 0.12);
        continue;
      }
      lamp(X, Y, C[6], 0.14);
      if (traffic) lamp(X + pw - 2, Y, WHITE, 0.16);
    }

  // One glint on the near lid corner, on the same beat as the 48 px mark.
  const beat = (at, len) => {
    const d = (ph - at + 1) % 1;
    return d < len ? Math.sin((d / len) * Math.PI) : 0;
  };
  const g = o.still >= 0 ? 0 : beat(0.1, 0.14);
  if (g > 0.2) p.dot(X0 + 1, Y0 - 1, C[6], g);
}
