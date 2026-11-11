// SPDX-License-Identifier: GPL-3.0-or-later
//
// Render the ice-box switch logo out of a pixelfx checkout into this directory's assets.
//
//   node docs/brand/switch-logo/tools/export.mjs <pixelfx checkout>
//
// The effect must already be copied to <pixelfx>/src/fx/brand/switch-logo.js. Writes, under
// docs/brand/switch-logo/assets:
//   logo.png, logo@4x.png             the plain logo, still, transparent
//   states/<state>.png                a horizontal strip of the state's loop, 48 px frames, transparent
//   states/<state>-still.png, @4x     one still frame of the state, transparent
//   manifest.json                     frame size, frame count and fps of the strips
import { pathToFileURL, fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { mkdirSync, writeFileSync } from "node:fs";

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
const fx = await lib.loadEffect("brand/switch-logo");
const [W, H] = fx.meta.size;

// Crop a rendered RGBA buffer (with pixelfx's render margin m) back to the effect's own size.
function crop(rgba, w, m) {
  const o = new Uint8ClampedArray(W * H * 4);
  for (let y = 0; y < H; y++) o.set(rgba.subarray(((y + m) * w + m) * 4, ((y + m) * w + m + W) * 4), y * W * 4);
  return o;
}

function still(options, name) {
  const r = lib.renderFrames(fx, { fps: 1, bg: "none", actor: false, options: { ...options, still: STILL } });
  const px = crop(r.frames[0].clear, r.w, r.margin);
  writeFileSync(join(out, `${name}.png`), encodePNG(W, H, px));
  writeFileSync(join(out, `${name}@4x.png`), encodePNG(W * 4, H * 4, lib.upscale(px, W, H, 4)));
}

function strip(options, name) {
  const r = lib.renderFrames(fx, { fps: FPS, bg: "none", actor: false, options });
  const n = r.frames.length, px = new Uint8ClampedArray(W * n * H * 4);
  r.frames.forEach((f, i) => {
    const c = crop(f.clear, r.w, r.margin);
    for (let y = 0; y < H; y++) px.set(c.subarray(y * W * 4, (y + 1) * W * 4), (y * W * n + i * W) * 4);
  });
  writeFileSync(join(out, `${name}.png`), encodePNG(W * n, H, px));
  return n;
}

mkdirSync(join(out, "states"), { recursive: true });
still({}, "logo");
let frames = 0;
for (const state of STATES) {
  frames = strip({ state }, `states/${state}`);
  still({ state }, `states/${state}-still`);
}
const manifest = { frame: [W, H], frames, fps: FPS, duration: fx.meta.duration, states: STATES };
writeFileSync(join(out, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
console.log(`wrote ${out}: logo + ${STATES.length} states, ${frames} frames each at ${FPS} fps`);
