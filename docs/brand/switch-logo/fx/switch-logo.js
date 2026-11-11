// SPDX-License-Identifier: GPL-3.0-or-later
//
// ice-box logo: a pixel-art network switch cut from ice crystal. Its port lights show the core state.
//
// A pixelfx effect (https://github.com/patch-gg/pixelfx). Copy this file into a pixelfx checkout as
// src/fx/brand/switch-logo.js and render it with ../tools/export.mjs; see ../README.md.
//
// The palette, the frost motes and the glints follow pixelfx's ice/ice-crystal, which adapts Sprite
// Fusion's open-source pickup gem (https://www.spritefusion.com/pixel-art-vfx/pickup). Those parts are
// MIT licensed: Copyright (c) 2026 Patch, Copyright (c) 2026 Hugo Duprez. See ../LICENSE-pixelfx.txt.
import { fit, cycle, sparkle, hex, mix, rand, TAU, WHITE } from "../../core/index.js";

export const meta = {
  name: "ice-box switch logo",
  desc: "A network switch cut from ice crystal: lit faces, pale ridges, glints, cold light, frost motes, and port lights that show the core state.",
  tags: ["logo", "switch", "network", "ice", "crystal", "loop"],
  size: [48, 48],
  origin: [0, 0],
  duration: 3,
  loop: true,
  fps: 30,
  bg: "dark",
  options: {
    // pixelfx ice/ice-crystal ramp, dark to bright
    palette: ["#151a3d", "#212f6b", "#2c58aa", "#3a8bd8", "#62c8f4", "#a6ecff", "#dcfbff", "#ffffff"],
    // core state shown by the port lights: stopped | starting | running | stopping | error.
    // Empty draws the plain logo, with ice-blue lights.
    state: "",
    motes: 1,
    still: -1, // >= 0: freeze at this phase (0..1) for a static image
    seed: 21,
  },
};

// Geometry: the front face spans X0..X1, Y0..Y1; the lid recedes up and to the right D pixels at 45 degrees.
const X0 = 3, X1 = 35, Y0 = 22, Y1 = 37, D = 9;

// Port lights for the core states.
const GREEN = hex("#5dff9a"), GREEN_HI = hex("#c4ffd9"), AMBER = hex("#ffc94d"), RED = hex("#ff5a5a");

function motes(p, C, t, o, front) {
  if (!o.motes || o.still >= 0) return;
  cycle(t, meta.duration, 12, o.seed, (g) => g.range(1.6, 2.6), (age, life, r, i) => {
    const th = r.range(0, TAU) + age * r.range(1.4, 2.2), R = r.range(17, 22), y = 26 + r.range(-6, 8) - age * r.range(2, 5), s = Math.sin(th), k = age / life;
    if (s >= 0 !== front) return;
    const x = 24 + Math.cos(th) * R, yy = y + s * R * 0.3, a = Math.min(1, age / 0.2) * (k > 0.75 ? (1 - k) / 0.25 : 1);
    // In front, only where a mote doesn't cross the switch, so the ports stay readable.
    if (front && x > X0 - 2 && x < X1 + D + 2 && yy > Y0 - D - 2 && yy < Y1 + 2) return;
    if (front) {
      p.dot(x, yy, i % 4 === 0 ? WHITE : C[6], a);
      if (i % 4 === 0) for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) p.dot(x + dx, yy + dy, C[5], a);
      p.glow(x, yy, 4, C[5], 0.3 * a);
    } else p.dot(x, yy, C[4], a * 0.9);
  });
}

export function drawBack(p, t, o) {
  motes(p, fit(o.palette, 8), t, o, false);
}

export function draw(p, t, o) {
  const C = fit(o.palette, 8), ph = o.still >= 0 ? o.still : t / meta.duration;
  const put = (x, y, c) => p.px(x, y, c);

  // Cold light behind it.
  p.glow(24, 27, 30, C[3], 0.2);
  p.glow(12, 16, 12, C[5], 0.06 + 0.02 * Math.sin(TAU * ph));

  // One light, from the upper left and a little in front. Each face shades smoothly across itself on the
  // ramp, brightest at the corner nearest the light. Half steps between ramp colors give a gentler fall-off;
  // there is no dithering, so the steps follow the light direction.
  const band = (v) => {
    const q = Math.max(1, Math.min(7, Math.round(v * 2) / 2)), lo = Math.floor(q);
    return q === lo ? C[lo] : mix(C[lo], C[lo + 1], 0.5);
  };
  // Lid: catches the most light; brightest at its front-left corner, falling off toward the back right.
  for (let r = 0; r < D; r++) {
    const y = Y0 - 1 - r;
    for (let x = X0 + 1 + r; x <= X1 + 1 + r; x++) {
      const u = (x - X0 - 1 - r) / (X1 - X0), w = r / (D - 1);
      put(x, y, band(6.1 - (u * 0.8 + w * 0.4)));
    }
  }
  // Right side: turned away from the light; darker toward the back and the bottom.
  for (let c = 0; c < D; c++) {
    const x = X1 + 1 + c;
    for (let y = Y0 - c; y <= Y1 - c; y++) put(x, y, band(3.0 - ((c / D) * 0.6 + ((y - Y0 + c) / (Y1 - Y0)) * 0.6)));
  }
  // Front: lit at a grazing angle from above; brighter at the top and on the left.
  for (let y = Y0; y <= Y1; y++)
    for (let x = X0; x <= X1; x++) put(x, y, band(4.3 - (((y - Y0) / (Y1 - Y0)) * 1.2 + ((x - X0) / (X1 - X0)) * 0.5)));

  // Light passing through the ice: a pale line just above the bottom of the front and up the near edge of the side.
  for (let x = X0 + 1; x < X1; x++) put(x, Y1 - 1, mix(p.get(x, Y1 - 1), C[5], 0.45));
  for (let y = Y0 + 2; y < Y1 - 1; y++) put(X1 + 1, y, mix(p.get(X1 + 1, y), C[4], 0.5));

  // Ridges: white where the lid meets the front, pale along lit edges, deep along shaded ones.
  for (let x = X0; x <= X1; x++) put(x, Y0 - 1, WHITE);
  for (let r = 0; r < D; r++) {
    put(X0 + 1 + r, Y0 - 1 - r, r < 3 ? WHITE : C[7]);
    put(X1 + 1 + r, Y0 - 1 - r, C[6]);
    put(X1 + 1 + r, Y1 - r, C[2]);
  }
  for (let x = X0 + 1 + D; x <= X1 + D; x++) put(x, Y0 - D, C[6]);
  for (let y = Y0; y <= Y1; y++) put(X0, y, C[5]), put(X1, y, C[4]);
  for (let x = X0; x <= X1; x++) put(x, Y1, C[2]);

  // Dark rim round the silhouette.
  const rim = [];
  for (let y = 0; y < 48; y++)
    for (let x = 0; x < 48; x++) {
      if (p.get(x, y)) continue;
      if (p.get(x + 1, y) || p.get(x - 1, y) || p.get(x, y + 1) || p.get(x, y - 1)) rim.push([x, y]);
    }
  for (const [x, y] of rim) put(x, y, C[1]);

  // Ports cut into the front: two rows of six, deep blue inside, a pale lip below each.
  const cols = 6, rows = 2, pw = 4, phh = 4, gap = 1, rgap = 3;
  const px0 = X1 - 2 - cols * (pw + gap) + gap, py0 = Y0 + 3;
  const st = o.state;
  // One light: a pixel with a little glow. With a state, each light is two pixels wide (the port's top edge
  // holds two lights), so the state reads at a glance.
  const wide = !!st;
  const lamp = (x, y, c, g = 0.18) => {
    const x2 = wide ? ((x - px0) % (pw + gap) === 0 ? x + 1 : x - 1) : x;
    put(x, y, c);
    if (wide) put(x2, y, c);
    p.glow((x + x2) / 2 + 0.5, y + 0.5, wide ? 3.5 : 2.5, c, wide ? g * 1.6 : g);
  };
  for (let r = 0; r < rows; r++)
    for (let c = 0; c < cols; c++) {
      const X = px0 + c * (pw + gap), Y = py0 + r * (phh + rgap);
      for (let y = 0; y < phh; y++) for (let x = 0; x < pw; x++) put(X + x, Y + y, y === 0 || x === 0 ? C[0] : C[1]);
      put(X + 1, Y + phh, C[0]), put(X + 2, Y + phh, C[0]);
      put(X + 3, Y + phh, C[5]);
      // Some ports have nothing plugged in; each plugged port has its own traffic rhythm. Whole-loop
      // periods keep the loop seamless.
      const i = r * cols + c, plugged = rand(i, 1, o.seed) >= 0.18;
      const n = 3 + Math.floor(rand(i, 2, o.seed) * 6), off = rand(i, 3, o.seed);
      const traffic = o.still >= 0 ? rand(i, 4, o.seed) > 0.45 : Math.sin((ph * n + off) * TAU) > 0.2;

      if (st === "stopped") continue; // all dark
      if (st === "starting") {
        // A short amber chase across the columns, twice a loop: the ports wake one after another.
        const head = ((ph * 2) % 1) * (cols + 2) - 1, d = head - c;
        if (d >= 0 && d < 1.5) lamp(X, Y, AMBER), lamp(X + pw - 1, Y, AMBER);
        else if (d >= 1.5 && d < 2.5) put(X, Y, mix(AMBER, C[1], 0.5)), put(X + 1, Y, mix(AMBER, C[1], 0.5));
        continue;
      }
      if (st === "stopping") {
        // Amber, going out column by column from the right, then all dark for a beat.
        const left = cols - Math.floor((ph % 1) * (cols + 2));
        if (plugged && c < left) lamp(X, Y, AMBER, 0.16);
        continue;
      }
      if (st === "error") {
        // Every port flashes red together.
        if (o.still >= 0 || Math.sin(ph * 4 * TAU) > 0) lamp(X, Y, RED, 0.22), lamp(X + pw - 1, Y, RED, 0.22);
        continue;
      }
      if (!plugged) continue;
      if (st === "running") {
        // Green link lights hold steady; activity lights blink with traffic.
        lamp(X, Y, GREEN, 0.16);
        if (traffic) lamp(X + pw - 1, Y, GREEN_HI, 0.18);
        continue;
      }
      // Plain logo: ice-blue link and white activity lights.
      lamp(X, Y, C[6], 0.16);
      if (traffic) lamp(X + pw - 1, Y, WHITE, 0.18);
    }

  // Power light, then a vent column on the left of the front.
  const pwr = { stopped: C[2], starting: AMBER, stopping: AMBER, running: GREEN, error: RED }[st];
  const lit = st === "starting" ? Math.sin(ph * 4 * TAU) > -0.3 : st === "error" ? Math.sin(ph * 4 * TAU) > 0 || o.still >= 0 : true;
  if (!pwr) lamp(X0 + 2, Y0 + 3, WHITE, 0.25);
  else if (st === "stopped") put(X0 + 2, Y0 + 3, C[2]);
  else if (!lit) put(X0 + 2, Y0 + 3, mix(pwr, C[1], 0.6));
  else lamp(X0 + 2, Y0 + 3, pwr, 0.25);
  for (let v = 0; v < 4; v++) put(X0 + 2, Y0 + 7 + v * 2, C[2]);

  // Glints on the near and far corners of the lid, each on its own beat.
  const beat = (at, len) => { const d = (ph - at + 1) % 1; return d < len ? Math.sin((d / len) * Math.PI) : 0; };
  if (o.still >= 0) sparkle(p, X0 + 1, Y0 - 1, 3, C[6], C[4], 0.5);
  else
    for (const [x, y, at] of [[X0 + 2, Y0 - 1, 0.1], [X1 + D, Y0 - D, 0.6]]) {
      const g = beat(at, 0.14);
      if (g > 0.05) sparkle(p, x, y, Math.round(1 + g * 4), C[6], C[4], 0.5);
    }
  motes(p, C, t, o, true);
}
