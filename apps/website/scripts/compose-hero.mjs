#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
/**
 * Screenshot the landing-page showcase for the README product shot.
 *
 * The shot is the website itself: the pixel laptop, the pixel phone, and the
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
const VIEWPORT = { width: 1440, height: 1200 };

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
  if (!landingIp || !/^\d{1,3}(?:\.\d{1,3}){3}$/.test(landingIp)) {
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
        const box = await page.locator(".showcase").boundingBox();
        if (!box) throw new Error("Showcase is not visible");
        const pad = 28;
        const clip = {
          x: Math.max(0, box.x - pad),
          y: Math.max(0, box.y - pad),
          width: Math.min(VIEWPORT.width - Math.max(0, box.x - pad), box.width + pad * 2),
          height: Math.min(VIEWPORT.height - Math.max(0, box.y - pad), box.height + pad * 2),
        };
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
