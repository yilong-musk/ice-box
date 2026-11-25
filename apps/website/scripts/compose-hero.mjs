#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
/**
 * Screenshot the landing-page showcase for the README product shot.
 *
 * The shot is the website itself: the app window, the phone frame, and the
 * page background, with the Live Demos loaded inside in capture mode so
 * traffic, clocks, and timestamps stay reproducible. It does not restage the
 * captures in a separate frame.
 *
 *   bash scripts/capture-docs.sh
 *   (cd apps/website && npm run capture-hero)
 *
 * Writes:
 *   docs/images/hero.png            hero.zh-CN.png
 */
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const PREVIEW_PORT = 4179;
const VIEWPORT = { width: 1440, height: 1400 };
/** Page background kept around the showcase in the shot. */
const SHOT_PAD = 28;

/**
 * Shot-only page styles, injected after the page has loaded. The sticky nav
 * is hidden so it cannot overlap the showcase once that is scrolled to the
 * top, and the showcase is lifted above the wallpaper (SHOT_WALLPAPER).
 */
const SHOT_STYLE = `
  .site-nav { visibility: hidden !important; }
  .showcase { z-index: 2; }
`;

/**
 * Wallpaper behind the windows in the shot, like a desktop background. The
 * dark landing page sits only a few lightness steps under the app's dark
 * cards, so in the README the windows melted into it; over soft colour the
 * dark app and phone stand off clearly. Shot only: the live site keeps its
 * own background. Laid on an element sized to the shot, so the whole
 * gradient is in frame (on <body> it would stretch over the full page).
 */
const SHOT_WALLPAPER = [
  "radial-gradient(60% 75% at 8% 0%, oklch(0.66 0.13 250 / 92%), transparent 70%)",
  "radial-gradient(55% 70% at 95% 5%, oklch(0.64 0.14 312 / 88%), transparent 70%)",
  "radial-gradient(65% 80% at 100% 100%, oklch(0.73 0.115 52 / 88%), transparent 70%)",
  "radial-gradient(60% 75% at 0% 100%, oklch(0.68 0.1 198 / 88%), transparent 70%)",
  "linear-gradient(135deg, oklch(0.54 0.115 268), oklch(0.58 0.12 328))",
].join(", ");

const TARGETS = [
  {
    language: "en",
    locale: "en-US",
    desktopLabel: "Stop System Proxy",
    phoneLabel: "Disconnect VPN",
    output: "docs/images/hero.png",
  },
  {
    language: "zh",
    locale: "zh-CN",
    desktopLabel: "关闭系统代理",
    phoneLabel: "断开 VPN",
    output: "docs/images/hero.zh-CN.png",
  },
];

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const websiteDir = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(websiteDir, "../..");

function waitForOutput(child, pattern, timeoutMs) {
  return new Promise((resolve, reject) => {
    let settled = false;
    const finish = (fn) => (arg) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      fn(arg);
    };
    const timer = setTimeout(
      () => finish(reject)(new Error(`Timed out waiting for preview server (${timeoutMs}ms)`)),
      timeoutMs,
    );
    const onData = (chunk) => {
      const text = String(chunk);
      process.stderr.write(text);
      if (pattern.test(text)) finish(resolve)();
    };
    child.stdout?.on("data", onData);
    child.stderr?.on("data", onData);
    child.once("exit", (code) => {
      finish(reject)(new Error(`Preview server exited early with code ${code}`));
    });
  });
}

function viteBin() {
  return path.join(websiteDir, "node_modules", "vite", "bin", "vite.js");
}

async function buildSite() {
  await new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [viteBin(), "build"], {
      cwd: websiteDir,
      stdio: "inherit",
    });
    child.once("exit", (code) => {
      if (code === 0) resolve();
      else reject(new Error(`Website build failed with code ${code}`));
    });
  });
}

/**
 * The landing page embeds the live demos. README shots need the frozen mock
 * (`?capture=1` in browser-api.ts), which is read from each iframe's own
 * location, so reload them before waiting on their contents.
 */
async function useCaptureDemos(page) {
  await page.evaluate(() => {
    for (const iframe of document.querySelectorAll(".showcase iframe")) {
      const url = new URL(iframe.getAttribute("src"), window.location.href);
      url.searchParams.set("capture", "1");
      if (iframe.src !== url.href) iframe.src = url.href;
    }
  });
}

/**
 * Wait until the capture-mode document is attached. A one-shot contentFrame()
 * is null while a lazy iframe has not started, and it can still point at the
 * pre-capture document for a beat after src changes.
 */
async function demoFrame(page, selector) {
  const iframe = page.locator(selector);
  await iframe.waitFor({ state: "attached", timeout: 30_000 });
  await page.waitForFunction(
    (sel) => {
      const el = document.querySelector(sel);
      if (!(el instanceof HTMLIFrameElement)) return false;
      try {
        const win = el.contentWindow;
        const doc = el.contentDocument;
        if (!win || !doc || doc.readyState !== "complete") return false;
        return new URL(win.location.href).searchParams.get("capture") === "1";
      } catch {
        return false;
      }
    },
    selector,
    { timeout: 30_000 },
  );
  const frame = await (await iframe.elementHandle())?.contentFrame();
  if (!frame) throw new Error(`No frame for ${selector}`);
  return frame;
}

/**
 * The demo reconciles a stored language with its mocked settings file, which
 * can put English back. Re-apply through the app's own language event until
 * the power button shows the target label.
 */
async function waitForLanguage(frame, target, label) {
  await frame.waitForFunction(
    ({ language, label }) => {
      const shown = Array.from(document.querySelectorAll("button")).some(
        (button) => button.getAttribute("aria-label") === label || button.textContent?.trim() === label,
      );
      if (shown) return true;
      window.localStorage.setItem("ice-box.language", language);
      window.dispatchEvent(new CustomEvent("ice-box-language", { detail: language }));
      return false;
    },
    { language: target.language, label },
    { timeout: 30_000, polling: 100 },
  );
}

async function waitForFonts(page, frames) {
  await page.evaluate(() => document.fonts.ready);
  for (const frame of frames) {
    await frame.evaluate(() => document.fonts.ready);
  }
}

async function waitForShowcase(page, target) {
  await page.locator(".showcase").scrollIntoViewIfNeeded();
  await useCaptureDemos(page);
  await page.waitForFunction(() =>
    [...document.querySelectorAll(".showcase .reveal")].every((el) =>
      el.classList.contains("is-visible"),
    ),
  );

  const desktop = await demoFrame(page, ".demo-screen iframe");
  const phone = await demoFrame(page, ".phone-viewport iframe");
  await waitForLanguage(desktop, target, target.desktopLabel);
  await waitForLanguage(phone, target, target.phoneLabel);

  const landingIp = (
    await desktop.getByTestId("home-outbound-ip").textContent({ timeout: 15_000 })
  )?.trim();
  if (!landingIp || !/^(?:\S+\s+)?\d{1,3}(?:\.\d{1,3}){3}$/.test(landingIp)) {
    throw new Error(`Desktop demo landing IP was not ready, got ${JSON.stringify(landingIp)}`);
  }
  const node = (
    await phone.getByTestId("home-current-node").textContent({ timeout: 15_000 })
  )?.trim();
  if (!node || node === "—") {
    throw new Error(`Phone demo node was not ready, got ${JSON.stringify(node)}`);
  }

  // Settings reconciliation is one-shot and can land after the first matching
  // paint. Check again once the home data is up, then shoot.
  await waitForLanguage(desktop, target, target.desktopLabel);
  await waitForLanguage(phone, target, target.phoneLabel);
  await waitForFonts(page, [desktop, phone]);
  await page.locator(".phone-clock").evaluate((el) => {
    el.textContent = "12:00";
  });
}

/**
 * The area to shoot: the showcase plus the phone frame (which can reach past
 * the showcase box), with SHOT_PAD of page around it. The area is scrolled to
 * the top of the viewport first; a clip used to be cut off at the viewport's
 * bottom edge, which cropped the phone. Fails rather than crop.
 */
async function shotClip(page) {
  const area = () =>
    page.evaluate(() => {
      const boxes = [".showcase", ".phone-frame"]
        .map((sel) => document.querySelector(sel)?.getBoundingClientRect())
        .filter((box) => box && box.width > 0 && box.height > 0);
      if (boxes.length === 0) return null;
      const left = Math.min(...boxes.map((b) => b.left));
      const top = Math.min(...boxes.map((b) => b.top));
      const right = Math.max(...boxes.map((b) => b.right));
      const bottom = Math.max(...boxes.map((b) => b.bottom));
      return { left, top, right, bottom, scrollY: window.scrollY };
    });
  const before = await area();
  if (!before) throw new Error("Showcase is not visible");
  await page.evaluate((y) => window.scrollTo(0, y), before.scrollY + before.top - SHOT_PAD);
  const box = await area();
  const clip = {
    x: Math.max(0, box.left - SHOT_PAD),
    y: Math.max(0, box.top - SHOT_PAD),
    width: box.right - box.left + SHOT_PAD * 2,
    height: box.bottom - box.top + SHOT_PAD * 2,
  };
  if (clip.x + clip.width > VIEWPORT.width || clip.y + clip.height > VIEWPORT.height) {
    throw new Error(`Shot area ${JSON.stringify(clip)} does not fit the ${VIEWPORT.width}x${VIEWPORT.height} viewport`);
  }
  await page.evaluate(
    ({ clip, wallpaper }) => {
      const el = document.createElement("div");
      el.setAttribute("aria-hidden", "true");
      Object.assign(el.style, {
        position: "fixed",
        left: `${clip.x}px`,
        top: `${clip.y}px`,
        width: `${clip.width}px`,
        height: `${clip.height}px`,
        zIndex: "1",
        pointerEvents: "none",
        background: wallpaper,
      });
      document.body.append(el);
    },
    { clip, wallpaper: SHOT_WALLPAPER },
  );
  return clip;
}

if (!fs.existsSync(path.join(websiteDir, "node_modules/playwright"))) {
  console.error(
    "compose-hero: install website deps first:\n" +
      "  (cd apps/website && npm ci && npx playwright install chromium)",
  );
  process.exit(1);
}

await buildSite();
const preview = spawn(
  process.execPath,
  [viteBin(), "preview", "--host", "127.0.0.1", "--port", String(PREVIEW_PORT), "--strictPort"],
  { cwd: websiteDir, stdio: ["ignore", "pipe", "pipe"] },
);

try {
  await waitForOutput(preview, /Local:/, 30_000);
  const browser = await chromium.launch({ headless: true });
  try {
    for (const target of TARGETS) {
      const context = await browser.newContext({
        viewport: VIEWPORT,
        deviceScaleFactor: 2,
        locale: target.locale,
        colorScheme: "dark",
        // Frozen capture timestamps render in this zone, so the axis does not
        // follow the machine clock. Reduced motion stops the background motes
        // and the reveal fade, which would otherwise move between runs.
        timezoneId: "UTC",
        reducedMotion: "reduce",
      });
      try {
        await context.addInitScript((language) => {
          window.localStorage.setItem("ice-box.theme", "dark");
          window.localStorage.setItem("ice-box.language", language);
          // The memory bar transitions its width for 500ms. A shot taken during
          // that glide lands on a different edge every run. Kill transitions
          // before first paint so the bar is at its final width.
          const style = document.createElement("style");
          style.textContent = "*, *::before, *::after { transition: none !important; }";
          document.documentElement.appendChild(style);
        }, target.language);
        const page = await context.newPage();
        await page.goto(`http://127.0.0.1:${PREVIEW_PORT}/`, {
          waitUntil: "domcontentloaded",
          timeout: 60_000,
        });
        await waitForShowcase(page, target);
        await page.addStyleTag({ content: SHOT_STYLE });
        const clip = await shotClip(page);
        const png = await page.screenshot({ type: "png", clip });
        const outFile = path.join(repoRoot, target.output);
        fs.mkdirSync(path.dirname(outFile), { recursive: true });
        fs.writeFileSync(outFile, png);
        console.log(`Wrote ${target.output}`);
      } finally {
        await context.close();
      }
    }
  } finally {
    await browser.close();
  }
} finally {
  preview.kill("SIGTERM");
}
