#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
/**
 * Capture the phone shell home view into the README images:
 *   docs/images/mobile-home.png      mobile-home.zh-CN.png
 *
 * Builds the mobile frontend with `--mode capture`, which replaces the Tauri
 * transport with the Live Demo browser mock. The viewport matches a common
 * Android phone (390x844) and screenshots are taken at 2x for GitHub, then
 * wrapped in the website phone shell (`src/phone-demo.css` and
 * `src/phone-status-icons.js`) with an Android status bar and gesture
 * navigation.
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
import { phoneStatusIcons } from "../src/phone-status-icons.js";
import { waitForAppFonts } from "./capture-fonts.mjs";

const VIEWPORT = { width: 390, height: 844 };
const SCALE = 2;
const PREVIEW_PORT = 4176;
const MOBILE_TABS = 5;
/** Gesture-navigation inset. The shell chrome itself comes from phone-demo.css. */
const PHONE_NAV_INSET = 24;

/** One capture per README language; parameters are identical otherwise. */
const TARGETS = [
  {
    language: "en",
    locale: "en-US",
    // Power button label (`home.power.vpnStop`) proving the language and
    // that the mocked tunnel is connected.
    vpnStopLabel: "Disconnect VPN",
    screenshots: {
      home: "docs/images/mobile-home.png",
    },
  },
  {
    language: "zh",
    locale: "zh-CN",
    vpnStopLabel: "断开 VPN",
    screenshots: {
      home: "docs/images/mobile-home.zh-CN.png",
    },
  },
];

/** Tab order in `App.tsx`: Home, Apps, Rules, Subs, Settings. */
const TAB_INDEX = { home: 0, apps: 1 };

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

  const node = (
    await page.getByTestId("home-current-node").textContent({ timeout: 15_000 })
  )?.trim();
  if (!node || node === "—") {
    throw new Error(`Phone Home must show the selected node, got ${JSON.stringify(node)}`);
  }

  const nodeRow = page
    .getByTestId("nodes-panel")
    .locator('[data-testid="node-card"]')
    .first();
  await nodeRow.waitFor({ timeout: 15_000 });
}

async function settle(page) {
  await new Promise((resolve) => setTimeout(resolve, 400));
}

/**
 * Wrap a raw phone screenshot in the website phone shell. The frame, status
 * icons, and gesture bar come from the landing page so a shell change does
 * not have to be copied here.
 */
function phoneShellHtml(screenshot) {
  const css = fs.readFileSync(path.join(websiteDir, "src/phone-demo.css"), "utf8");
  const image = `data:image/png;base64,${screenshot.toString("base64")}`;
  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <style>
      :root {
        --sans: "Inter", "Roboto", system-ui, -apple-system, "Segoe UI", sans-serif;
      }
      * { box-sizing: border-box; }
      html, body { margin: 0; padding: 0; overflow: hidden; background: transparent; }
      body { width: max-content; }
      /* Room for the side buttons and the hard pixel shadow, which sit
         outside the frame. Wider than 1100px so the landing-page mobile
         stage rule does not shrink the shell. */
      .capture-stage { display: inline-block; padding: 8px 20px 20px 16px; }
      .phone-viewport .phone-capture {
        position: absolute;
        inset: 0;
        display: block;
        width: 100%;
        height: 100%;
      }
      ${css}
    </style>
  </head>
  <body>
    <div class="capture-stage">
      <div class="phone-demo-stage">
        <div class="phone-frame">
          <span class="phone-sim" aria-hidden="true"></span>
          <div class="phone-screen">
            <div class="phone-status-bar" aria-hidden="true">
              <span class="phone-clock">12:00</span>
              <div class="phone-status-icons">${phoneStatusIcons}</div>
            </div>
            <div class="phone-viewport">
              <img class="phone-capture" src="${image}" alt="" />
              <span class="phone-gesture" aria-hidden="true"></span>
            </div>
            <span class="phone-camera" aria-hidden="true"></span>
          </div>
        </div>
      </div>
    </div>
  </body>
</html>`;
}

async function addPhoneShell(browser, screenshot) {
  const context = await browser.newContext({
    viewport: { width: 1400, height: 1600 },
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
      .locator(".capture-stage")
      .screenshot({ type: "png", omitBackground: true });
  } finally {
    await context.close();
  }
}

/**
 * Tab button by position, so the capture does not depend on translated labels.
 */
function tabButton(page, tab) {
  return page.getByTestId("phone-tab-bar").getByRole("button").nth(TAB_INDEX[tab]);
}

/**
 * Visit Apps once so returning to Home reloads the node state. The screenshot
 * itself is taken only on Home.
 */
async function visitApps(page) {
  await tabButton(page, "apps").click();
  await page.getByTestId("apps-panel").waitFor({ timeout: 15_000 });
  await page
    .getByTestId("apps-panel")
    .getByRole("listitem")
    .first()
    .waitFor({ timeout: 15_000 });
}

async function capture(browser, baseUrl, target) {
  const context = await browser.newContext({
    // The viewport includes the gesture navigation inset so the phone UI draws
    // edge-to-edge behind the system navigation bar, as it does on Android.
    viewport: { width: VIEWPORT.width, height: VIEWPORT.height + PHONE_NAV_INSET },
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
    // MainActivity sets --ice-nav-inset from the real navigation bar height on
    // Android; the browser mock reports no safe-area inset, so provide it once
    // the document exists (init scripts run before documentElement is created).
    await page.evaluate((navInset) => {
      document.documentElement.style.setProperty("--ice-nav-inset", `${navInset}px`);
    }, PHONE_NAV_INSET);
    await page.getByTestId("phone-shell").waitFor({ timeout: 30_000 });

    // The embedded node list shares the runtime store. On the first frame the
    // store can still be empty, so leave and re-enter Home once: the second
    // activation refreshes the node state against the loaded status and drops
    // the transient "service not running" warning.
    await visitApps(page);
    await tabButton(page, "home").click();
    await page
      .getByTestId("nodes-panel")
      .getByRole("alert")
      .first()
      .waitFor({ state: "detached", timeout: 15_000 });

    await verifyHomeData(page, target);
    // Wait for Inter before the shot: the fallback font changes text metrics
    // and can shift the phone layout.
    await waitForAppFonts(page);
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
    "capture-mobile: install desktop deps first (the mobile app reuses them):\n" +
      "  (cd apps/desktop && npm ci)\n" +
      "  (cd apps/mobile && npm run build)  # links node_modules on a fresh clone",
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
