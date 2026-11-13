#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
/**
 * Screenshot the landing-page showcase for the README product shot.
 *
 * The shot is the website itself: the pixel laptop, the pixel phone, and the
 * page background, with the Live Demos loaded inside. It does not restage the
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
 * The demo reconciles a stored language with its mocked settings file, which
 * can put English back. Re-apply through the app's own language event until
 * the power button shows the target label.
 */
async function demoFrame(page, selector) {
  const handle = await page.locator(selector).elementHandle();
  const frame = await handle?.contentFrame();
  if (!frame) throw new Error(`No frame for ${selector}`);
  return frame;
}

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

async function waitForShowcase(page, target) {
  await page.locator(".showcase").scrollIntoViewIfNeeded();
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

  // Charts settle a beat after the numbers. Freeze the status clock so the
  // shot does not depend on the wall time.
  await page.waitForTimeout(600);
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
      });
      try {
        await context.addInitScript((language) => {
          window.localStorage.setItem("ice-box.theme", "dark");
          window.localStorage.setItem("ice-box.language", language);
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
