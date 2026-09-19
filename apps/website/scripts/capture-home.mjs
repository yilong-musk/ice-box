#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
/**
 * Capture the Live Demo Home view into docs/images/home.png (en) and
 * docs/images/home.zh-CN.png (zh), without the marketing window bar.
 * Matches the GitHub Pages iframe (1180x690).
 *
 * Usage:
 *   bash scripts/capture-demo-home.sh
 *   CAPTURE_DEMO_HOME_FORCE=1 bash scripts/capture-demo-home.sh
 */
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const VIEWPORT = { width: 1180, height: 690 };
const SCALE = 2;
const PREVIEW_PORT = 4175;

/** One capture per README language; the parameters are identical otherwise. */
const TARGETS = [
  {
    language: "en",
    locale: "en-US",
    // Power button label (`home.power.stop`) proving the UI language.
    stopLabel: "Stop Proxy Service",
    screenshot: "docs/images/home.png",
  },
  {
    language: "zh",
    locale: "zh-CN",
    stopLabel: "停止代理服务",
    screenshot: "docs/images/home.zh-CN.png",
  },
];

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const websiteDir = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(websiteDir, "../..");
const versionFile = path.join(repoRoot, "docs/images/home.version");
const desktopPackageJson = path.join(repoRoot, "apps/desktop/package.json");

function screenshotPath(target) {
  return path.join(repoRoot, target.screenshot);
}

function appVersion() {
  return JSON.parse(fs.readFileSync(desktopPackageJson, "utf8")).version;
}

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

function startPreview() {
  const viteBin = path.join(websiteDir, "node_modules/.bin/vite");
  return spawn(
    viteBin,
    ["preview", "--host", "127.0.0.1", "--port", String(PREVIEW_PORT), "--strictPort"],
    { cwd: websiteDir, stdio: ["ignore", "pipe", "pipe"] },
  );
}

async function ensureBuild() {
  const viteBin = path.join(websiteDir, "node_modules/.bin/vite");
  await new Promise((resolve, reject) => {
    const child = spawn(viteBin, ["build"], {
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
 * Wait until the power button shows the target language.
 *
 * `addInitScript` stores the preference before boot, but the demo reconciles
 * it with the mocked settings file (`language: "en"` in browser-api.ts) right
 * after the app mounts, which overwrites a stored "zh". Re-apply the target
 * through the app's own language event (`LANGUAGE_CHANGE_EVENT` in
 * apps/desktop/src/lib/i18n.tsx) on every poll: the listener mounts with the
 * app shell, so an early dispatch is simply repeated until it sticks.
 */
async function waitForLanguage(page, target) {
  await page.waitForFunction(
    ({ language, label }) => {
      const shown = Array.from(document.querySelectorAll("button")).some(
        (button) => button.getAttribute("aria-label") === label,
      );
      if (shown) return true;
      window.localStorage.setItem("ice-box.language", language);
      window.dispatchEvent(new CustomEvent("ice-box-language", { detail: language }));
      return false;
    },
    { language: target.language, label: target.stopLabel },
    { timeout: 30_000, polling: 100 },
  );
}

async function capture(browser, baseUrl, target) {
  const context = await browser.newContext({
    viewport: VIEWPORT,
    deviceScaleFactor: SCALE,
    colorScheme: "dark",
    locale: target.locale,
    timezoneId: "UTC",
  });
  try {
    await context.addInitScript((language) => {
      window.localStorage.setItem("ice-box.theme", "dark");
      window.localStorage.setItem("ice-box.language", language);
    }, target.language);
    const page = await context.newPage();
    await page.goto(`${baseUrl}/demo.html?capture=1`, { waitUntil: "domcontentloaded", timeout: 60_000 });
    // Mount signal: the shell renders the power button in either language.
    await page
      .getByRole("button", { name: /Stop Proxy Service|停止代理服务/ })
      .waitFor({ timeout: 30_000 });
    await waitForLanguage(page, target);
    await page.evaluate(() => window.dispatchEvent(new Event("resize")));
    await page.waitForFunction(() => {
      const el = document.querySelector(".recharts-wrapper");
      return el && el.getBoundingClientRect().height >= 240;
    }, { timeout: 15_000 });
    await new Promise((resolve) => setTimeout(resolve, 400));
    return await page.screenshot({ type: "png" });
  } finally {
    await context.close();
  }
}

await ensureBuild();
const preview = startPreview();
try {
  await waitForOutput(preview, /Local:/, 30_000);
  const baseUrl = `http://127.0.0.1:${PREVIEW_PORT}`;
  const browser = await chromium.launch({ headless: true });
  try {
    // Capture both languages first and only then write the files, so a
    // failure in either capture leaves the previous set untouched.
    const shots = [];
    for (const target of TARGETS) {
      try {
        shots.push({ target, png: await capture(browser, baseUrl, target) });
      } catch (error) {
        throw new Error(
          `[${target.language}] ${target.screenshot}: ${error instanceof Error ? error.message : error}`,
          { cause: error },
        );
      }
    }
    const version = appVersion();
    for (const { target, png } of shots) {
      const outFile = screenshotPath(target);
      fs.mkdirSync(path.dirname(outFile), { recursive: true });
      fs.writeFileSync(outFile, png);
    }
    fs.writeFileSync(versionFile, `${version}\n`);
    for (const { target } of shots) {
      console.log(`Wrote ${target.screenshot} (${version})`);
    }
  } finally {
    await browser.close();
  }
} finally {
  preview.kill("SIGTERM");
}
