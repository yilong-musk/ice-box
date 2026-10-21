#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
/**
 * Smoke test for the mobile Live Demo: the phone-first UI embedded on the
 * landing page. Builds are assumed (`npm run build`); the script serves
 * `dist` with `vite preview` and drives Chromium through Playwright.
 *
 * Checks:
 *   - the landing page loads `mobile-demo.html` in the phone frame
 *   - the phone shell renders all five tabs
 *   - the Apps fixtures load and the VPN power button toggles
 *   - the site language toggle reaches the iframe (same-origin storage bridge)
 *
 * Usage:
 *   (cd apps/website && npm run build && npm run test:mobile-demo)
 */
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const websiteDir = path.resolve(scriptDir, "..");
const distDir = path.join(websiteDir, "dist");
const PORT = 4178;
const BASE_URL = `http://127.0.0.1:${PORT}`;

function fail(message) {
  console.error(`smoke-mobile-demo: ${message}`);
  process.exit(1);
}

if (!fs.existsSync(path.join(distDir, "mobile-demo.html"))) {
  fail("missing dist/mobile-demo.html. Run `npm run build` first.");
}

function startPreview() {
  const vite = path.join(websiteDir, "node_modules/.bin/vite");
  return spawn(
    vite,
    ["preview", "--host", "127.0.0.1", "--port", String(PORT), "--strictPort"],
    { cwd: websiteDir, stdio: ["ignore", "pipe", "pipe"] },
  );
}

async function waitForServer(timeoutMs = 20_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${BASE_URL}/`);
      if (response.ok) return;
    } catch {
      // Server not up yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`preview server did not start within ${timeoutMs}ms`);
}

async function waitForText(locator, expected, timeoutMs = 8000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if ((await locator.textContent())?.trim() === expected) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  const actual = (await locator.textContent())?.trim();
  throw new Error(`expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
}

const preview = startPreview();
preview.stderr.on("data", (chunk) => process.stderr.write(chunk));

let browser;
try {
  await waitForServer();
  browser = await chromium.launch();
  const context = await browser.newContext({
    viewport: { width: 1280, height: 1000 },
    colorScheme: "dark",
    locale: "en-US",
  });
  await context.addInitScript(() => {
    window.localStorage.setItem("ice-box.theme", "dark");
    window.localStorage.setItem("ice-box.language", "en");
  });
  const page = await context.newPage();
  const errors = [];
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  page.on("pageerror", (error) => errors.push(error.message));

  await page.goto(`${BASE_URL}/`, { waitUntil: "domcontentloaded" });
  await page.locator("#android-demo").scrollIntoViewIfNeeded();

  const frame = page.frameLocator(".phone-viewport iframe");
  await frame.locator('[data-testid="phone-shell"]').waitFor({ timeout: 30_000 });
  if ((await frame.locator("html").getAttribute("data-shell")) !== "phone") {
    throw new Error("mobile-demo.html did not select the phone shell");
  }

  const tabs = frame.getByTestId("phone-tab-bar").getByRole("button");
  if ((await tabs.count()) !== 5) {
    throw new Error(`phone tab bar must show 5 tabs, got ${await tabs.count()}`);
  }

  await tabs.nth(1).click();
  await frame
    .locator('[data-testid="apps-panel"] [role="listitem"]')
    .first()
    .waitFor({ timeout: 15_000 });
  const apps = await frame
    .locator('[data-testid="apps-panel"] [role="listitem"]')
    .count();
  if (apps !== 8) throw new Error(`apps fixture must show 8 rows, got ${apps}`);

  await tabs.nth(0).click();
  const power = frame.locator('[data-testid="home-panel"] button[aria-pressed]');
  const before = (await power.textContent())?.trim();
  const after = before === "Disconnect VPN" ? "Connect VPN" : "Disconnect VPN";
  await power.click();
  await waitForText(power, after);

  await page.locator(".nav-language").click();
  await frame.getByRole("button", { name: "应用", exact: true }).waitFor({
    timeout: 10_000,
  });

  if (errors.length > 0) {
    throw new Error(`browser console errors:\n${errors.join("\n")}`);
  }
  console.log("smoke-mobile-demo: OK");
} catch (error) {
  process.exitCode = 1;
  console.error(
    `smoke-mobile-demo: ${error instanceof Error ? error.message : String(error)}`,
  );
} finally {
  await browser?.close();
  preview.kill("SIGTERM");
}
