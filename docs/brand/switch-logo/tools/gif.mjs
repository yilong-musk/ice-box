// SPDX-License-Identifier: GPL-3.0-or-later
//
// A small GIF89a writer for the logo export. It needs no packages: the frames
// are RGBA already, the palette is built with a median cut, and the LZW coder
// is the one the format specifies. GIF has 1-bit transparency, so pixels below
// the alpha cutoff become the transparent index.

const ALPHA_CUTOFF = 128;
const TRANSPARENT = 255;
const MAX_COLORS = 255;

function histogram(frames) {
  const counts = new Map();
  for (const rgba of frames) {
    for (let i = 0; i < rgba.length; i += 4) {
      if (rgba[i + 3] < ALPHA_CUTOFF) continue;
      const key = (rgba[i] << 16) | (rgba[i + 1] << 8) | rgba[i + 2];
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
  }
  const colors = [];
  for (const [key, count] of counts) {
    colors.push({ r: key >> 16, g: (key >> 8) & 255, b: key & 255, count });
  }
  return colors;
}

function bounds(box) {
  let minR = 255, maxR = 0, minG = 255, maxG = 0, minB = 255, maxB = 0, count = 0;
  for (const c of box) {
    minR = Math.min(minR, c.r); maxR = Math.max(maxR, c.r);
    minG = Math.min(minG, c.g); maxG = Math.max(maxG, c.g);
    minB = Math.min(minB, c.b); maxB = Math.max(maxB, c.b);
    count += c.count;
  }
  return { minR, maxR, minG, maxG, minB, maxB, count };
}

function splitBox(box) {
  const b = bounds(box);
  const ranges = [b.maxR - b.minR, b.maxG - b.minG, b.maxB - b.minB];
  const channel = ranges[0] >= ranges[1] && ranges[0] >= ranges[2] ? "r" : ranges[1] >= ranges[2] ? "g" : "b";
  const sorted = box.slice().sort((x, y) => x[channel] - y[channel]);
  const half = b.count / 2;
  let acc = 0, cut = 0;
  while (cut < sorted.length - 1 && acc + sorted[cut].count <= half) acc += sorted[cut++].count;
  cut = Math.max(1, Math.min(sorted.length - 1, cut));
  return [sorted.slice(0, cut), sorted.slice(cut)];
}

/** Median cut down to at most maxColors boxes, each reduced to its weighted mean. */
function medianCut(colors, maxColors) {
  if (colors.length <= maxColors) return colors.map(({ r, g, b }) => ({ r, g, b }));
  let boxes = [colors];
  while (boxes.length < maxColors) {
    let pick = -1, score = -1;
    for (let i = 0; i < boxes.length; i++) {
      if (boxes[i].length < 2) continue;
      const b = bounds(boxes[i]);
      const range = Math.max(b.maxR - b.minR, b.maxG - b.minG, b.maxB - b.minB);
      // Prefer a wide box that also carries a lot of pixels.
      const s = range * Math.log2(b.count + 1);
      if (s > score) { score = s; pick = i; }
    }
    if (pick < 0) break;
    const [left, right] = splitBox(boxes[pick]);
    boxes.splice(pick, 1, left, right);
  }
  return boxes.map((box) => {
    let r = 0, g = 0, b = 0, n = 0;
    for (const c of box) {
      r += c.r * c.count; g += c.g * c.count; b += c.b * c.count; n += c.count;
    }
    return { r: Math.round(r / n), g: Math.round(g / n), b: Math.round(b / n) };
  });
}

function paletteFor(frames) {
  const colors = histogram(frames);
  const palette = medianCut(colors, MAX_COLORS);
  // Cache nearest-palette lookups per source color.
  const nearest = new Map();
  const indexOf = (r, g, b) => {
    const key = (r << 16) | (g << 8) | b;
    let index = nearest.get(key);
    if (index !== undefined) return index;
    let best = 0, dist = Infinity;
    for (let i = 0; i < palette.length; i++) {
      const p = palette[i];
      const d = (p.r - r) ** 2 + (p.g - g) ** 2 + (p.b - b) ** 2;
      if (d < dist) { dist = d; best = i; }
    }
    nearest.set(key, best);
    return best;
  };
  return { palette, indexOf };
}

/** LSB-first variable-width LZW, the code stream GIF uses. */
export function lzwEncode(indices, minCodeSize = 8) {
  const clear = 1 << minCodeSize, eoi = clear + 1;
  let codeSize = minCodeSize + 1, next = eoi + 1;
  const dict = new Map(), bytes = [];
  let acc = 0, bits = 0;
  const emit = (code) => {
    acc |= code << bits;
    bits += codeSize;
    while (bits >= 8) {
      bytes.push(acc & 255);
      acc >>>= 8;
      bits -= 8;
    }
  };
  emit(clear);
  let prefix = indices[0];
  for (let i = 1; i < indices.length; i++) {
    const k = indices[i], key = prefix * 4096 + k;
    const found = dict.get(key);
    if (found !== undefined) { prefix = found; continue; }
    emit(prefix);
    if (next < 4096) {
      dict.set(key, next++);
      if (next > 1 << codeSize && codeSize < 12) codeSize++;
    } else {
      emit(clear);
      dict.clear();
      codeSize = minCodeSize + 1;
      next = eoi + 1;
    }
    prefix = k;
  }
  emit(prefix);
  emit(eoi);
  if (bits > 0) bytes.push(acc & 255);
  return bytes;
}

function subBlocks(bytes) {
  const blocks = [];
  for (let i = 0; i < bytes.length; i += 255) {
    const chunk = bytes.slice(i, i + 255);
    blocks.push(Buffer.from([chunk.length, ...chunk]));
  }
  blocks.push(Buffer.from([0]));
  return blocks;
}

function upscaleIndices(indices, w, h, scale) {
  if (scale === 1) return indices;
  const out = new Uint8Array(w * scale * h * scale);
  for (let y = 0; y < h * scale; y++) {
    const sy = Math.floor(y / scale);
    for (let x = 0; x < w * scale; x++) out[y * w * scale + x] = indices[sy * w + Math.floor(x / scale)];
  }
  return out;
}

/**
 * Encode w x h RGBA frames as a looping GIF.
 * opts: { scale (nearest-neighbour), delay (1/100 s per frame) }.
 */
export function encodeGif(w, h, frames, { scale = 1, delay = 7 } = {}) {
  const { palette, indexOf } = paletteFor(frames);
  const W = w * scale, H = h * scale;
  const parts = [];

  const header = Buffer.alloc(13);
  header.write("GIF89a", 0, "ascii");
  header.writeUInt16LE(W, 6);
  header.writeUInt16LE(H, 8);
  header[10] = 0xf7; // global color table, 8-bit color, 256 entries
  header[11] = TRANSPARENT; // background
  header[12] = 0;
  parts.push(header);

  const table = Buffer.alloc(768);
  palette.forEach((c, i) => {
    table[i * 3] = c.r;
    table[i * 3 + 1] = c.g;
    table[i * 3 + 2] = c.b;
  });
  parts.push(table);

  // NETSCAPE2.0 looping extension (repeat forever).
  parts.push(Buffer.from([0x21, 0xff, 0x0b, ...Buffer.from("NETSCAPE2.0"), 0x03, 0x01, 0x00, 0x00, 0x00]));

  for (const rgba of frames) {
    const indices = new Uint8Array(w * h);
    for (let i = 0, p = 0; i < rgba.length; i += 4, p++) {
      indices[p] = rgba[i + 3] < ALPHA_CUTOFF ? TRANSPARENT : indexOf(rgba[i], rgba[i + 1], rgba[i + 2]);
    }
    const pixels = upscaleIndices(indices, w, h, scale);

    // Graphic control: dispose to background, transparent index, delay.
    parts.push(Buffer.from([0x21, 0xf9, 0x04, 0x09, delay & 255, (delay >> 8) & 255, TRANSPARENT, 0x00]));

    const descriptor = Buffer.alloc(10);
    descriptor[0] = 0x2c;
    descriptor.writeUInt16LE(0, 1);
    descriptor.writeUInt16LE(0, 3);
    descriptor.writeUInt16LE(W, 5);
    descriptor.writeUInt16LE(H, 7);
    descriptor[9] = 0x00;
    parts.push(descriptor, Buffer.from([8]), ...subBlocks(lzwEncode(pixels)));
  }

  parts.push(Buffer.from([0x3b]));
  return Buffer.concat(parts);
}
