#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
/**
 * Smoke test for the mobile Live Demo: the phone-first UI embedded on the
 * landing page. Builds are assumed (`npm run build`); the script serves
 * `dist` with `vite preview` and drives Chromium through Playwright.
 *
 * Checks:
 *   - the landing page loads `mobile-demo.html` in the phone frame
 *   - the phone node list does not flash "service not running"
 *   - the phone shell renders all five tabs
 *   - every demo node carries a country flag
 *   - the Apps fixtures load, every row has an icon, and per-app routing
 *     survives a tab round trip
 *   - switching subscriptions replaces the node list
 *   - the VPN power button toggles and the node list follows it
 *   - capture mode keeps the subscription expiry in the future
 *   - language and theme changes on the site reach the iframe
 *   - the download platform rail supports click/keyboard navigation without
 *     intercepting modified shortcuts, keeps desktop panel height and note-box
 *     alignment consistent, preserves state across language switches, and
 *     refines Linux architecture without overriding manual selection
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
  // Resolve Vite's JS entry and run it with the current Node binary. The
  // `node_modules/.bin/vite` shim is a `.cmd` file on Windows, which `spawn`
  // cannot execute without a shell.
  const viteBin = path.join(websiteDir, "node_modules", "vite", "bin", "vite.js");
  return spawn(
    process.execPath,
    [viteBin, "preview", "--host", "127.0.0.1", "--port", String(PORT), "--strictPort"],
    { cwd: websiteDir, stdio: ["ignore", "pipe", "pipe"] },
  );
}

async function waitForServer(preview, timeoutMs = 20_000) {
  let exit = null;
  preview.once("exit", (code, signal) => {
    exit = { code, signal };
  });
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (exit) {
      throw new Error(
        `preview server exited (code ${exit.code}, signal ${exit.signal}); is port ${PORT} in use?`,
      );
    }
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

async function waitForText(locator, expected, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if ((await locator.textContent())?.trim() === expected) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  const actual = (await locator.textContent())?.trim();
  if (actual === expected) return;
  throw new Error(`expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
}

async function waitForTheme(frame, dark, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const isDark = await frame
      .locator("html")
      .evaluate((el) => el.classList.contains("dark"));
    if (isDark === dark) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  const isDark = await frame
    .locator("html")
    .evaluate((el) => el.classList.contains("dark"));
  if (isDark === dark) return;
  throw new Error(
    `phone iframe theme did not resolve to ${dark ? "dark" : "light"}`,
  );
}

async function waitForAttribute(locator, name, expected, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if ((await locator.getAttribute(name)) === expected) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  if ((await locator.getAttribute(name)) === expected) return;
  throw new Error(`${name} did not become ${expected}`);
}

const DOWNLOAD_PLATFORMS = ["macos", "windows", "linux", "android"];

/** Measure desktop download panel height, note-box clearance, and bottom edge across all 4 tabs. */
async function assertUniformDesktopDownloadPanel(page, label) {
  const metrics = [];
  for (const platform of DOWNLOAD_PLATFORMS) {
    await page.locator(`[data-download-tab="${platform}"]`).click();
    const m = await page.evaluate(async (plat) => {
      const panel = document.querySelector(".download-panel");
      const pane = document.querySelector(`[data-download-panel="${plat}"]`);
      // Wait for the pane's translateY entrance animation to settle before reading rects.
      await Promise.all(
        pane.getAnimations().map((animation) => animation.finished.catch(() => {})),
      );
      const noteBox = pane.querySelector(".download-note-box");
      const above =
        plat === "linux"
          ? pane.querySelector(".download-files")
          : pane.querySelector(".download-action-row");
      const panelRect = panel.getBoundingClientRect();
      const noteRect = noteBox.getBoundingClientRect();
      const aboveRect = above.getBoundingClientRect();
      return {
        platform: plat,
        panelHeight: panelRect.height,
        noteGap: noteRect.top - aboveRect.bottom,
        noteBottomOffset: panelRect.bottom - noteRect.bottom,
      };
    }, platform);
    if (m.noteGap < 17) {
      throw new Error(
        `${label}: ${platform} note-box gap collapsed to ${m.noteGap.toFixed(1)}px`,
      );
    }
    metrics.push(m);
  }
  const heights = metrics.map((m) => m.panelHeight);
  if (Math.max(...heights) - Math.min(...heights) > 0.5) {
    throw new Error(
      `${label}: desktop download panel heights diverged: ${JSON.stringify(metrics)}`,
    );
  }
  const bottomOffsets = metrics.map((m) => m.noteBottomOffset);
  if (Math.max(...bottomOffsets) - Math.min(...bottomOffsets) > 0.5) {
    throw new Error(
      `${label}: desktop note-box bottom alignment diverged: ${JSON.stringify(metrics)}`,
    );
  }
}

async function verifyDownloadDesktopAndMobile(browser) {
  // Just above the 960px breakpoint: still 2-column desktop, vertical rail, uniform height.
  const narrowDesktop = await browser.newContext({
    viewport: { width: 970, height: 900 },
    locale: "en-US",
    userAgent:
      "Mozilla/5.0 (Windows NT 10.0; Win64; ARM64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
  });
  try {
    await narrowDesktop.addInitScript(() => {
      window.localStorage.setItem("ice-box.language", "en");
    });
    const p = await narrowDesktop.newPage();
    await p.goto(`${BASE_URL}/#download`, { waitUntil: "domcontentloaded" });
    await p.evaluate(() => document.fonts.ready);
    await waitForAttribute(p.locator(".download-rail"), "aria-orientation", "vertical");
    // Non-Linux ARM64 UA still defaults the Linux tab to x64.
    await p.locator('[data-download-tab="linux"]').click();
    await waitForAttribute(p.locator('[data-linux-arch="amd64"]'), "aria-pressed", "true");
    await assertUniformDesktopDownloadPanel(p, "970px en");
    await p.setViewportSize({ width: 390, height: 844 });
    await waitForAttribute(p.locator(".download-rail"), "aria-orientation", "horizontal");
  } finally {
    await narrowDesktop.close();
  }

  // Linux frozen x86_64 UA refined to arm64 via Client Hints: auto-updates when untouched.
  const frozenLinuxUa =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
  const autoRefineCtx = await browser.newContext({
    viewport: { width: 1280, height: 900 },
    locale: "en-US",
    userAgent: frozenLinuxUa,
  });
  try {
    await autoRefineCtx.addInitScript(() => {
      window.localStorage.setItem("ice-box.language", "en");
      Object.defineProperty(navigator, "userAgentData", {
        configurable: true,
        value: {
          getHighEntropyValues: async () => ({ architecture: "arm" }),
        },
      });
    });
    const p = await autoRefineCtx.newPage();
    await p.goto(`${BASE_URL}/#download`, { waitUntil: "domcontentloaded" });
    await waitForAttribute(p.locator('[data-linux-arch="arm64"]'), "aria-pressed", "true");
    const heroHref = await p.locator(".hero-download").getAttribute("href");
    const appimageHref = await p.locator('[data-linux-format="appimage"]').getAttribute("href");
    const debHref = await p.locator('[data-linux-format="deb"]').getAttribute("href");
    if (!heroHref?.endsWith("_aarch64.AppImage") || !appimageHref?.endsWith("_aarch64.AppImage") || !debHref?.endsWith("_arm64.deb")) {
      throw new Error(
        `refineLinuxArch auto-update failed: ${JSON.stringify({ heroHref, appimageHref, debHref })}`,
      );
    }
  } finally {
    await autoRefineCtx.close();
  }

  // Deferred Client Hints resolution must not override an explicit user selection.
  const manualArchCtx = await browser.newContext({
    viewport: { width: 1280, height: 900 },
    locale: "en-US",
    userAgent: frozenLinuxUa,
  });
  try {
    await manualArchCtx.addInitScript(() => {
      window.localStorage.setItem("ice-box.language", "en");
      Object.defineProperty(navigator, "userAgentData", {
        configurable: true,
        value: {
          getHighEntropyValues: () =>
            new Promise((resolve) => {
              window.__resolveArchHint = () => resolve({ architecture: "arm" });
            }),
        },
      });
    });
    const p = await manualArchCtx.newPage();
    await p.goto(`${BASE_URL}/#download`, { waitUntil: "domcontentloaded" });
    await p.locator('[data-linux-arch="amd64"]').click();
    await p.evaluate(() => window.__resolveArchHint());
    await p.waitForFunction(() =>
      document.querySelector(".hero-download")?.getAttribute("href")?.endsWith("_aarch64.AppImage"),
    );
    const pressed = await p.locator('[data-linux-arch="amd64"]').getAttribute("aria-pressed");
    const appimageHref = await p.locator('[data-linux-format="appimage"]').getAttribute("href");
    const debHref = await p.locator('[data-linux-format="deb"]').getAttribute("href");
    if (pressed !== "true" || !appimageHref?.endsWith("_amd64.AppImage") || !debHref?.endsWith("_amd64.deb")) {
      throw new Error(
        `refineLinuxArch overwrote manual x64 pick: ${JSON.stringify({ pressed, appimageHref, debHref })}`,
      );
    }
  } finally {
    await manualArchCtx.close();
  }
}

const preview = startPreview();
preview.stderr.on("data", (chunk) => process.stderr.write(chunk));

let browser;
try {
  await waitForServer(preview);
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

  // The node list must know the tunnel is up before it first paints; otherwise
  // it shows the "proxy service not running" warning over a connected VPN.
  await frame
    .locator('[data-testid="nodes-panel"] [data-testid="node-card"]')
    .first()
    .waitFor({ timeout: 15_000 });
  const firstNode = await frame
    .locator('[data-testid="nodes-panel"] [data-testid="node-card"]')
    .first()
    .textContent();
  if (!firstNode?.includes("🇯🇵 Tokyo / edge-01")) {
    throw new Error(`nodes fixture must carry country flags, got ${JSON.stringify(firstNode)}`);
  }
  if (
    (await frame.getByText("Proxy service not running: delay tests").count()) > 0
  ) {
    throw new Error(
      "phone node list reported a stopped service while the VPN is connected",
    );
  }

  // Favoriting pins the card to the top of this subscription's list. The next
  // profile switch must show its own (empty) favorite set.
  const nodeRows = frame.locator(
    '[data-testid="nodes-panel"] [data-testid="node-card"]',
  );
  await nodeRows
    .nth(1)
    .getByRole("button", { name: "Favorite node" })
    .click();
  await nodeRows
    .first()
    .getByText("🇸🇬 Singapore / edge-02")
    .waitFor({ timeout: 8000 });
  // The optimistic star must survive the post-success refresh.
  if (
    (await nodeRows
      .first()
      .getByRole("button", { name: "Unfavorite node" })
      .count()) !== 1
  ) {
    throw new Error("favorited node must keep its filled star");
  }

  const tabs = frame.getByTestId("phone-tab-bar").getByRole("button");
  if ((await tabs.count()) !== 5) {
    throw new Error(`phone tab bar must show 5 tabs, got ${await tabs.count()}`);
  }

  await tabs.nth(1).click();
  const appRows = frame.locator('[data-testid="apps-panel"] [role="listitem"]');
  await appRows.first().waitFor({ timeout: 15_000 });
  const apps = await appRows.count();
  if (apps !== 20) throw new Error(`apps fixture must show 20 rows, got ${apps}`);
  const appIcons = await appRows.locator("img").count();
  if (appIcons !== apps) {
    throw new Error(`every app row must show a real icon, got ${appIcons}/${apps}`);
  }

  // A routing change is optimistic in the UI, so revisit the tab: the value
  // must come back from the fixture store rather than the seeded default.
  const youtube = frame.getByRole("button", {
    name: "Set the proxy mode for YouTube",
  });
  await youtube.click();
  await frame
    .getByRole("group", { name: "Proxy mode" })
    .getByRole("button", { name: /^Direct/ })
    .click();
  await youtube.getByText("Direct", { exact: true }).waitFor({ timeout: 8000 });
  await tabs.nth(0).click();
  // Home stays attached while hidden (App.tsx KeptPane); wait for its pane to show.
  await frame.locator('[data-active="true"]:has(> [data-testid="home-panel"])').waitFor();
  await tabs.nth(1).click();
  await appRows.first().waitFor();
  // Let the Apps page re-read settings before asserting the value survived.
  await page.waitForTimeout(600);
  await youtube.getByText("Direct", { exact: true }).waitFor({ timeout: 8000 });

  await tabs.nth(3).click();
  const subItems = frame.locator('[data-testid="subs-panel"] [data-slot="item"]');
  await subItems.first().waitFor({ timeout: 15_000 });
  if ((await subItems.count()) !== 3) {
    throw new Error(`subscriptions fixture must show 3 rows, got ${await subItems.count()}`);
  }

  // Switching profiles must load that subscription's node list: the Work
  // profile advertises 12 nodes, while the Personal profile has 18.
  // A row is activated by clicking its surface (here, its title).
  await subItems.nth(1).locator('[data-slot="item-title"]').click();
  await frame
    .locator('[data-testid="subs-panel"] [data-slot="item"][aria-current="true"]')
    .getByText("Work profile", { exact: true })
    .waitFor({ timeout: 8000 });
  await tabs.nth(0).click();
  const homeNodeRows = frame
    .getByTestId("home-panel")
    .locator('[data-testid="nodes-panel"] [data-testid="node-card"]');
  await homeNodeRows.first().waitFor({ timeout: 15_000 });
  // The card grid reports how many nodes it holds, mounted or not.
  await waitForAttribute(
    frame.getByTestId("home-panel").locator('[data-testid="nodes-panel"] [data-card-count]'),
    "data-card-count",
    "12",
  );
  await homeNodeRows
    .first()
    .getByText("🇯🇵 Tokyo / edge-01")
    .waitFor({ timeout: 8000 });
  if (
    (await frame.getByRole("button", { name: "Unfavorite node" }).count()) !== 0
  ) {
    throw new Error("favorites must stay scoped to the demo subscription");
  }

  const power = frame.locator('[data-testid="home-panel"] [data-testid="home-power-button"]');
  if ((await power.textContent())?.trim() !== "Disconnect VPN") {
    throw new Error("phone demo must boot with the VPN connected");
  }
  const warning = frame.getByText("Proxy service not running: delay tests");
  await power.click();
  await waitForText(power, "Connect VPN");
  // The synthesized state-changed event must reach the embedded node list.
  await warning.waitFor({ timeout: 8000 });
  await power.click();
  await waitForText(power, "Disconnect VPN");
  await warning.waitFor({ state: "detached", timeout: 8000 });

  // Settings must match the Android app: the VPN service is the only capture,
  // so the desktop TUN mode, privileged helper, and launch at login stay
  // hidden; with no reachable data folder, so does the Data card's button.
  await frame
    .getByTestId("phone-tab-bar")
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  // Wait until Settings has read the settings and status: the status-gated
  // cards (TUN, helper, launch at login) render only after that, so checking
  // any earlier would pass even if they leaked onto the phone.
  const settings = frame.locator('[data-testid="settings-stack"][data-loaded="true"]');
  await settings.waitFor({ timeout: 15_000 });
  // The Android system card stands in for the desktop Data card.
  await settings
    .locator('[data-slot="card-title"]', { hasText: /^System$/ })
    .waitFor({ timeout: 8000 });
  for (const desktopOnly of ["TUN Mode", "Install Helper", "Launch at login", "Open Data Directory"]) {
    if ((await settings.getByText(desktopOnly, { exact: true }).count()) !== 0) {
      throw new Error(`phone Settings must not show the desktop-only "${desktopOnly}"`);
    }
  }
  await tabs.nth(0).click();
  await frame.locator('[data-active="true"]:has(> [data-testid="home-panel"])').waitFor();

  // Verify the download section's tablist navigation, Linux arch switcher, and
  // uniform desktop height before and after toggling the page language.
  await page.locator("#download").scrollIntoViewIfNeeded();
  await page.evaluate(() => document.fonts.ready);
  await waitForAttribute(page.locator(".download-rail"), "aria-orientation", "vertical");
  await assertUniformDesktopDownloadPanel(page, "1280px en");

  const macosTab = page.locator('[data-download-tab="macos"]');
  await macosTab.click();
  await macosTab.focus();
  await page.keyboard.press("ArrowDown");
  await waitForAttribute(page.locator('[data-download-tab="windows"]'), "aria-selected", "true");
  await page.keyboard.press("End");
  await waitForAttribute(page.locator('[data-download-tab="android"]'), "aria-selected", "true");
  await page.keyboard.press("ArrowLeft");
  await waitForAttribute(page.locator('[data-download-tab="linux"]'), "aria-selected", "true");
  for (const shortcut of ["Alt+ArrowLeft", "Meta+ArrowRight", "Control+ArrowDown", "Shift+ArrowRight"]) {
    await page.keyboard.press(shortcut);
    if ((await page.locator('[data-download-tab="linux"]').getAttribute("aria-selected")) !== "true") {
      throw new Error(`download tablist intercepted modified shortcut ${shortcut}`);
    }
  }

  await page.locator('[data-linux-arch="amd64"]').click();
  await waitForAttribute(page.locator('[data-linux-arch="amd64"]'), "aria-pressed", "true");
  if (!(await page.locator('[data-linux-format="appimage"]').getAttribute("href"))?.endsWith("_amd64.AppImage")) {
    throw new Error("Linux x64 toggle did not update AppImage href");
  }
  await page.locator('[data-linux-arch="arm64"]').click();
  await waitForAttribute(page.locator('[data-linux-arch="arm64"]'), "aria-pressed", "true");
  const armAppImage = await page.locator('[data-linux-format="appimage"]').getAttribute("href");
  const armDeb = await page.locator('[data-linux-format="deb"]').getAttribute("href");
  const armFile = (await page.locator('[data-linux-file="appimage"]').textContent())?.trim();
  if (!armAppImage?.endsWith("_aarch64.AppImage") || !armDeb?.endsWith("_arm64.deb") || !armFile?.endsWith("_aarch64.AppImage")) {
    throw new Error(
      `Linux arm64 toggle did not update links/filenames: ${JSON.stringify({ armAppImage, armDeb, armFile })}`,
    );
  }

  await page.locator(".nav-language").click();
  await frame.getByRole("button", { name: "应用", exact: true }).waitFor({
    timeout: 10_000,
  });
  if (
    (await page.locator('[data-download-tab="linux"]').getAttribute("aria-selected")) !== "true" ||
    (await page.locator('[data-linux-arch="arm64"]').getAttribute("aria-pressed")) !== "true"
  ) {
    throw new Error("language switch did not preserve active download platform and Linux arch");
  }
  await waitForText(page.locator('[data-linux-format="appimage"] .download-action'), "下载 AppImage");
  await assertUniformDesktopDownloadPanel(page, "1280px zh");

  // Theme changes travel through the same storage bridge as the language: the
  // frame follows a write from the app, and the app follows a write from the
  // site or the other demo iframe.
  await frame.locator("html").evaluate(() => {
    window.localStorage.setItem("ice-box.theme", "light");
  });
  await waitForAttribute(page.locator(".phone-frame"), "data-theme", "light");
  await page.evaluate(() => {
    window.localStorage.setItem("ice-box.theme", "dark");
  });
  await waitForTheme(frame, true);
  await page.evaluate(() => {
    window.localStorage.setItem("ice-box.theme", "light");
  });
  await waitForTheme(frame, false);

  if (errors.length > 0) {
    throw new Error(`browser console errors:\n${errors.join("\n")}`);
  }

  // Capture mode must keep the fixture expiry in the future: the subscription
  // list compares it with the real clock, so a date frozen at CAPTURE_NOW would
  // turn every screenshot into an "Expired" state.
  const captureContext = await browser.newContext({
    viewport: { width: 390, height: 868 },
    colorScheme: "dark",
    locale: "en-US",
  });
  await captureContext.addInitScript(() => {
    window.localStorage.setItem("ice-box.theme", "dark");
    window.localStorage.setItem("ice-box.language", "en");
  });
  const capturePage = await captureContext.newPage();
  await capturePage.goto(`${BASE_URL}/mobile-demo.html?capture=1`);
  await capturePage.getByTestId("phone-shell").waitFor({ timeout: 30_000 });
  await capturePage.getByTestId("phone-tab-bar").getByRole("button").nth(3).click();
  const captureSubs = capturePage.locator(
    '[data-testid="subs-panel"] [data-slot="item"]',
  );
  await captureSubs.first().waitFor({ timeout: 15_000 });
  if ((await capturePage.getByText(/Expired/).count()) > 0) {
    throw new Error("capture mode must not mark the subscription fixtures as expired");
  }
  await captureContext.close();
  await verifyDownloadDesktopAndMobile(browser);

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
