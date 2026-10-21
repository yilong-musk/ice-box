#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
/**
 * Compose the README product shot: the desktop window on the left with the
 * Android phone shell on the right, over a dark gradient backdrop.
 *
 * Reads the desktop and phone captures, so run the capture scripts first:
 *   bash scripts/capture-demo-home.sh
 *   bash scripts/capture-demo-mobile.sh
 *   (cd apps/website && npm run capture-hero)
 *
 * Writes:
 *   docs/images/hero.png            hero.zh-CN.png
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const CANVAS = { width: 2400, height: 1440 };

/** Composition in canvas CSS pixels; image heights follow the source aspect. */
const LAYOUT = {
  desktop: { left: 70, top: 136, width: 1620, radius: 22 },
  phone: { right: 48, bottom: 90, width: 620 },
};

const TARGETS = [
  {
    language: "en",
    desktop: "docs/images/home.png",
    mobile: "docs/images/mobile-home.png",
    output: "docs/images/hero.png",
  },
  {
    language: "zh",
    desktop: "docs/images/home.zh-CN.png",
    mobile: "docs/images/mobile-home.zh-CN.png",
    output: "docs/images/hero.zh-CN.png",
  },
];

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const websiteDir = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(websiteDir, "../..");

function dataUri(relativePath) {
  const file = path.join(repoRoot, relativePath);
  return `data:image/png;base64,${fs.readFileSync(file).toString("base64")}`;
}

/** Intrinsic pixel size of a PNG from its IHDR chunk. */
function pngSize(relativePath) {
  const buffer = fs.readFileSync(path.join(repoRoot, relativePath));
  return { width: buffer.readUInt32BE(16), height: buffer.readUInt32BE(20) };
}

function composeHtml(target) {
  const desktopSize = pngSize(target.desktop);
  const phoneSize = pngSize(target.mobile);
  const desktopHeight = (LAYOUT.desktop.width * desktopSize.height) / desktopSize.width;
  const phoneHeight = (LAYOUT.phone.width * phoneSize.height) / phoneSize.width;
  const desktop = dataUri(target.desktop);
  const phone = dataUri(target.mobile);

  return `<!doctype html>
<html lang="${target.language}">
  <head>
    <meta charset="UTF-8" />
    <style>
      * { box-sizing: border-box; }
      html, body { margin: 0; padding: 0; }
      body {
        position: relative;
        width: ${CANVAS.width}px;
        height: ${CANVAS.height}px;
        overflow: hidden;
        background:
          radial-gradient(1600px 1200px at 16% 6%, rgba(44, 104, 102, 0.7), transparent 60%),
          radial-gradient(1400px 1200px at 88% 10%, rgba(58, 64, 128, 0.65), transparent 62%),
          radial-gradient(1200px 1000px at 74% 96%, rgba(104, 50, 84, 0.6), transparent 60%),
          linear-gradient(150deg, #0a1117 0%, #0f1a22 52%, #080b0f 100%);
      }
      .desktop {
        position: absolute;
        left: ${LAYOUT.desktop.left}px;
        top: ${LAYOUT.desktop.top}px;
        width: ${LAYOUT.desktop.width}px;
        border-radius: ${LAYOUT.desktop.radius}px;
        overflow: hidden;
        background: #131416;
        box-shadow:
          0 0 0 1px rgba(255, 255, 255, 0.08),
          0 48px 90px rgba(0, 0, 0, 0.58),
          0 12px 30px rgba(0, 0, 0, 0.42);
      }
      .desktop img {
        display: block;
        width: 100%;
        height: ${desktopHeight}px;
      }
      .phone {
        position: absolute;
        bottom: ${LAYOUT.phone.bottom}px;
        right: ${LAYOUT.phone.right}px;
        width: ${LAYOUT.phone.width}px;
        filter: drop-shadow(0 42px 60px rgba(0, 0, 0, 0.55));
      }
      .phone img {
        display: block;
        width: 100%;
        height: ${phoneHeight}px;
      }
    </style>
  </head>
  <body>
    <div class="desktop"><img src="${desktop}" alt="" /></div>
    <div class="phone"><img src="${phone}" alt="" /></div>
  </body>
</html>`;
}

if (!fs.existsSync(path.join(websiteDir, "node_modules/playwright"))) {
  console.error(
    "compose-hero: install website deps first:\n" +
      "  (cd apps/website && npm ci && npx playwright install chromium)",
  );
  process.exit(1);
}

const missingSources = TARGETS.flatMap((target) => [target.desktop, target.mobile]).filter(
  (relativePath) => !fs.existsSync(path.join(repoRoot, relativePath)),
);
if (missingSources.length > 0) {
  console.error(
    "compose-hero: missing source screenshots:\n" +
      missingSources.map((file) => `  ${file}`).join("\n") +
      "\nRun the capture scripts first:\n" +
      "  bash scripts/capture-demo-home.sh\n" +
      "  bash scripts/capture-demo-mobile.sh",
  );
  process.exit(1);
}

const browser = await chromium.launch({ headless: true });
try {
  const page = await browser.newPage({ viewport: CANVAS, deviceScaleFactor: 1 });
  for (const target of TARGETS) {
    await page.setContent(composeHtml(target), { waitUntil: "load" });
    await page.evaluate(async () => {
      await document.fonts.ready;
      await Promise.all(
        Array.from(document.images, (image) =>
          image.complete ? Promise.resolve() : image.decode(),
        ),
      );
    });
    const png = await page.screenshot({ type: "png" });
    const outFile = path.join(repoRoot, target.output);
    fs.mkdirSync(path.dirname(outFile), { recursive: true });
    fs.writeFileSync(outFile, png);
    console.log(`Wrote ${target.output}`);
  }
} finally {
  await browser.close();
}
