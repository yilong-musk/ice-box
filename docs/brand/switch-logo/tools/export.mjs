// SPDX-License-Identifier: GPL-3.0-or-later
//
// Render the ice-box switch logo out of a pixelfx checkout into this directory's assets.
//
//   node docs/brand/switch-logo/tools/export.mjs <pixelfx checkout>
//
// The effects must already be copied to <pixelfx>/src/fx/brand/switch-logo.js,
// switch-logo-24.js, switch-logo-32.js and _switch-logo-small.js. Writes, under
// docs/brand/switch-logo/assets:
//   logo.png, logo@4x.png             the plain logo, still, transparent
//   logo.gif                          the running loop as a 192 px looping GIF
//   states/<state>.png                a horizontal strip of the state's loop, 48 px frames, transparent
//   states/<state>-still.png, @4x     one still frame of the state, transparent
//   states-24/, states-32/            the same set redrawn at 24 px and 32 px
//   manifest.json, states-*/manifest.json
//                                     frame size, frame count and fps of each set
import { pathToFileURL, fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { mkdirSync, writeFileSync } from "node:fs";
import { encodeGif } from "./gif.mjs";

const STATES = ["stopped", "starting", "running", "stopping", "error"];
const FPS = 15; // strip frame rate: 45 frames over the 3 s loop
const STILL = 0.95; // loop phase used for still frames

const pixelfx = process.argv[2];
if (!pixelfx) {
  console.error("usage: node export.mjs <pixelfx checkout>");
  process.exit(1);
}
const root = resolve(pixelfx);
const out = join(dirname(fileURLToPath(import.meta.url)), "..", "assets");
const lib = await import(pathToFileURL(join(root, "tools/lib/fxlib.mjs")).href);
const { encodePNG } = await import(pathToFileURL(join(root, "tools/lib/png.mjs")).href);

// Crop a rendered RGBA buffer (with pixelfx's render margin m) back to the effect's own size.
function crop(rgba, w, m, W, H) {
  const o = new Uint8ClampedArray(W * H * 4);
  for (let y = 0; y < H; y++) o.set(rgba.subarray(((y + m) * w + m) * 4, ((y + m) * w + m + W) * 4), y * W * 4);
  return o;
}

function stillPixels(fx, options) {
  const [W, H] = fx.meta.size;
  const r = lib.renderFrames(fx, { fps: 1, bg: "none", actor: false, options: { ...options, still: STILL } });
  return { W, H, px: crop(r.frames[0].clear, r.w, r.margin, W, H) };
}

function writeStill(fx, options, file) {
  const { W, H, px } = stillPixels(fx, options);
  writeFileSync(`${file}.png`, encodePNG(W, H, px));
  writeFileSync(`${file}@4x.png`, encodePNG(W * 4, H * 4, lib.upscale(px, W, H, 4)));
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

// The README uses a GIF because Markdown cannot play the CSS sprite. GIF has
// 1-bit transparency, so the faint glow and motes fall away; 4x and 7 cs keep
// the pixels crisp and the loop as close to 15 fps as GIF's timing allows.
function writeRunningGif(fx) {
  const [W, H] = fx.meta.size;
  const r = lib.renderFrames(fx, { fps: FPS, bg: "none", actor: false, options: { state: "running" } });
  const frames = r.frames.map((f) => crop(f.clear, r.w, r.margin, W, H));
  const gif = encodeGif(W, H, frames, { scale: 4, delay: 7 });
  writeFileSync(join(out, "logo.gif"), gif);
  return gif.length;
}

// 48 px: the original effect and the legacy asset paths.
const fx48 = await lib.loadEffect("brand/switch-logo");
mkdirSync(join(out, "states"), { recursive: true });
writeStill(fx48, {}, join(out, "logo"));
let frames = 0;
for (const state of STATES) {
  frames = writeStrip(fx48, { state }, join(out, "states", state));
  writeStill(fx48, { state }, join(out, "states", `${state}-still`));
}
writeManifest(join(out, "manifest.json"), fx48.meta.size, frames, fx48.meta.duration);
const gifBytes = writeRunningGif(fx48);

// 24 px and 32 px: redrawn at that grid, not downscaled, so the ports and lights
// stay one pixel wide. Each set is self-contained under states-<size>/.
for (const size of [24, 32]) {
  const fx = await lib.loadEffect(`brand/switch-logo-${size}`);
  const dir = join(out, `states-${size}`);
  mkdirSync(dir, { recursive: true });
  writeStill(fx, {}, join(dir, "logo"));
  frames = 0;
  for (const state of STATES) {
    frames = writeStrip(fx, { state }, join(dir, state));
    writeStill(fx, { state }, join(dir, `${state}-still`));
  }
  writeManifest(join(dir, "manifest.json"), fx.meta.size, frames, fx.meta.duration);
}

console.log(`wrote ${out}: logo + logo.gif (${gifBytes} bytes) + ${STATES.length} states at 48, 32 and 24 px, ${frames} frames each at ${FPS} fps`);
