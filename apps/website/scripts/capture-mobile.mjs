#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
/**
 * Capture the phone shell home view into the README images:
 *   docs/images/mobile-home.png      mobile-home.zh-CN.png
 *
 * Builds the mobile frontend with `--mode capture`, which replaces the Tauri
 * transport with the Live Demo browser mock. The viewport matches a common
 * Android phone (390x844) and screenshots are taken at 2x for GitHub, then
 * wrapped in a current-generation Pixel-style phone shell with an Android
 * status bar and gesture navigation.
 *
 * Usage:
 *   bash scripts/capture-demo-mobile.sh
 *   CAPTURE_DEMO_MOBILE_FORCE=1 bash scripts/capture-demo-mobile.sh
 */
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const VIEWPORT = { width: 390, height: 844 };
const SCALE = 2;
const PREVIEW_PORT = 4176;
const MOBILE_TABS = 5;
const PHONE_SHELL = {
  bezelX: 14,
  bezelY: 16,
  radius: 48,
  padding: 28,
  statusBar: 24,
  navBar: 24,
};

/** One capture per README language; parameters are identical otherwise. */
const TARGETS = [
  {
    language: "en",
    locale: "en-US",
    // Power button label (`home.power.vpnStop`) proving the language and
    // that the mocked tunnel is connected.
    vpnStopLabel: "Disconnect VPN",
    homeLabel: "Home",
    // Only used to leave Home before re-entering it, so the runtime store
    // refreshes against the loaded status.
    appsLabel: "Apps",
    screenshots: {
      home: "docs/images/mobile-home.png",
    },
  },
  {
    language: "zh",
    locale: "zh-CN",
    vpnStopLabel: "断开 VPN",
    homeLabel: "主页",
    // Only used to leave Home before re-entering it, so the runtime store
    // refreshes against the loaded status.
    appsLabel: "应用",
    screenshots: {
      home: "docs/images/mobile-home.zh-CN.png",
    },
  },
];

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const websiteDir = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(websiteDir, "../..");
const mobileDir = path.join(repoRoot, "apps/mobile");
const versionFile = path.join(repoRoot, "docs/images/mobile.version");
const mobilePackageJson = path.join(mobileDir, "package.json");

function screenshotPath(target, page) {
  return path.join(repoRoot, target.screenshots[page]);
}

function appVersion() {
  return JSON.parse(fs.readFileSync(mobilePackageJson, "utf8")).version;
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

function viteBin() {
  return path.join(mobileDir, "node_modules/.bin/vite");
}

async function buildCapture() {
  await new Promise((resolve, reject) => {
    const child = spawn(viteBin(), ["build", "--mode", "capture"], {
      cwd: mobileDir,
      stdio: "inherit",
    });
    child.once("exit", (code) => {
      if (code === 0) resolve();
      else reject(new Error(`Mobile capture build failed with code ${code}`));
    });
  });
}

function startPreview() {
  return spawn(
    viteBin(),
    [
      "preview",
      "--mode",
      "capture",
      "--host",
      "127.0.0.1",
      "--port",
      String(PREVIEW_PORT),
      "--strictPort",
    ],
    { cwd: mobileDir, stdio: ["ignore", "pipe", "pipe"] },
  );
}

async function verifyHomeData(page, target) {
  await page.getByTestId("phone-shell").waitFor({ timeout: 30_000 });
  await page
    .getByRole("button", { name: target.vpnStopLabel, exact: true })
    .waitFor({ timeout: 30_000 });

  const tabs = page.getByTestId("phone-tab-bar").getByRole("button");
  if ((await tabs.count()) !== MOBILE_TABS) {
    throw new Error(`Phone tab bar must show ${MOBILE_TABS} tabs, got ${await tabs.count()}`);
  }

  const uptime = (
    await page.getByTestId("home-run-uptime").textContent({ timeout: 15_000 })
  )?.trim();
  if (!uptime || !/^\d{2}:\d{2}$/.test(uptime)) {
    throw new Error(`Phone run uptime must show mocked MM:SS data, got ${JSON.stringify(uptime)}`);
  }

  const node = (
    await page.getByTestId("home-current-node").textContent({ timeout: 15_000 })
  )?.trim();
  if (!node || node === "—") {
    throw new Error(`Phone Home must show the selected node, got ${JSON.stringify(node)}`);
  }

  const nodeRow = page
    .getByTestId("nodes-panel")
    .locator('[role="listitem"]')
    .first();
  await nodeRow.waitFor({ timeout: 15_000 });
}

async function settle(page) {
  await new Promise((resolve) => setTimeout(resolve, 400));
}

/** Pixel-like shell dimensions shared by the HTML and the capture viewport. */
function phoneShellSize() {
  const screenHeight =
    VIEWPORT.height + PHONE_SHELL.statusBar + PHONE_SHELL.navBar;
  return {
    width:
      VIEWPORT.width + PHONE_SHELL.bezelX * 2 + PHONE_SHELL.padding * 2,
    height:
      screenHeight + PHONE_SHELL.bezelY * 2 + PHONE_SHELL.padding * 2,
  };
}

/**
 * Wrap a raw phone screenshot in a Pixel-style shell with Android system bars.
 * The frame is drawn in CSS so captures stay reproducible without image assets.
 */
function phoneShellHtml(screenshot) {
  const screenWidth = VIEWPORT.width;
  const { bezelX, bezelY, radius, padding, statusBar, navBar } = PHONE_SHELL;
  const screenHeight = VIEWPORT.height + statusBar + navBar;
  const shellWidth = screenWidth + bezelX * 2;
  const shellHeight = screenHeight + bezelY * 2;
  const screenRadius = radius - bezelY;
  const image = `data:image/png;base64,${screenshot.toString("base64")}`;
  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <style>
      * { box-sizing: border-box; }
      html, body { margin: 0; padding: 0; overflow: hidden; background: transparent; }
      body { width: max-content; }
      .stage { display: inline-block; padding: ${padding}px; }
      .phone {
        position: relative;
        width: ${shellWidth}px;
        height: ${shellHeight}px;
        padding: ${bezelY}px ${bezelX}px;
        border-radius: ${radius}px;
        background:
          linear-gradient(145deg, #6d6e72 0%, #303135 8%, #121315 26%, #0c0d0f 62%, #232427 88%, #55565a 100%);
        box-shadow:
          0 0 0 1px rgba(255, 255, 255, 0.1),
          0 28px 56px rgba(0, 0, 0, 0.6),
          0 4px 14px rgba(0, 0, 0, 0.45),
          inset 0 1px 0 rgba(255, 255, 255, 0.2),
          inset 0 -1px 0 rgba(255, 255, 255, 0.08),
          inset 0 0 0 1px rgba(0, 0, 0, 0.9);
      }
      .phone::before,
      .phone::after {
        content: "";
        position: absolute;
        right: -3px;
        width: 3px;
        border-radius: 0 2px 2px 0;
        background: linear-gradient(180deg, #5a5b5f, #26272a 45%, #4a4b4f);
        box-shadow: inset 0 0 1px rgba(255, 255, 255, 0.25);
      }
      .phone::before {
        top: 168px;
        height: 60px;
      }
      .phone::after {
        top: 244px;
        height: 42px;
      }
      .sim-tray {
        position: absolute;
        left: -2px;
        top: 220px;
        width: 2px;
        height: 26px;
        border-radius: 1px;
        background: #202124;
        box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.04);
      }
      .screen {
        position: relative;
        display: flex;
        flex-direction: column;
        width: ${screenWidth}px;
        height: ${screenHeight}px;
        overflow: hidden;
        border-radius: ${screenRadius}px;
        background: #000;
        box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.95);
      }
      .status-bar {
        display: flex;
        flex: 0 0 ${statusBar}px;
        align-items: center;
        justify-content: space-between;
        height: ${statusBar}px;
        padding: 0 14px 0 16px;
        background: #131416;
        color: #ecedef;
        font-family: "Inter", "Roboto", system-ui, -apple-system, "Segoe UI", sans-serif;
      }
      .clock { font-size: 12.5px; font-weight: 600; letter-spacing: 0.1px; }
      .status-icons { display: flex; align-items: center; gap: 5px; }
      .network { font-size: 9px; font-weight: 700; letter-spacing: 0.2px; }
      .status-icons svg { display: block; }
      .screen img {
        display: block;
        flex: 0 0 ${VIEWPORT.height}px;
        width: 100%;
        height: ${VIEWPORT.height}px;
      }
      .nav-bar {
        display: flex;
        flex: 0 0 ${navBar}px;
        align-items: center;
        justify-content: center;
        height: ${navBar}px;
        background: #0b0c0e;
      }
      .gesture-pill {
        width: 104px;
        height: 4px;
        border-radius: 999px;
        background: rgba(255, 255, 255, 0.85);
        box-shadow: 0 0 2px rgba(0, 0, 0, 0.4);
      }
      .screen::after {
        content: "";
        position: absolute;
        inset: 0;
        border-radius: inherit;
        box-shadow:
          inset 0 0 0 1px rgba(255, 255, 255, 0.06),
          inset 0 0 14px rgba(0, 0, 0, 0.3);
        pointer-events: none;
      }
      .camera {
        position: absolute;
        z-index: 2;
        top: 6px;
        left: 50%;
        width: 11px;
        height: 11px;
        transform: translateX(-50%);
        border-radius: 50%;
        background: radial-gradient(circle at 35% 35%, #1c2430 0%, #05070a 55%, #000 100%);
        box-shadow:
          0 0 0 1px rgba(255, 255, 255, 0.06),
          inset 0 0 2px rgba(90, 130, 200, 0.35);
      }
    </style>
  </head>
  <body>
    <div class="stage">
      <div class="phone">
        <span class="sim-tray"></span>
        <div class="screen">
          <div class="status-bar">
            <span class="clock">12:00</span>
            <div class="status-icons">
              <span class="network">5G</span>
              <svg width="14" height="12" viewBox="0 0 14 12" fill="none" aria-hidden="true">
                <rect x="0.5" y="8" width="2.2" height="3.5" rx="0.7" fill="#ecedef" />
                <rect x="4" y="6" width="2.2" height="5.5" rx="0.7" fill="#ecedef" />
                <rect x="7.5" y="3.5" width="2.2" height="8" rx="0.7" fill="#ecedef" />
                <rect x="11" y="1" width="2.2" height="10.5" rx="0.7" fill="#ecedef" />
              </svg>
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path d="M12 2.5 20 5.5v6c0 4.5-3.2 8.2-8 10-4.8-1.8-8-5.5-8-10v-6L12 2.5Z" stroke="#ecedef" stroke-width="2" stroke-linejoin="round" />
              </svg>
              <svg width="14" height="12" viewBox="0 0 14 12" fill="none" aria-hidden="true">
                <path d="M1 4.2C2.6 2.8 4.7 2 7 2s4.4.8 6 2.2" stroke="#ecedef" stroke-width="1.6" stroke-linecap="round" />
                <path d="M3.1 6.7C4.2 5.7 5.5 5.1 7 5.1s2.8.6 3.9 1.6" stroke="#ecedef" stroke-width="1.6" stroke-linecap="round" />
                <path d="M5.2 9.1C5.8 8.5 6.4 8.2 7 8.2s1.2.3 1.8.9" stroke="#ecedef" stroke-width="1.6" stroke-linecap="round" />
                <circle cx="7" cy="10.4" r="0.9" fill="#ecedef" />
              </svg>
              <svg width="22" height="11" viewBox="0 0 22 11" fill="none" aria-hidden="true">
                <rect x="0.5" y="0.5" width="19" height="10" rx="2.5" stroke="#ecedef" stroke-opacity="0.85" />
                <rect x="2" y="2" width="14.5" height="7" rx="1.4" fill="#ecedef" />
                <rect x="20.2" y="3.5" width="1.5" height="4" rx="0.75" fill="#ecedef" fill-opacity="0.85" />
              </svg>
            </div>
          </div>
          <img src="${image}" alt="" />
          <div class="nav-bar">
            <span class="gesture-pill"></span>
          </div>
          <span class="camera"></span>
        </div>
      </div>
    </div>
  </body>
</html>`;
}

async function addPhoneShell(browser, screenshot) {
  const context = await browser.newContext({
    viewport: phoneShellSize(),
    deviceScaleFactor: SCALE,
    colorScheme: "dark",
  });
  try {
    const page = await context.newPage();
    await page.setContent(phoneShellHtml(screenshot), { waitUntil: "load" });
    await page.evaluate(async () => {
      await document.fonts.ready;
      await Promise.all(
        Array.from(document.images, (image) =>
          image.complete ? Promise.resolve() : image.decode(),
        ),
      );
    });
    return await page
      .locator(".stage")
      .screenshot({ type: "png", omitBackground: true });
  } finally {
    await context.close();
  }
}

/**
 * Visit Apps once so returning to Home reloads the node state. The screenshot
 * itself is taken only on Home.
 */
async function visitApps(page, target) {
  await page
    .getByTestId("phone-tab-bar")
    .getByRole("button", { name: target.appsLabel, exact: true })
    .click();
  await page.getByTestId("apps-panel").waitFor({ timeout: 15_000 });
  await page
    .getByTestId("apps-panel")
    .getByRole("listitem")
    .first()
    .waitFor({ timeout: 15_000 });
}

async function capture(browser, baseUrl, target) {
  const context = await browser.newContext({
    viewport: VIEWPORT,
    deviceScaleFactor: SCALE,
    isMobile: true,
    hasTouch: true,
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
    await page.goto(`${baseUrl}/capture.html?capture=1`, {
      waitUntil: "domcontentloaded",
      timeout: 60_000,
    });
    await page.getByTestId("phone-shell").waitFor({ timeout: 30_000 });

    // The embedded node list shares the runtime store. On the first frame the
    // store can still be empty, so leave and re-enter Home once: the second
    // activation refreshes the node state against the loaded status and drops
    // the transient "service not running" warning.
    await visitApps(page, target);
    await page
      .getByTestId("phone-tab-bar")
      .getByRole("button", { name: target.homeLabel, exact: true })
      .click();
    await page
      .getByTestId("nodes-panel")
      .getByRole("alert")
      .first()
      .waitFor({ state: "detached", timeout: 15_000 });

    await verifyHomeData(page, target);
    await settle(page);
    const home = await page.screenshot({ type: "png" });

    return { home };
  } finally {
    await context.close();
  }
}

if (!fs.existsSync(path.join(websiteDir, "node_modules/playwright"))) {
  console.error(
    "capture-mobile: install website deps first:\n" +
      "  (cd apps/website && npm ci && npx playwright install chromium)",
  );
  process.exit(1);
}
if (!fs.existsSync(viteBin())) {
  console.error(
    "capture-mobile: install desktop deps first (mobile links them):\n" +
      "  (cd apps/desktop && npm ci)",
  );
  process.exit(1);
}

await buildCapture();
const preview = startPreview();
try {
  await waitForOutput(preview, /Local:/, 30_000);
  const baseUrl = `http://127.0.0.1:${PREVIEW_PORT}`;
  const browser = await chromium.launch({ headless: true });
  try {
    // Capture every language first and only then write the files, so a
    // failure in either capture leaves the previous set untouched.
    const shots = [];
    for (const target of TARGETS) {
      try {
        const raw = await capture(browser, baseUrl, target);
        shots.push({ target, png: await addPhoneShell(browser, raw.home) });
      } catch (error) {
        throw new Error(
          `[${target.language}] ${target.screenshots.home}: ${
            error instanceof Error ? error.message : error
          }`,
          { cause: error },
        );
      }
    }
    const version = appVersion();
    for (const { target, png } of shots) {
      const outFile = screenshotPath(target, "home");
      fs.mkdirSync(path.dirname(outFile), { recursive: true });
      fs.writeFileSync(outFile, png);
      console.log(`Wrote ${target.screenshots.home} (${version})`);
    }
    fs.writeFileSync(versionFile, `${version}\n`);
  } finally {
    await browser.close();
  }
} finally {
  preview.kill("SIGTERM");
}
