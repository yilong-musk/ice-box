// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Dot-matrix globe for Home's landing probe card, drawn on a 2D canvas and
 * zoomed in so the sphere is larger than the card: only the part of the
 * globe around the view centre shows, filling the whole display area, with
 * the curvature still visible.
 *
 * The land mask is a set of hand-simplified continent outlines sampled onto
 * an equal-area dot grid once at module load; it is a schematic, not a
 * geographic reference. Dots are projected orthographically. When the probe
 * lands, the globe turns so the exit country (from a built-in table of
 * approximate centroids) sits in the middle of the card, the dots around it
 * take the accent colour, and an HTML marker pings there. A country missing
 * from the table leaves the globe without a marker; the card's text row still
 * shows the IP.
 *
 * Expanding: the very same canvas leaves the card and becomes the map in the
 * full-size dialog (LandingMapDialog). On `expanded` the canvas is moved into
 * a fixed "stage" on top of everything, the stage's box flies from the card
 * to the dialog's map slot (`[data-landing-slot]`), and every land dot slides
 * from its place on the globe to its place on a flat equirectangular map
 * centred on the meridian the globe was facing, so the globe unrolls in
 * place. On landing the canvas is put into the slot. Collapsing runs the same
 * thing backwards into the card. One element, one set of dots, the whole way.
 *
 * The canvas is created here, not by React, so it can change parents without
 * being remounted (and without React losing track of its own nodes).
 *
 * Frames are drawn only while something moves: once after a change, during
 * the 1.2 s turn to a new country, during an expand / collapse, and as a slow
 * drift while the probe runs. The marker's ping is a CSS animation, so a
 * settled globe costs no frames.
 */

import { useEffect, useRef, type RefObject } from "react";
import { cn } from "@/lib/utils";

type Ring = readonly (readonly [lon: number, lat: number])[];

/** Rough coastlines, [lon, lat]. Inland seas and small islands are ignored. */
export const LAND: readonly Ring[] = [
  // North and Central America
  [
    [-168, 66], [-156, 71], [-130, 70], [-110, 73], [-90, 72], [-80, 68],
    [-64, 60], [-56, 52], [-66, 45], [-70, 42], [-76, 36], [-80, 32],
    [-81, 26], [-84, 30], [-90, 29], [-97, 26], [-97, 21], [-92, 18],
    [-87, 21], [-88, 15], [-83, 9], [-78, 8], [-80, 7], [-86, 11],
    [-92, 14], [-100, 17], [-105, 20], [-110, 24], [-113, 31], [-117, 32],
    [-122, 37], [-124, 42], [-124, 48], [-130, 54], [-140, 59], [-150, 60],
    [-158, 57], [-165, 60],
  ],
  // Greenland
  [[-52, 60], [-44, 60], [-22, 70], [-18, 77], [-30, 83], [-60, 82], [-70, 77], [-56, 70]],
  // Iceland
  [[-24, 63], [-13, 64], [-14, 67], [-23, 67]],
  // South America
  [
    [-78, 8], [-72, 12], [-62, 11], [-52, 5], [-50, 0], [-35, -6],
    [-39, -14], [-41, -22], [-48, -26], [-53, -34], [-58, -38], [-65, -42],
    [-66, -48], [-69, -52], [-72, -54], [-75, -48], [-73, -38], [-71, -30],
    [-70, -18], [-76, -14], [-81, -6], [-80, 0], [-77, 4],
  ],
  // Eurasia
  [
    [-10, 36], [-9, 43], [-2, 44], [-4, 48], [2, 51], [8, 54], [8, 57],
    [5, 62], [12, 66], [18, 70], [28, 71], [40, 68], [55, 68], [70, 73],
    [80, 73], [100, 77], [115, 74], [130, 72], [150, 71], [170, 70],
    [180, 66], [180, 62], [163, 60], [160, 55], [156, 51], [142, 53],
    [140, 47], [132, 43], [129, 35], [126, 37], [121, 40], [122, 31],
    [120, 24], [110, 20], [108, 16], [109, 12], [105, 9], [100, 13],
    [100, 7], [103, 1], [98, 8], [98, 16], [94, 17], [92, 22], [88, 22],
    [80, 15], [77, 8], [73, 17], [70, 22], [66, 25], [57, 25], [56, 27],
    [50, 30], [48, 29], [51, 24], [56, 26], [59, 22], [52, 16], [43, 13],
    [39, 21], [35, 28], [34, 31], [36, 36], [30, 36], [27, 37], [26, 40],
    [23, 38], [22, 37], [20, 40], [19, 42], [13, 45], [14, 42], [18, 40],
    [16, 38], [12, 42], [9, 44], [3, 43], [-1, 37], [-6, 36],
  ],
  // Great Britain
  [[-6, 50], [2, 51], [0, 54], [-2, 58], [-5, 58], [-5, 54]],
  // Africa
  [
    [-17, 21], [-16, 28], [-10, 30], [-6, 36], [3, 37], [10, 37], [11, 33],
    [20, 31], [25, 32], [32, 31], [34, 28], [38, 20], [43, 12], [51, 12],
    [44, 4], [40, -3], [40, -11], [35, -20], [33, -26], [27, -34],
    [19, -35], [16, -28], [12, -17], [13, -7], [9, -1], [9, 4], [3, 6],
    [-8, 4], [-13, 8], [-17, 14],
  ],
  // Madagascar
  [[44, -25], [49, -13], [51, -16], [47, -26]],
  // Japan
  [[130, 31], [136, 34], [141, 38], [142, 44], [146, 44], [141, 41], [140, 36], [135, 33]],
  // Taiwan
  [[120, 22], [122, 25], [121.5, 25.3], [120, 23.5]],
  // Philippines
  [[120, 18], [123, 18], [127, 7], [124, 6], [120, 14]],
  // Sumatra, Borneo, Java, New Guinea
  [[95, 6], [106, -6], [104, -6], [96, 1]],
  [[109, 1], [117, 7], [119, 1], [116, -4], [110, -3]],
  [[105, -6], [115, -8], [114, -9], [106, -8]],
  [[131, -1], [141, -3], [150, -10], [142, -9], [135, -4]],
  // Australia
  [
    [114, -22], [122, -18], [130, -12], [137, -12], [142, -11], [146, -19],
    [153, -26], [150, -37], [141, -38], [135, -35], [131, -32], [118, -35],
    [115, -34],
  ],
  // New Zealand
  [[172, -34], [179, -38], [174, -42], [171, -46], [166, -46], [171, -41]],
];

/** Approximate centroids, [lat, lon], keyed by ISO 3166-1 alpha-2. */
const CENTROIDS: Readonly<Record<string, readonly [number, number]>> = {
  US: [39, -98], CA: [56, -106], MX: [23, -102], BR: [-10, -52],
  AR: [-34, -64], CL: [-33, -71], CO: [4, -73], PE: [-9, -75],
  GB: [54, -2], IE: [53, -8], IS: [65, -18], FR: [46, 2], DE: [51, 10],
  NL: [52, 5], BE: [50.5, 4.5], LU: [49.8, 6.1], CH: [47, 8],
  AT: [47.5, 14.5], IT: [42.8, 12.5], ES: [40, -4], PT: [39.5, -8],
  SE: [62, 15], NO: [62, 10], FI: [64, 26], DK: [56, 10], PL: [52, 19],
  CZ: [49.8, 15.5], SK: [48.7, 19.7], HU: [47, 19.5], SI: [46.1, 14.9],
  HR: [45.1, 15.2], RS: [44, 21], RO: [46, 25], BG: [42.7, 25.5],
  GR: [39, 22], MD: [47, 28.5], UA: [49, 32], BY: [53, 28], EE: [59, 25],
  LV: [57, 25], LT: [55, 24], RU: [60, 90], TR: [39, 35], GE: [42, 43.5],
  AM: [40, 45], AZ: [40.5, 47.5], IL: [31.5, 34.8], IQ: [33, 44],
  IR: [32, 53], SA: [24, 45], AE: [24, 54], QA: [25.3, 51.2],
  KW: [29.3, 47.5], KZ: [48, 67], PK: [30, 70], IN: [21, 78],
  NP: [28, 84], BD: [24, 90], LK: [7.8, 80.7], MM: [21, 96],
  TH: [15, 101], LA: [18, 103], KH: [12.5, 105], VN: [16, 107],
  MY: [4, 102], SG: [1.35, 103.8], ID: [-2, 118], PH: [12, 122],
  CN: [35, 104], HK: [22.3, 114.2], MO: [22.2, 113.5], TW: [23.7, 121],
  MN: [46, 105], KP: [40, 127], KR: [36.5, 128], JP: [36, 138],
  AU: [-25, 134], NZ: [-41, 174], ZA: [-29, 24], EG: [26, 30],
  MA: [32, -6], NG: [9, 8], KE: [0, 38],
};

const DEG = Math.PI / 180;
/** Degrees between neighbouring land dots. */
export const DOT_STEP = 2;
/** Globe radius as a multiple of the card's half-diagonal. Below 1 the card's
 * corners fall outside the limb, so the sphere's curved edge shows there
 * (empty card background) and more of the region around the exit stays in
 * view. */
export const ZOOM = 0.7;
/** Where the globe rests with no exit to show (East Asia). */
const DEFAULT_VIEW = { lon: 110, lat: 25 };
/** The view centre tilts at most this far, so the poles stay off-centre. */
const MAX_TILT = 60;
/** Dots within this many degrees of the exit take the accent colour. */
const HIGHLIGHT_DEG = 6;
const TURN_MS = 1200;
/** Slow drift while the probe runs, in degrees per second. */
const DRIFT_DEG_PER_S = 8;
/** Expand / collapse flight, on the iOS sheet curve. */
const EXPAND_MS = 560;
/** Collapse: the canvas flies back into the card, rolling up into the globe.
 * Must match lm-zoom-out in index.css, so the canvas and the dialog's frame
 * land together. */
const COLLAPSE_MS = 480;
/** How long an expand waits for the dialog's map slot to mount. */
const SLOT_WAIT_MS = 500;
const SLOT_SELECTOR = "[data-landing-slot]";

/**
 * Flat (expanded) layout: equirectangular, one unit per degree, latitudes
 * trimmed to the land. The dialog's SVG overlay uses the same box with
 * `xMidYMid meet`, so pins drawn there line up with the dots drawn here.
 */
export const LANDING_FLAT = { latTop: 84, latBottom: -58, width: 360, height: 142 } as const;
const FLAT_ALPHA = 0.28;

/** Longitude difference folded into [-180, 180). */
export const wrapLon = (d: number) => ((((d % 360) + 540) % 360) - 180);

/** Where the globe was last drawn facing (as a globe, not mid-unroll). */
let lastGlobeView: { lon: number; lat: number } = { ...DEFAULT_VIEW };
export function getLandingGlobeView(): { lon: number; lat: number } {
  return { ...lastGlobeView };
}
/** Central meridian of the flat map, if the dialog picked one (the midpoint
 * between the user and the exit); otherwise whatever the globe faces. */
let flatCenterOverride: number | null = null;
export function setLandingFlatCenter(lon: number | null): void {
  flatCenterOverride = lon == null ? null : wrapLon(lon);
}
export function landingFlatCenter(): number {
  return flatCenterOverride ?? wrapLon(lastGlobeView.lon);
}

/** cubic-bezier(0.32, 0.72, 0, 1). */
function easeSheet(p: number): number {
  if (p <= 0) return 0;
  if (p >= 1) return 1;
  const bx = (u: number) => 3 * (1 - u) * (1 - u) * u * 0.32 + u * u * u;
  const by = (u: number) => 3 * (1 - u) * (1 - u) * u * 0.72 + 3 * (1 - u) * u * u + u * u * u;
  let lo = 0;
  let hi = 1;
  for (let i = 0; i < 24; i++) {
    const mid = (lo + hi) / 2;
    if (bx(mid) < p) lo = mid;
    else hi = mid;
  }
  return by((lo + hi) / 2);
}

export function insideRing(lon: number, lat: number, ring: Ring): boolean {
  let inside = false;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const [xi, yi] = ring[i];
    const [xj, yj] = ring[j];
    if (yi > lat !== yj > lat && lon < ((xj - xi) * (lat - yi)) / (yj - yi) + xi) {
      inside = !inside;
    }
  }
  return inside;
}

/** Land dots on an equal-area grid, in degrees and as sin/cos/radians. */
export const LAND_DOTS = (() => {
  const latDeg: number[] = [];
  const lonDeg: number[] = [];
  for (let lat = -56 + DOT_STEP / 2; lat < 84; lat += DOT_STEP) {
    const count = Math.max(1, Math.round((360 * Math.cos(lat * DEG)) / DOT_STEP));
    for (let k = 0; k < count; k++) {
      const lon = -180 + ((k + 0.5) * 360) / count;
      if (LAND.some((ring) => insideRing(lon, lat, ring))) {
        latDeg.push(lat);
        lonDeg.push(lon);
      }
    }
  }
  return {
    count: lonDeg.length,
    latDeg: Float32Array.from(latDeg),
    lonDeg: Float32Array.from(lonDeg),
    sinLat: Float32Array.from(latDeg, (v) => Math.sin(v * DEG)),
    cosLat: Float32Array.from(latDeg, (v) => Math.cos(v * DEG)),
    lon: Float32Array.from(lonDeg, (v) => v * DEG),
  };
})();

/**
 * Dots for the globe → flat-map unroll, built on first expand.
 *
 * The globe uses an equal-area grid (fewer dots per row toward the poles),
 * but a flat equirectangular map stretches high latitudes sideways, so those
 * same dots would end up sparse up north and dense near the equator. The
 * flat map instead wants a regular grid: every row has a dot every
 * `DOT_STEP` degrees. So each globe dot is paired with the regular column
 * nearest to it in its row (rows are identical in both grids, and a row
 * never has more globe dots than columns, so pairs are one-to-one); it
 * starts at its globe place and ends on that column. Regular land columns
 * left without a partner are "extra": invisible on the globe, they fade in
 * as the map unrolls, filling the rows out to an even density.
 */
type MorphDots = {
  count: number;
  latDeg: Float32Array;
  /** Globe longitude in degrees (an extra dot's own column). */
  lonDeg: Float32Array;
  sinLat: Float32Array;
  cosLat: Float32Array;
  /** Globe longitude in radians. */
  lon: Float32Array;
  /** Longitude on the flat map's regular grid, degrees. */
  flatLon: Float32Array;
  /** 1 = not on the globe, fades in while unrolling. */
  extra: Uint8Array;
};
let morphDots: MorphDots | null = null;
function getMorphDots(): MorphDots {
  if (morphDots) return morphDots;
  const cols = Math.round(360 / DOT_STEP);
  const colLon = (j: number) => -180 + (j + 0.5) * DOT_STEP;
  const lat: number[] = [];
  const gLon: number[] = [];
  const fLon: number[] = [];
  const extra: number[] = [];
  const G = LAND_DOTS;
  let i = 0;
  for (let row = -56 + DOT_STEP / 2; row < 84; row += DOT_STEP) {
    const used = new Uint8Array(cols);
    // Globe dots of this row (LAND_DOTS is built row by row, in this order).
    while (i < G.count && Math.abs(G.latDeg[i] - row) < 1e-3) {
      const j = Math.min(cols - 1, Math.max(0, Math.floor((G.lonDeg[i] + 180) / DOT_STEP)));
      // Nearest free column (a collision is only possible right at a wrap).
      let k = j;
      for (let step = 1; used[k] && step < cols; step++) {
        k = (j + (step % 2 ? (step + 1) / 2 : -step / 2) + cols) % cols;
      }
      used[k] = 1;
      lat.push(row);
      gLon.push(G.lonDeg[i]);
      fLon.push(colLon(k));
      extra.push(0);
      i++;
    }
    for (let j = 0; j < cols; j++) {
      if (used[j]) continue;
      const lon = colLon(j);
      if (!LAND.some((ring) => insideRing(lon, row, ring))) continue;
      lat.push(row);
      gLon.push(lon);
      fLon.push(lon);
      extra.push(1);
    }
  }
  morphDots = {
    count: lat.length,
    latDeg: Float32Array.from(lat),
    lonDeg: Float32Array.from(gLon),
    sinLat: Float32Array.from(lat, (v) => Math.sin(v * DEG)),
    cosLat: Float32Array.from(lat, (v) => Math.cos(v * DEG)),
    lon: Float32Array.from(gLon, (v) => v * DEG),
    flatLon: Float32Array.from(fLon),
    extra: Uint8Array.from(extra),
  };
  return morphDots;
}

export type LandingMapState = "located" | "probing" | "off" | "idle";

/** Centroid of a country, [lat, lon], or null when it is not in the table. */
export function landingMapCentroid(
  country: string | null | undefined,
): readonly [number, number] | null {
  if (!country) return null;
  return CENTROIDS[country.toUpperCase()] ?? null;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

/** `value` if this canvas understands it as a colour, else `fallback`.
 * Theme tokens are oklch(); an older WebView may not parse that. */
export function resolveColor(ctx: CanvasRenderingContext2D, value: string, fallback: string): string {
  if (!value) return fallback;
  ctx.fillStyle = "#010203";
  ctx.fillStyle = value;
  return ctx.fillStyle === "#010203" ? fallback : value;
}

type Engine = {
  update: (state: LandingMapState, centroid: readonly [number, number] | null) => void;
  setExpanded: (open: boolean) => void;
  dispose: () => void;
};

type Box = { left: number; top: number; width: number; height: number };
const boxOf = (el: Element): Box => {
  const r = el.getBoundingClientRect();
  return { left: r.left, top: r.top, width: r.width, height: r.height };
};

/** How long the globe takes to spring back after a drag. */
const RETURN_MS = 600;
/** Alpha levels the unrolling dots are batched into (one fill each). */
const LEVELS = 16;
const MAX_ALPHA = 0.9;

function createEngine(
  canvas: HTMLCanvasElement,
  ctx: CanvasRenderingContext2D,
  host: HTMLElement,
  markerRef: RefObject<HTMLSpanElement | null>,
): Engine {
  let w = 0;
  let h = 0;
  let dpr = 1;

  let view = { ...DEFAULT_VIEW };
  let from = view;
  let to = view;
  let turnStart = 0;
  let turnMs = TURN_MS;
  let turning = false;
  let drifting = false;
  let state: LandingMapState = "idle";
  let target: readonly [number, number] | null = null;
  let raf = 0;
  let last = 0;
  const reduceMotion =
    typeof matchMedia === "function" &&
    matchMedia("(prefers-reduced-motion: reduce)").matches;

  // Expand / collapse. `morph` is 0 for the globe in the card, 1 for the flat
  // map in the dialog; anything between is mid-flight.
  let expanded = false;
  let morph = 0;
  let flatLonC = 0;
  let pendingTurn = false;
  let stage: HTMLDivElement | null = null;
  let flight: {
    created: number;
    start: number | null;
    ms: number;
    fromMorph: number;
    toMorph: number;
    from: Box;
    target: () => Element | null;
  } | null = null;

  const readColors = () => {
    const dark = document.documentElement.classList.contains("dark");
    const primary = getComputedStyle(document.documentElement)
      .getPropertyValue("--primary")
      .trim();
    return {
      dot: resolveColor(ctx, getComputedStyle(canvas).color, dark ? "#f4f4f5" : "#18181b"),
      accent: resolveColor(ctx, primary, dark ? "#0a84ff" : "#007aff"),
    };
  };
  let colors = readColors();

  // Per-draw scratch: visible dots bucketed by depth (one path and one alpha
  // per bucket), plus the highlighted dots around the exit.
  const BUCKETS = 4;
  const bx = Array.from({ length: BUCKETS }, () => new Float32Array(LAND_DOTS.count));
  const by = Array.from({ length: BUCKETS }, () => new Float32Array(LAND_DOTS.count));
  const bn = new Int32Array(BUCKETS);
  const hx = new Float32Array(LAND_DOTS.count);
  const hy = new Float32Array(LAND_DOTS.count);
  // Unroll scratch: x, y, r triplets per alpha level, plain then accent.
  const lv: number[][] = Array.from({ length: LEVELS * 2 }, () => []);

  const hideMarker = () => {
    const marker = markerRef.current;
    if (marker) marker.style.opacity = "0";
  };

  const drawGlobe = () => {
    const radius = (Math.hypot(w, h) / 2) * ZOOM;
    const cx = w / 2;
    const cy = h / 2;
    const located = state === "located" && target != null;
    const sl0 = Math.sin(view.lat * DEG);
    const cl0 = Math.cos(view.lat * DEG);
    const lon0 = view.lon * DEG;
    const dotR = Math.max(0.6, radius * DOT_STEP * DEG * 0.3);
    const pad = dotR * 2;
    const cosT = target ? Math.cos(target[0] * DEG) : 1;

    bn.fill(0);
    let hn = 0;
    const { count, sinLat, cosLat, lon, latDeg, lonDeg } = LAND_DOTS;
    for (let i = 0; i < count; i++) {
      const d = lon[i] - lon0;
      const cd = Math.cos(d);
      const z = sl0 * sinLat[i] + cl0 * cosLat[i] * cd;
      if (z <= 0) continue;
      const x = cx + cosLat[i] * Math.sin(d) * radius;
      const y = cy - (cl0 * sinLat[i] - sl0 * cosLat[i] * cd) * radius;
      if (x < -pad || x > w + pad || y < -pad || y > h + pad) continue;
      if (located && target) {
        const dLat = latDeg[i] - target[0];
        const dLon = wrapLon(lonDeg[i] - target[1]) * cosT;
        if (dLat * dLat + dLon * dLon <= HIGHLIGHT_DEG * HIGHLIGHT_DEG) {
          hx[hn] = x;
          hy[hn] = y;
          hn++;
          continue;
        }
      }
      const b = Math.min(BUCKETS - 1, Math.floor(z * BUCKETS));
      const n = bn[b]++;
      bx[b][n] = x;
      by[b][n] = y;
    }

    const base = located ? 0.38 : state === "probing" ? 0.34 : 0.22;
    ctx.fillStyle = colors.dot;
    for (let b = 0; b < BUCKETS; b++) {
      const zMid = (b + 0.5) / BUCKETS;
      const r = dotR * (0.7 + 0.3 * zMid);
      ctx.globalAlpha = base * (0.45 + 0.55 * zMid);
      ctx.beginPath();
      for (let k = 0; k < bn[b]; k++) {
        ctx.moveTo(bx[b][k] + r, by[b][k]);
        ctx.arc(bx[b][k], by[b][k], r, 0, Math.PI * 2);
      }
      ctx.fill();
    }
    if (hn > 0) {
      ctx.globalAlpha = MAX_ALPHA;
      ctx.fillStyle = colors.accent;
      ctx.beginPath();
      for (let k = 0; k < hn; k++) {
        ctx.moveTo(hx[k] + dotR, hy[k]);
        ctx.arc(hx[k], hy[k], dotR, 0, Math.PI * 2);
      }
      ctx.fill();
    }
    ctx.globalAlpha = 1;

    // HTML marker on the exit country; hidden once it leaves the card.
    const marker = markerRef.current;
    if (marker) {
      if (located && target) {
        const d = target[1] * DEG - lon0;
        const st = Math.sin(target[0] * DEG);
        const ct = Math.cos(target[0] * DEG);
        const z = sl0 * st + cl0 * ct * Math.cos(d);
        const x = cx + ct * Math.sin(d) * radius;
        const y = cy - (cl0 * st - sl0 * ct * Math.cos(d)) * radius;
        const inside = z > 0 && x >= 0 && x <= w && y >= 0 && y <= h;
        marker.style.transform = `translate(${x}px, ${y}px)`;
        marker.style.opacity = inside ? "1" : "0";
      } else {
        marker.style.opacity = "0";
      }
    }
  };

  /**
   * Globe → flat map at `morph`. Each dot is placed on the globe (as the card
   * draws it, for the current box) and on the flat map (as the dialog shows
   * it), then interpolated; size and alpha follow. Dots on the globe's far
   * side start on its limb with no alpha, so they roll out from behind the
   * edge as the sphere opens up.
   */
  const drawMorph = () => {
    const m = morph;
    const radius = (Math.hypot(w, h) / 2) * ZOOM;
    const cx = w / 2;
    const cy = h / 2;
    // While it unrolls, the globe also turns to face the flat map's centre
    // (meridian `flatLonC`, the map's middle latitude). Otherwise, when the
    // card faces far from that centre (e.g. a US exit with the map centred
    // between China and the US), every dot slides sideways across half the
    // map on top of unrolling. The turn front-loads (done by ~60% of the
    // morph), so the late part is a pure unroll in place. At m = 0 the pose
    // is exactly the card's, so the first frame still matches.
    const tk = Math.min(1, m / 0.6);
    const turn = tk * tk * (3 - 2 * tk);
    const midLat = (LANDING_FLAT.latTop + LANDING_FLAT.latBottom) / 2;
    const vLon = view.lon + wrapLon(flatLonC - view.lon) * turn;
    const vLat = lerp(view.lat, midLat, turn);
    const sl0 = Math.sin(vLat * DEG);
    const cl0 = Math.cos(vLat * DEG);
    const lon0 = vLon * DEG;
    const s = Math.min(w / LANDING_FLAT.width, h / LANDING_FLAT.height);
    const ox = (w - LANDING_FLAT.width * s) / 2;
    const oy = (h - LANDING_FLAT.height * s) / 2;
    const globeR = Math.max(0.6, radius * DOT_STEP * DEG * 0.3);
    const flatR = Math.max(0.6, DOT_STEP * s * 0.3);
    const pad = Math.max(globeR, flatR) * 2;
    const located = state === "located" && target != null;
    const base = located ? 0.38 : state === "probing" ? 0.34 : 0.22;
    const cosT = target ? Math.cos(target[0] * DEG) : 1;

    for (const list of lv) list.length = 0;
    const { count, sinLat, cosLat, lon, latDeg, lonDeg, flatLon, extra } = getMorphDots();
    for (let i = 0; i < count; i++) {
      const d = lon[i] - lon0;
      const cd = Math.cos(d);
      const z = sl0 * sinLat[i] + cl0 * cosLat[i] * cd;
      const isExtra = extra[i] === 1;
      const px = cosLat[i] * Math.sin(d);
      const py = cl0 * sinLat[i] - sl0 * cosLat[i] * cd;
      let gx: number;
      let gy: number;
      if (z > 0) {
        gx = cx + px * radius;
        gy = cy - py * radius;
      } else {
        const n = Math.hypot(px, py) || 1;
        gx = cx + (px / n) * radius;
        gy = cy - (py / n) * radius;
      }
      const fx = ox + (wrapLon(flatLon[i] - flatLonC) + 180) * s;
      const fy = oy + (LANDING_FLAT.latTop - latDeg[i]) * s;
      const x = lerp(gx, fx, m);
      const y = lerp(gy, fy, m);
      if (x < -pad || x > w + pad || y < -pad || y > h + pad) continue;

      let hot = false;
      if (located && target) {
        const dLat = latDeg[i] - target[0];
        const dLon = wrapLon(lonDeg[i] - target[1]) * cosT;
        hot = dLat * dLat + dLon * dLon <= HIGHLIGHT_DEG * HIGHLIGHT_DEG;
      }
      const toLevel = (v: number) => Math.min(LEVELS - 1, Math.round((v / MAX_ALPHA) * (LEVELS - 1)));
      if (hot) {
        // The accent patch marks the exit on the globe. On the flat map the
        // dialog's pin does that, so the patch hands over to plain dots as
        // the map unrolls (no stray accent dots left beside the pin).
        const r = lerp(globeR, flatR, m);
        const accent = toLevel((z > 0 && !isExtra ? MAX_ALPHA : 0) * (1 - m));
        const plain = toLevel(FLAT_ALPHA * m);
        if (accent > 0) lv[LEVELS + accent].push(x, y, r);
        if (plain > 0) lv[plain].push(x, y, r);
        continue;
      }
      // Dots coming round the limb as the globe turns fade in from nothing
      // instead of popping in at the limb's alpha (only while turning, so
      // the first frame still matches the card exactly).
      const limbFade = lerp(1, Math.min(1, Math.max(0, z) / 0.15), turn);
      const ga = z > 0 && !isExtra ? base * (0.45 + 0.55 * z) * limbFade : 0;
      const gr = z > 0 ? globeR * (0.7 + 0.3 * z) : globeR * 0.7;
      const level = toLevel(lerp(ga, FLAT_ALPHA, m));
      if (level <= 0) continue;
      lv[level].push(x, y, lerp(gr, flatR, m));
    }

    for (let k = 0; k < lv.length; k++) {
      const list = lv[k];
      if (list.length === 0) continue;
      ctx.fillStyle = k >= LEVELS ? colors.accent : colors.dot;
      ctx.globalAlpha = ((k % LEVELS) / (LEVELS - 1)) * MAX_ALPHA;
      ctx.beginPath();
      for (let j = 0; j < list.length; j += 3) {
        ctx.moveTo(list[j] + list[j + 2], list[j + 1]);
        ctx.arc(list[j], list[j + 1], list[j + 2], 0, Math.PI * 2);
      }
      ctx.fill();
    }
    ctx.globalAlpha = 1;
    hideMarker();
  };

  const draw = () => {
    if (w <= 0 || h <= 0) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    if (morph > 0) {
      drawMorph();
    } else {
      lastGlobeView = { ...view };
      drawGlobe();
    }
  };

  /** Size the backing store for a w × h box (CSS px). */
  const setSize = (nw: number, nh: number) => {
    dpr = Math.min(2, window.devicePixelRatio || 1);
    w = nw;
    h = nh;
    canvas.width = Math.max(1, Math.round(w * dpr));
    canvas.height = Math.max(1, Math.round(h * dpr));
  };

  const placeStage = (box: Box) => {
    if (!stage) return;
    stage.style.left = `${box.left}px`;
    stage.style.top = `${box.top}px`;
    stage.style.width = `${box.width}px`;
    stage.style.height = `${box.height}px`;
    setSize(box.width, box.height);
  };

  const turnTo = (centroid: readonly [number, number]) => {
    // Shortest way round in longitude.
    const dLon = wrapLon(centroid[1] - view.lon);
    from = { ...view };
    to = { lon: view.lon + dLon, lat: clamp(centroid[0], -MAX_TILT, MAX_TILT) };
    if (reduceMotion) {
      view = to;
      turning = false;
      draw();
      return;
    }
    turnStart = performance.now();
    turnMs = TURN_MS;
    turning = true;
    kick();
  };

  /** End of a flight: hand the canvas to the slot (expanded) or the card. */
  const land = (slot: Element | null) => {
    const f = flight;
    flight = null;
    if (f && f.toMorph === 1 && slot) {
      morph = 1;
      slot.prepend(canvas);
    } else {
      morph = 0;
      host.prepend(canvas);
      canvas.style.cursor = "grab";
    }
    stage?.remove();
    stage = null;
    resize();
    if (morph === 0) {
      if (pendingTurn && target) {
        pendingTurn = false;
        turnTo(target);
      } else if (drifting) {
        kick();
      }
    }
  };

  const frame = (now: number) => {
    raf = 0;
    const dt = last ? Math.min(0.05, (now - last) / 1000) : 0;
    last = now;
    let more = false;
    if (flight) {
      const f = flight;
      const el = f.target();
      if (!el && f.toMorph === 1) {
        // The dialog's slot is not mounted yet: hold still, give up later.
        if (now - f.created > SLOT_WAIT_MS) land(null);
        else more = true;
      } else {
        if (f.start == null) f.start = now;
        const p = f.ms > 0 ? Math.min(1, (now - f.start) / f.ms) : 1;
        const e = easeSheet(p);
        morph = lerp(f.fromMorph, f.toMorph, e);
        if (p < 1 && el) {
          const b = boxOf(el);
          placeStage({
            left: lerp(f.from.left, b.left, e),
            top: lerp(f.from.top, b.top, e),
            width: lerp(f.from.width, b.width, e),
            height: lerp(f.from.height, b.height, e),
          });
          more = true;
        } else {
          land(el);
        }
      }
    } else if (morph === 0) {
      if (turning) {
        const p = Math.min(1, (now - turnStart) / turnMs);
        const e = p < 0.5 ? 4 * p * p * p : 1 - (-2 * p + 2) ** 3 / 2;
        view = { lon: from.lon + (to.lon - from.lon) * e, lat: from.lat + (to.lat - from.lat) * e };
        if (p < 1) more = true;
        else {
          turning = false;
          // A drag's spring-back ends where the probe's slow drift resumes.
          if (drifting) more = true;
        }
      } else if (drifting) {
        view = { lon: view.lon + dt * DRIFT_DEG_PER_S, lat: view.lat };
        more = true;
      }
    }
    draw();
    if (more && document.visibilityState !== "hidden") {
      if (!raf) raf = requestAnimationFrame(frame);
    } else if (!raf) {
      last = 0;
    }
  };

  const kick = () => {
    if (raf) return;
    last = 0;
    raf = requestAnimationFrame(frame);
  };

  const resize = () => {
    // A flight sizes the canvas itself, every frame.
    if (flight) return;
    // Layout size: unaffected by any transform on an ancestor.
    setSize(canvas.clientWidth, canvas.clientHeight);
    draw();
  };

  const onVisibility = () => {
    if (document.visibilityState === "visible" && (turning || drifting || flight)) kick();
  };

  /** Lift the canvas onto the stage where it is now, then fly it. */
  const startFlight = (toMorph: number, targetEl: () => Element | null, ms: number) => {
    const box = boxOf(canvas);
    if (!stage) {
      stage = document.createElement("div");
      stage.className = "landing-map-stage";
      stage.setAttribute("aria-hidden", "true");
      stage.style.borderRadius = getComputedStyle(host).borderRadius;
      document.body.append(stage);
    }
    if (canvas.parentElement !== stage) stage.append(canvas);
    placeStage(box);
    const now = performance.now();
    flight = {
      created: now,
      start: toMorph === 1 ? null : now,
      ms: reduceMotion ? 0 : ms,
      fromMorph: morph,
      toMorph,
      from: box,
      target: targetEl,
    };
    turning = false;
    draw();
    kick();
  };

  // Drag to turn the globe; on release it springs back to where it was (or
  // to where an in-flight turn was heading), then the probe's drift resumes.
  let dragging: {
    x: number;
    y: number;
    start: { lon: number; lat: number };
    rest: { lon: number; lat: number };
    drift: boolean;
  } | null = null;
  const onPointerDown = (e: PointerEvent) => {
    if (e.button !== 0 || expanded || morph > 0) return;
    dragging = {
      x: e.clientX,
      y: e.clientY,
      start: { ...view },
      rest: turning ? { ...to } : { ...view },
      drift: drifting,
    };
    turning = false;
    drifting = false;
    canvas.setPointerCapture?.(e.pointerId);
    canvas.style.cursor = "grabbing";
  };
  const onPointerMove = (e: PointerEvent) => {
    if (!dragging) return;
    const radius = (Math.hypot(w, h) / 2) * ZOOM;
    if (radius <= 0) return;
    // One radius of drag ≈ one radian of rotation.
    const k = 180 / Math.PI / radius;
    view = {
      lon: dragging.start.lon - (e.clientX - dragging.x) * k,
      lat: clamp(dragging.start.lat + (e.clientY - dragging.y) * k, -85, 85),
    };
    draw();
  };
  const onPointerUp = (e: PointerEvent) => {
    if (!dragging) return;
    const { rest, drift } = dragging;
    dragging = null;
    canvas.releasePointerCapture?.(e.pointerId);
    canvas.style.cursor = "grab";
    drifting = drift && !reduceMotion;
    // Back the short way round in longitude.
    const dLon = wrapLon(rest.lon - view.lon);
    from = { ...view };
    to = { lon: view.lon + dLon, lat: rest.lat };
    if (reduceMotion) {
      view = to;
      draw();
      return;
    }
    turnStart = performance.now();
    turnMs = RETURN_MS;
    turning = true;
    kick();
  };
  canvas.style.cursor = "grab";
  canvas.style.touchAction = "none";
  canvas.addEventListener("pointerdown", onPointerDown);
  canvas.addEventListener("pointermove", onPointerMove);
  canvas.addEventListener("pointerup", onPointerUp);
  canvas.addEventListener("pointercancel", onPointerUp);

  const resizeObserver =
    typeof ResizeObserver === "function" ? new ResizeObserver(resize) : null;
  resizeObserver?.observe(canvas);
  const themeObserver =
    typeof MutationObserver === "function"
      ? new MutationObserver(() => {
          colors = readColors();
          draw();
        })
      : null;
  themeObserver?.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["class", "data-theme", "style"],
  });
  document.addEventListener("visibilitychange", onVisibility);
  resize();

  return {
    update(nextState, centroid) {
      state = nextState;
      target = centroid;
      drifting = nextState === "probing" && !reduceMotion;
      if (expanded || morph > 0) {
        // Keep the globe's pose while it is the dialog's map; turn later.
        pendingTurn = centroid != null;
        draw();
        return;
      }
      if (centroid) {
        turnTo(centroid);
        return;
      }
      turning = false;
      if (drifting) kick();
      else draw();
    },
    setExpanded(open) {
      if (open === expanded) return;
      expanded = open;
      if (open) {
        // Same value the dialog reads while rendering, for its pins.
        flatLonC = landingFlatCenter();
        dragging = null;
        canvas.style.cursor = "";
        startFlight(1, () => document.querySelector(SLOT_SELECTOR), EXPAND_MS);
      } else {
        // An expand still waiting for the dialog's slot never left the card.
        if (flight && flight.toMorph === 1 && flight.start == null) {
          land(null);
          return;
        }
        // The reverse of the expand: from wherever the canvas is now (the
        // dialog's slot, or mid-flight), fly it back into the card while it
        // rolls up into the globe. The dialog's frame shrinks into the card
        // underneath it on the same curve (lm-zoom-out).
        startFlight(0, () => host, COLLAPSE_MS);
      }
    },
    dispose() {
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
      flight = null;
      stage?.remove();
      stage = null;
      resizeObserver?.disconnect();
      themeObserver?.disconnect();
      document.removeEventListener("visibilitychange", onVisibility);
      canvas.removeEventListener("pointerdown", onPointerDown);
      canvas.removeEventListener("pointermove", onPointerMove);
      canvas.removeEventListener("pointerup", onPointerUp);
      canvas.removeEventListener("pointercancel", onPointerUp);
    },
  };
}

export function LandingMap({
  country,
  state,
  expanded = false,
  className,
}: {
  /** ISO alpha-2 of the probed exit; only used while `state` is `located`. */
  country: string | null | undefined;
  state: LandingMapState;
  /** True while LandingMapDialog is open: the canvas lives in its slot. */
  expanded?: boolean;
  className?: string;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const markerRef = useRef<HTMLSpanElement>(null);
  const engineRef = useRef<Engine | null>(null);
  const centroid = state === "located" ? landingMapCentroid(country) : null;
  const lat = centroid?.[0];
  const lon = centroid?.[1];

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const canvas = document.createElement("canvas");
    canvas.className = "absolute inset-0 size-full text-foreground";
    host.prepend(canvas);
    // No 2D backend (e.g. jsdom in tests): leave the markup without a globe.
    let ctx: CanvasRenderingContext2D | null = null;
    try {
      ctx = canvas.getContext("2d");
    } catch {
      ctx = null;
    }
    if (!ctx) return () => canvas.remove();
    const engine = createEngine(canvas, ctx, host, markerRef);
    engineRef.current = engine;
    return () => {
      engine.dispose();
      engineRef.current = null;
      canvas.remove();
    };
  }, []);

  useEffect(() => {
    engineRef.current?.update(
      state,
      lat != null && lon != null ? ([lat, lon] as const) : null,
    );
  }, [state, lat, lon]);

  useEffect(() => {
    engineRef.current?.setExpanded(expanded);
  }, [expanded]);

  return (
    <div
      ref={hostRef}
      aria-hidden="true"
      data-testid="landing-map"
      data-state={state}
      className={cn(
        "landing-map relative h-36 w-full select-none overflow-hidden rounded-lg",
        className,
      )}
    >
      {centroid ? (
        <span
          ref={markerRef}
          data-testid="landing-map-marker"
          data-country={country?.toUpperCase()}
          className="landing-map-marker"
          style={{ opacity: 0 }}
        />
      ) : null}
    </div>
  );
}
