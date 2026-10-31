#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
/**
 * Capture the Live Demo Home view into docs/images/home.png (en) and
 * docs/images/home.zh-CN.png (zh) at the desktop app's default window size
 * (860x620), with simulated macOS window controls.
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
import { waitForAppFonts } from "./capture-fonts.mjs";

const VIEWPORT = { width: 860, height: 620 };
const SCALE = 2;
const PREVIEW_PORT = 4175;

/** macOS traffic lights: close, minimize, zoom. */
const TRAFFIC_LIGHTS = [
  { color: "#ff5f57", symbol: "close" },
  { color: "#febc2e", symbol: "minimize" },
  { color: "#28c840", symbol: "zoom" },
];

/** One capture per README language; the parameters are identical otherwise. */
const TARGETS = [
  {
    language: "en",
    locale: "en-US",
    // Power button label (`home.power.stopSystem`) proving the UI language.
    stopLabel: "Stop System Proxy",
    screenshot: "docs/images/home.png",
  },
  {
    language: "zh",
    locale: "zh-CN",
    stopLabel: "关闭系统代理",
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

async function verifyMockedHomeData(page) {
  const uptime = (
    await page.getByTestId("home-run-uptime").textContent({ timeout: 15_000 })
  )?.trim();
  if (!uptime || !/^\d{2}:\d{2}$/.test(uptime)) {
    throw new Error(
      `Live Demo run uptime must show mocked MM:SS data, got ${JSON.stringify(uptime)}`,
    );
  }

  const landingIp = (
    await page.getByTestId("home-outbound-ip").textContent({ timeout: 15_000 })
  )?.trim();
  if (!landingIp || !/^\d{1,3}(?:\.\d{1,3}){3}$/.test(landingIp)) {
    throw new Error(
      `Live Demo landing IP must show mocked IPv4 data, got ${JSON.stringify(landingIp)}`,
    );
  }

  const mcpNav = page.getByRole("button", { name: "MCP", exact: true });
  if ((await mcpNav.count()) === 0) {
    throw new Error("Live Demo must show the MCP navigation entry");
  }

  const mcpStatus = (
    await page.getByTestId("home-health-mcp").textContent({ timeout: 15_000 })
  )?.trim();
  if (!mcpStatus || !/^(Running|运行中)$/.test(mcpStatus)) {
    throw new Error(
      `Live Demo MCP service status must be running, got ${JSON.stringify(mcpStatus)}`,
    );
  }
}

/**
 * Overlay the three macOS window controls on the Live Demo page. The desktop
 * app uses an overlay title bar (`titleBarStyle: "Overlay"`), so the lights sit
 * on top of the sidebar instead of a separate title bar strip.
 */
async function addMacWindowControls(page) {
  await page.evaluate((lights) => {
    const glyphs = {
      close:
        '<svg width="8" height="8" viewBox="0 0 12 12" fill="none" aria-hidden="true">' +
        '<path d="M4.1 4.1 7.9 7.9M7.9 4.1 4.1 7.9" stroke="rgba(0, 0, 0, 0.55)" stroke-width="1.4" stroke-linecap="round" />' +
        "</svg>",
      minimize:
        '<svg width="8" height="8" viewBox="0 0 12 12" fill="none" aria-hidden="true">' +
        '<path d="M4 6h4" stroke="rgba(0, 0, 0, 0.55)" stroke-width="1.4" stroke-linecap="round" />' +
        "</svg>",
      zoom:
        '<svg width="9" height="9" viewBox="0 0 12 12" fill="none" aria-hidden="true">' +
        '<path d="M7.1 4.9 10 2M10 2H7.8M10 2v2.2" stroke="rgba(0, 0, 0, 0.55)" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" />' +
        '<path d="M4.9 7.1 2 10M2 10h2.2M2 10V7.8" stroke="rgba(0, 0, 0, 0.55)" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" />' +
        "</svg>",
    };
    document.getElementById("ice-box-capture-window-controls")?.remove();
    const controls = document.createElement("div");
    controls.id = "ice-box-capture-window-controls";
    Object.assign(controls.style, {
      position: "fixed",
      top: "12px",
      left: "12px",
      display: "flex",
      gap: "8px",
      zIndex: "2147483647",
      pointerEvents: "none",
    });
    for (const light of lights) {
      const dot = document.createElement("span");
      Object.assign(dot.style, {
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        width: "12px",
        height: "12px",
        borderRadius: "50%",
        backgroundColor: light.color,
        lineHeight: "0",
        boxShadow:
          "inset 0 0.5px 0 rgba(255, 255, 255, 0.35), " +
          "inset 0 0 0 0.5px rgba(0, 0, 0, 0.22), " +
          "0 0 0 0.5px rgba(0, 0, 0, 0.14)",
      });
      dot.innerHTML = glyphs[light.symbol];
      controls.append(dot);
    }
    document.body.append(controls);
  }, TRAFFIC_LIGHTS);
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
      .getByRole("button", { name: /Stop System Proxy|关闭系统代理/ })
      .waitFor({ timeout: 30_000 });
    await waitForLanguage(page, target);
    await page.evaluate(() => window.dispatchEvent(new Event("resize")));
    await page.waitForFunction(() => {
      // The home chart is a compact module, not a full-height pane.
      const el = document.querySelector(".recharts-wrapper");
      return el && el.getBoundingClientRect().height >= 80;
    }, { timeout: 15_000 });
    await verifyMockedHomeData(page);
    // Wait for Inter before the shot: the fallback font changes text metrics,
    // which decides whether long labels truncate.
    await waitForAppFonts(page);
    await addMacWindowControls(page);
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
