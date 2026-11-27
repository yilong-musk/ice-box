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

  async function pickSiteLanguage(code) {
    await page.locator(".nav-language").click();
    await page.locator(`.nav-language-option[data-language="${code}"]`).click();
  }

  await pickSiteLanguage("zh");
  await frame.getByRole("button", { name: "应用", exact: true }).waitFor({
    timeout: 10_000,
  });

  // Persian and Arabic switch both the landing page and the embedded phone app to RTL.
  await pickSiteLanguage("fa");
  await waitForAttribute(page.locator("html"), "dir", "rtl");
  await waitForAttribute(frame.locator("html"), "dir", "rtl");
  await frame.getByRole("button", { name: "برنامه‌ها", exact: true }).waitFor({
    timeout: 10_000,
  });

  await pickSiteLanguage("ar");
  await waitForAttribute(page.locator("html"), "dir", "rtl");
  await waitForAttribute(frame.locator("html"), "dir", "rtl");
  await waitForAttribute(frame.locator("html"), "lang", "ar");

  // Switching back to an LTR locale restores `dir="ltr"` everywhere.
  await pickSiteLanguage("en");
  await waitForAttribute(page.locator("html"), "dir", "ltr");
  await waitForAttribute(frame.locator("html"), "dir", "ltr");

  // Keyboard navigation on the language popover: clicking opens it with focus
  // on the trigger, ArrowDown moves into the selected option, and Tab closes
  // the menu while advancing focus cleanly rather than dropping it to <body>.
  await page.locator(".nav-language").click();
  await page.keyboard.press("ArrowDown");
  const focusedLang = await page.evaluate(
    () => document.activeElement?.getAttribute("data-language") ?? null,
  );
  if (focusedLang !== "en") {
    throw new Error(
      `ArrowDown after opening language menu must focus active option "en", got ${JSON.stringify(focusedLang)}`,
    );
  }
  await page.keyboard.press("Tab");
  const tabResult = await page.evaluate(() => ({
    menuHidden: document.querySelector(".nav-language-menu")?.hasAttribute("hidden") ?? false,
    activeTag: document.activeElement?.tagName ?? null,
    insideMenu: Boolean(
      document.querySelector(".nav-language-menu")?.contains(document.activeElement),
    ),
  }));
  if (!tabResult.menuHidden || tabResult.insideMenu || tabResult.activeTag === "BODY") {
    throw new Error(
      `Tab inside language menu must close it and advance focus cleanly, got ${JSON.stringify(tabResult)}`,
    );
  }

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
  // Returning to a previously visited Settings tab (<Activity mode="hidden">)
  // must keep the light theme instead of re-applying its stale dark state.
  await frame
    .getByTestId("phone-tab-bar")
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  await frame
    .locator('[data-testid="settings-stack"][data-loaded="true"]')
    .waitFor({ timeout: 15_000 });
  await waitForTheme(frame, false);
  if ((await frame.locator("#settings-theme").inputValue()) !== "light") {
    throw new Error(
      `Settings theme select must reflect "light" after returning from hidden Activity, got ${JSON.stringify(await frame.locator("#settings-theme").inputValue())}`,
    );
  }

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

  // Initial load with an RTL preference must set `lang`, `dir="rtl"`, and the
  // stored dark theme on the landing page and both embedded demo iframes before
  // any language switch happens.
  const rtlContext = await browser.newContext({
    viewport: { width: 1280, height: 1000 },
    colorScheme: "dark",
    locale: "ar-SA",
  });
  await rtlContext.addInitScript(() => {
    window.localStorage.setItem("ice-box.theme", "dark");
    window.localStorage.setItem("ice-box.language", "ar");
  });
  const rtlPage = await rtlContext.newPage();
  await rtlPage.goto(`${BASE_URL}/`, { waitUntil: "domcontentloaded" });
  const rtlDesktopFrame = rtlPage.frameLocator(".demo-screen iframe");
  const rtlPhoneFrame = rtlPage.frameLocator(".phone-viewport iframe");
  await rtlDesktopFrame.locator('[data-testid="home-panel"]').waitFor({ timeout: 30_000 });
  await rtlPhoneFrame.locator('[data-testid="phone-shell"]').waitFor({ timeout: 30_000 });
  await waitForAttribute(rtlPage.locator("html"), "dir", "rtl");
  await waitForAttribute(rtlDesktopFrame.locator("html"), "lang", "ar");
  await waitForAttribute(rtlDesktopFrame.locator("html"), "dir", "rtl");
  await waitForTheme(rtlDesktopFrame, true);
  await waitForAttribute(rtlPhoneFrame.locator("html"), "lang", "ar");
  await waitForAttribute(rtlPhoneFrame.locator("html"), "dir", "rtl");
  await waitForTheme(rtlPhoneFrame, true);
  await rtlContext.close();

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
