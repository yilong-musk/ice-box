// SPDX-License-Identifier: GPL-3.0-or-later
//
// Render the ice-box switch logo out of a pixelfx checkout into this directory's assets,
// then publish the copies the apps actually load.
//
//   node docs/brand/switch-logo/tools/export.mjs <pixelfx checkout>
//
// The effects must already be copied to <pixelfx>/src/fx/brand/switch-logo.js,
// switch-logo-24.js, switch-logo-32.js and _switch-logo-small.js. Writes, under
// docs/brand/switch-logo/assets:
//   logo.png, logo@4x.png             the plain logo, still, transparent
//   logo.gif                          the running loop as a 288x210 px looping GIF
//   states/<state>.png                a horizontal strip of the state's loop, 48 px frames, transparent
//   states/<state>-still.png, @4x     one still frame of the state, transparent
//   states-24/, states-32/            the same set redrawn at 24 px and 32 px
//   manifest.json, states-*/manifest.json
//                                     frame size, frame count and fps of each set
// and copies the 48 px set (32 px under 32/) to apps/desktop/src/assets/switch-logo/.
// From the plain logo it also writes:
//   apps/desktop/src-tauri/icons/tray.png
//                                     the opaque mark, 1x, for the macOS menu bar
//   apps/desktop/src-tauri/icons/tray-square.png
//                                     the same mark at 2x, letterboxed square, for Windows and Linux
//   apps/mobile/.../mipmap-*/ic_launcher_foreground.png
//                                     the plain logo fitted inside the 66 dp adaptive-icon keyline
import { pathToFileURL, fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { copyFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { encodeGif } from "./gif.mjs";

const STATES = ["stopped", "starting", "running", "stopping", "error"];
const FPS = 15; // strip frame rate: 45 frames over the 3 s loop
// Still frames are what StatusLogo shows when the window is in the background
// or the user prefers reduced motion, so the phase has to read as that state.
// 0.95 sits in stopping's final all-dark beat and at the tail of starting's
// chase. 0.215 puts that chase across two columns on both the 48 px and the
// 32 px grids (0.25 leaves the 32 px phone still with a single lamp). 0.4
// leaves stopping's amber lit on the left half of every grid.
const STILL_PHASE = {
  "": 0.95,
  stopped: 0.95,
  starting: 0.215,
  running: 0.95,
  stopping: 0.4,
  error: 0.95,
};

// Android adaptive icons are a 108 dp layer. Masks can cover anything outside
// the center 66 dp keyline; a circular launcher shows at most the inner 72 dp.
// Keep every opaque pixel of the mark 1 dp inside the keyline circle.
const ADAPTIVE_DP = 108;
const KEYLINE_DIAMETER_DP = 66;
const KEYLINE_MARGIN_DP = 1;
const DENSITIES = [
  ["mdpi", 1],
  ["hdpi", 1.5],
  ["xhdpi", 2],
  ["xxhdpi", 3],
  ["xxxhdpi", 4],
];

const pixelfx = process.argv[2];
if (!pixelfx) {
  console.error("usage: node export.mjs <pixelfx checkout>");
  process.exit(1);
}
const root = resolve(pixelfx);
const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, "..", "assets");
const repo = join(here, "..", "..", "..", "..");
const lib = await import(pathToFileURL(join(root, "tools/lib/fxlib.mjs")).href);
const { encodePNG } = await import(pathToFileURL(join(root, "tools/lib/png.mjs")).href);

function stillPhase(state) {
  const phase = STILL_PHASE[state];
  if (phase === undefined) throw new Error(`no still phase for state ${JSON.stringify(state)}`);
  return phase;
}

// Crop a rendered RGBA buffer (with pixelfx's render margin m) back to the effect's own size.
function crop(rgba, w, m, W, H) {
  const o = new Uint8ClampedArray(W * H * 4);
  for (let y = 0; y < H; y++) o.set(rgba.subarray(((y + m) * w + m) * 4, ((y + m) * w + m + W) * 4), y * W * 4);
  return o;
}

function stillPixels(fx, options) {
  const [W, H] = fx.meta.size;
  const phase = stillPhase(options.state ?? "");
  const r = lib.renderFrames(fx, { fps: 1, bg: "none", actor: false, options: { ...options, still: phase } });
  return { W, H, px: crop(r.frames[0].clear, r.w, r.margin, W, H) };
}

function writeStill(fx, options, file) {
  const { W, H, px } = stillPixels(fx, options);
  writeFileSync(`${file}.png`, encodePNG(W, H, px));
  writeFileSync(`${file}@4x.png`, encodePNG(W * 4, H * 4, lib.upscale(px, W, H, 4)));
  return px;
}

function writeStrip(fx, options, file) {
  const [W, H] = fx.meta.size;
  const r = lib.renderFrames(fx, { fps: FPS, bg: "none", actor: false, options });
  const n = r.frames.length, px = new Uint8ClampedArray(W * n * H * 4);
  r.frames.forEach((f, i) => {
    const c = crop(f.clear, r.w, r.margin, W, H);
    for (let y = 0; y < H; y++) px.set(c.subarray(y * W * 4, (y + 1) * W * 4), (y * W * n + i * W) * 4);
  });
  writeFileSync(`${file}.png`, encodePNG(W * n, H, px));
  return n;
}

function writeManifest(file, frame, frames, duration) {
  const manifest = { frame, frames, fps: FPS, duration, states: STATES };
  writeFileSync(file, JSON.stringify(manifest, null, 2) + "\n");
}

function diffPixels(a, b) {
  if (a.length !== b.length) return Math.max(a.length, b.length);
  let n = 0;
  for (let i = 0; i < a.length; i += 4) {
    if (a[i] !== b[i] || a[i + 1] !== b[i + 1] || a[i + 2] !== b[i + 2] || a[i + 3] !== b[i + 3]) n++;
  }
  return n;
}

// A shared phase used to make stopping's still identical to stopped. Each
// state's still has to carry its own port lights.
function assertDistinctStills(stills, size) {
  for (const state of ["starting", "running", "stopping", "error"]) {
    const n = diffPixels(stills.stopped, stills[state]);
    if (n === 0) throw new Error(`${size} px ${state} still matches stopped`);
  }
  if (diffPixels(stills.starting, stills.stopping) === 0) {
    throw new Error(`${size} px starting still matches stopping`);
  }
}

// The README uses a GIF because Markdown cannot play the CSS sprite. GIF has
// 1-bit transparency, so the faint glow and motes fall away; 6x and 7 cs keep
// the pixels crisp and the loop as close to 15 fps as GIF's timing allows.
// The 48 px canvas has empty rows above and below the mark; trim most of them
// so the README does not carry a tall band of transparent padding.
const GIF_TOP = 8, GIF_H = 35;

function cropRows(rgba, w, y0, h) {
  const o = new Uint8ClampedArray(w * h * 4);
  o.set(rgba.subarray(y0 * w * 4, (y0 + h) * w * 4));
  return o;
}

function writeRunningGif(fx) {
  const [W, H] = fx.meta.size;
  const r = lib.renderFrames(fx, { fps: FPS, bg: "none", actor: false, options: { state: "running" } });
  const frames = r.frames.map((f) => cropRows(crop(f.clear, r.w, r.margin, W, H), W, GIF_TOP, GIF_H));
  const gif = encodeGif(W, GIF_H, frames, { scale: 6, delay: 7 });
  writeFileSync(join(out, "logo.gif"), gif);
  return gif.length;
}

function copyAppAssets() {
  const app = join(repo, "apps/desktop/src/assets/switch-logo");
  copyFileSync(join(out, "logo.png"), join(app, "logo.png"));
  copyFileSync(join(out, "logo@4x.png"), join(app, "logo@4x.png"));
  for (const state of STATES) {
    copyFileSync(join(out, "states", `${state}.png`), join(app, `${state}.png`));
    copyFileSync(join(out, "states", `${state}-still.png`), join(app, `${state}-still.png`));
    copyFileSync(join(out, "states-32", `${state}.png`), join(app, "32", `${state}.png`));
    copyFileSync(join(out, "states-32", `${state}-still.png`), join(app, "32", `${state}-still.png`));
  }
}

// Inclusive bounds of opaque pixels, the center of that box, and the distance
// from that center to the farthest opaque corner (source pixels).
function opaqueMetrics(px, w, h) {
  let x0 = w, y0 = h, x1 = -1, y1 = -1;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      if (px[(y * w + x) * 4 + 3] === 0) continue;
      if (x < x0) x0 = x;
      if (y < y0) y0 = y;
      if (x > x1) x1 = x;
      if (y > y1) y1 = y;
    }
  }
  if (x1 < 0) throw new Error("logo has no opaque pixels");
  const cx = (x0 + x1 + 1) / 2, cy = (y0 + y1 + 1) / 2;
  let far = 0;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      if (px[(y * w + x) * 4 + 3] === 0) continue;
      for (const dx of [0, 1]) for (const dy of [0, 1]) {
        const r = Math.hypot(x + dx - cx, y + dy - cy);
        if (r > far) far = r;
      }
    }
  }
  return { x0, y0, x1, y1, cx, cy, far };
}

function cropToOpaque(px, w, metrics) {
  const cw = metrics.x1 - metrics.x0 + 1, ch = metrics.y1 - metrics.y0 + 1;
  const out = new Uint8ClampedArray(cw * ch * 4);
  for (let y = 0; y < ch; y++) {
    const s = ((y + metrics.y0) * w + metrics.x0) * 4;
    out.set(px.subarray(s, s + cw * 4), y * cw * 4);
  }
  return { px: out, w: cw, h: ch };
}

// macOS draws tray.png at half its pixel size in points (see
// size_macos_tray_icon): a 44×27 crop is 22×13.5 pt, one bitmap pixel per
// device pixel on a 2× display. The square bitmap is the same crop at 2×,
// letterboxed, because Windows and Linux scale a tray icon into a square slot.
function writeTray(px, w, h) {
  const cropped = cropToOpaque(px, w, opaqueMetrics(px, w, h));
  const icons = join(repo, "apps/desktop/src-tauri/icons");
  writeFileSync(join(icons, "tray.png"), encodePNG(cropped.w, cropped.h, cropped.px));
  const big = lib.upscale(cropped.px, cropped.w, cropped.h, 2);
  const bw = cropped.w * 2, bh = cropped.h * 2;
  const side = Math.max(bw, bh);
  const ox = (side - bw) / 2, oy = (side - bh) / 2;
  if (!Number.isInteger(ox) || !Number.isInteger(oy)) {
    throw new Error(`tray letterbox offset ${ox},${oy} is not on a pixel`);
  }
  const square = new Uint8ClampedArray(side * side * 4);
  for (let y = 0; y < bh; y++) {
    square.set(big.subarray(y * bw * 4, (y + 1) * bw * 4), ((y + oy) * side + ox) * 4);
  }
  writeFileSync(join(icons, "tray-square.png"), encodePNG(side, side, square));
  return { w: cropped.w, h: cropped.h, side };
}

// Nearest-neighbour sample of the plain logo onto one density's 108 dp layer.
// Output pixel centers map back onto the source so the mark stays centered
// and one source pixel stays a block of output pixels.
function renderForeground(src, sw, sh, metrics, density) {
  const canvas = Math.round(ADAPTIVE_DP * density);
  const out = new Uint8ClampedArray(canvas * canvas * 4);
  const scale = (KEYLINE_DIAMETER_DP / 2 - KEYLINE_MARGIN_DP) / metrics.far;
  const center = ADAPTIVE_DP / 2;
  for (let oy = 0; oy < canvas; oy++) {
    const sy = ((oy + 0.5) / density - center) / scale + metrics.cy;
    const isy = Math.floor(sy);
    if (isy < 0 || isy >= sh) continue;
    for (let ox = 0; ox < canvas; ox++) {
      const sx = ((ox + 0.5) / density - center) / scale + metrics.cx;
      const isx = Math.floor(sx);
      if (isx < 0 || isx >= sw) continue;
      const si = (isy * sw + isx) * 4;
      if (src[si + 3] === 0) continue;
      const di = (oy * canvas + ox) * 4;
      out[di] = src[si];
      out[di + 1] = src[si + 1];
      out[di + 2] = src[si + 2];
      out[di + 3] = src[si + 3];
    }
  }
  return { out, canvas, scale };
}

// Pixel corners, not just centers: a mask clips the square a pixel covers.
function keylineOverflow(px, canvas, density) {
  const radius = (KEYLINE_DIAMETER_DP / 2) * density;
  const limit = radius * radius;
  const cx = canvas / 2, cy = canvas / 2;
  let n = 0;
  for (let y = 0; y < canvas; y++) {
    for (let x = 0; x < canvas; x++) {
      if (px[(y * canvas + x) * 4 + 3] === 0) continue;
      let outside = false;
      for (const dx of [0, 1]) for (const dy of [0, 1]) {
        const ddx = x + dx - cx, ddy = y + dy - cy;
        if (ddx * ddx + ddy * ddy > limit + 1e-4) outside = true;
      }
      if (outside) n++;
    }
  }
  return n;
}

function writeAdaptiveIcons(src, sw, sh) {
  const res = join(repo, "apps/mobile/src-tauri/gen/android/app/src/main/res");
  if (!existsSync(res)) throw new Error(`android res dir missing: ${res}`);
  const metrics = opaqueMetrics(src, sw, sh);
  let scale = 0;
  for (const [dpi, density] of DENSITIES) {
    const rendered = renderForeground(src, sw, sh, metrics, density);
    scale = rendered.scale;
    const overflow = keylineOverflow(rendered.out, rendered.canvas, density);
    if (overflow !== 0) throw new Error(`${dpi} foreground has ${overflow} pixels outside the 66 dp keyline`);
    const file = join(res, `mipmap-${dpi}`, "ic_launcher_foreground.png");
    writeFileSync(file, encodePNG(rendered.canvas, rendered.canvas, rendered.out));
  }
  return scale;
}

// 48 px: the original effect and the legacy asset paths.
const fx48 = await lib.loadEffect("brand/switch-logo");
mkdirSync(join(out, "states"), { recursive: true });
const plain = stillPixels(fx48, {});
writeFileSync(join(out, "logo.png"), encodePNG(plain.W, plain.H, plain.px));
writeFileSync(join(out, "logo@4x.png"), encodePNG(plain.W * 4, plain.H * 4, lib.upscale(plain.px, plain.W, plain.H, 4)));
let frames = 0;
const stills48 = {};
for (const state of STATES) {
  frames = writeStrip(fx48, { state }, join(out, "states", state));
  stills48[state] = writeStill(fx48, { state }, join(out, "states", `${state}-still`));
}
assertDistinctStills(stills48, 48);
writeManifest(join(out, "manifest.json"), fx48.meta.size, frames, fx48.meta.duration);
const gifBytes = writeRunningGif(fx48);

// 24 px and 32 px: redrawn at that grid, not downscaled, so the ports and the
// two-pixel lights stay on the pixel grid. Each set is self-contained under
// states-<size>/.
for (const size of [24, 32]) {
  const fx = await lib.loadEffect(`brand/switch-logo-${size}`);
  const dir = join(out, `states-${size}`);
  mkdirSync(dir, { recursive: true });
  writeStill(fx, {}, join(dir, "logo"));
  const stills = {};
  frames = 0;
  for (const state of STATES) {
    frames = writeStrip(fx, { state }, join(dir, state));
    stills[state] = writeStill(fx, { state }, join(dir, `${state}-still`));
  }
  assertDistinctStills(stills, size);
  writeManifest(join(dir, "manifest.json"), fx.meta.size, frames, fx.meta.duration);
}

copyAppAssets();
const tray = writeTray(plain.px, plain.W, plain.H);
const adaptiveScale = writeAdaptiveIcons(plain.px, plain.W, plain.H);

console.log(`wrote ${out}: logo + logo.gif (${gifBytes} bytes) + ${STATES.length} states at 48, 32 and 24 px, ${frames} frames each at ${FPS} fps`);
console.log(`tray ${tray.w}x${tray.h} (square ${tray.side}), adaptive foreground scale ${adaptiveScale.toFixed(3)} dp/px`);
