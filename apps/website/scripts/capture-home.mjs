#!/usr/bin/env node
/**
 * Capture the Live Demo Home view into docs/images/home.png.
 * Matches the GitHub Pages iframe (1180x690), without the marketing window bar.
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

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const websiteDir = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(websiteDir, "../..");
const outFile = path.join(repoRoot, "docs/images/home.png");

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

async function capture(baseUrl) {
  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({
    viewport: VIEWPORT,
    deviceScaleFactor: SCALE,
    colorScheme: "dark",
    locale: "en-US",
    timezoneId: "UTC",
  });
  await context.addInitScript(() => {
    window.localStorage.setItem("ice-box.theme", "dark");
    window.localStorage.setItem("ice-box.language", "en");
  });
  const page = await context.newPage();
  await page.goto(`${baseUrl}/demo.html?capture=1`, { waitUntil: "domcontentloaded", timeout: 60_000 });
  await page.getByRole("button", { name: "Stop Proxy Service" }).waitFor({ timeout: 30_000 });
  await page.evaluate(() => window.dispatchEvent(new Event("resize")));
  await page.waitForFunction(() => {
    const el = document.querySelector(".recharts-wrapper");
    return el && el.getBoundingClientRect().height >= 240;
  }, { timeout: 15_000 });
  await new Promise((resolve) => setTimeout(resolve, 400));
  fs.mkdirSync(path.dirname(outFile), { recursive: true });
  await page.screenshot({ path: outFile, type: "png" });
  await browser.close();
}

await ensureBuild();
const preview = startPreview();
try {
  await waitForOutput(preview, /Local:/, 30_000);
  await capture(`http://127.0.0.1:${PREVIEW_PORT}`);
  console.log(`Wrote ${path.relative(repoRoot, outFile)}`);
} finally {
  preview.kill("SIGTERM");
}
