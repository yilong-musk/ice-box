#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
/**
 * Capture the phone shell into the README images:
 *   docs/images/mobile-home.png      mobile-home.zh-CN.png
 *   docs/images/mobile-apps.png      mobile-apps.zh-CN.png
 *
 * Builds the mobile frontend with `--mode capture`, which replaces the Tauri
 * transport with the Live Demo browser mock. The viewport matches a common
 * Android phone (390x844) and screenshots are taken at 2x for GitHub.
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

/** One capture per README language; parameters are identical otherwise. */
const TARGETS = [
  {
    language: "en",
    locale: "en-US",
    // Power button label (`home.power.vpnStop`) proving the language and
    // that the mocked tunnel is connected.
    vpnStopLabel: "Disconnect VPN",
    homeLabel: "Home",
    appsLabel: "Apps",
    screenshots: {
      home: "docs/images/mobile-home.png",
      apps: "docs/images/mobile-apps.png",
    },
  },
  {
    language: "zh",
    locale: "zh-CN",
    vpnStopLabel: "断开 VPN",
    homeLabel: "主页",
    appsLabel: "应用",
    screenshots: {
      home: "docs/images/mobile-home.zh-CN.png",
      apps: "docs/images/mobile-apps.zh-CN.png",
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

async function openApps(page, target) {
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
    await openApps(page, target);
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

    await openApps(page, target);
    await settle(page);
    const apps = await page.screenshot({ type: "png" });

    return { home, apps };
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
        shots.push({ target, png: await capture(browser, baseUrl, target) });
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
      for (const name of ["home", "apps"]) {
        const outFile = screenshotPath(target, name);
        fs.mkdirSync(path.dirname(outFile), { recursive: true });
        fs.writeFileSync(outFile, png[name]);
        console.log(`Wrote ${target.screenshots[name]} (${version})`);
      }
    }
    fs.writeFileSync(versionFile, `${version}\n`);
  } finally {
    await browser.close();
  }
} finally {
  preview.kill("SIGTERM");
}
