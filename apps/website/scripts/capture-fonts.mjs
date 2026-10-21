// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Wait for the app's bundled fonts before a documentation screenshot.
 *
 * Inter is a variable font loaded with `font-display: swap`. A fixed timeout
 * can capture the fallback font on a cold cache, which changes text metrics
 * and even whether long labels truncate. Load the faces the shell uses and
 * wait for the font set, then let the browser lay out one more frame.
 */
const FACES = [
  '400 14px "Inter Variable"',
  '500 14px "Inter Variable"',
  '600 14px "Inter Variable"',
  '700 14px "Inter Variable"',
  '400 14px "JetBrains Mono Variable"',
  '500 14px "JetBrains Mono Variable"',
  '400 14px "Twemoji Country Flags"',
];

export async function waitForAppFonts(page) {
  await page.evaluate(async (faces) => {
    await document.fonts.ready;
    await Promise.all(faces.map((face) => document.fonts.load(face)));
    await document.fonts.ready;
    await new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(resolve)),
    );
  }, FACES);
}
