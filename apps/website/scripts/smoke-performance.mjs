#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later

/** Text-only browser regression: npm --prefix apps/website run test:performance. */
import assert from "node:assert/strict";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer } from "vite";

const websiteDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const nodeCount = 5000;
const server = await createServer({
  root: websiteDir,
  configFile: path.join(websiteDir, "vite.config.ts"),
  server: { host: "127.0.0.1", port: 4176, strictPort: false, open: false },
});
let browser;

try {
  await server.listen();
  const baseUrl = server.resolvedUrls.local[0];
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1180, height: 690 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));

  // This mode freezes demo traffic and disables mock latency, without taking images.
  await page.goto(new URL("demo.html?capture=1", baseUrl).href);
  await page.getByRole("button", { name: "Nodes", exact: true }).waitFor();
  const runUptime = await page.getByTestId("home-run-uptime").textContent();
  assert.match(runUptime?.trim() ?? "", /^\d{2}:\d{2}$/, "The Live Demo must show mocked run uptime");
  const landingIp = await page.getByTestId("home-outbound-ip").textContent();
  assert.equal(
    landingIp?.trim(),
    "203.0.113.42",
    "The Live Demo must show the mocked landing IP",
  );
  await page.evaluate(async count => {
    const { api } = await import("/src/browser-api.ts");
    const members = Array.from({ length: count }, (_, i) => `Benchmark node ${i}`);
    api.listNodes = async () => [
      { tag: "Benchmark group", outbound_type: "selector", group_now: members[0], group_all: members },
      ...members.map(tag => ({ tag, outbound_type: "trojan", group_now: null, group_all: null })),
    ];
  }, nodeCount);

  await page.getByRole("button", { name: "Nodes", exact: true }).click();
  const panel = page.getByTestId("nodes-panel");
  const viewport = panel.locator('[data-slot="scroll-area-viewport"]');
  const titles = panel.locator('[data-testid="node-card-title"]');
  // A grouped profile lists its group collapsed: members stay unmounted.
  const toggle = panel.getByRole("button", { name: "Benchmark group", exact: true });
  await toggle.waitFor();
  const firstCount = await titles.count();
  assert.equal(firstCount, 0, "A collapsed group must not mount its members");

  await toggle.click();
  const group = panel.locator('[id^="group-members-"]');
  await group.getByText("Benchmark node 1", { exact: true }).waitFor();
  const firstMembers = await group.locator('[data-testid="node-card"]').count();
  assert(firstMembers > 0 && firstMembers < 120, "Expanded member cards must remain virtualized");

  await viewport.evaluate(el => { el.scrollTop = el.scrollHeight; });
  await group.getByText(`Benchmark node ${nodeCount - 1}`, { exact: true }).waitFor();
  const lastMembers = await group.locator('[data-testid="node-card"]').count();
  assert(lastMembers > 0 && lastMembers < 120, "The last member must not require mounting every member");
  const lastCount = await titles.count();
  const previousTop = await viewport.evaluate(el => el.scrollTop);
  assert(previousTop > 0, "The test must reach the bottom of a scrollable list");

  await page.getByRole("button", { name: "Home", exact: true }).click();
  // Hidden, the Nodes pane keeps its rows and scroll offset (App.tsx KeptPane).
  await page.locator('[data-active="false"]:has(> [data-testid="nodes-panel"])').waitFor();
  assert.equal(await viewport.evaluate(el => el.scrollTop), previousTop, "The hidden pane must keep its scroll position");
  await page.getByRole("button", { name: "Nodes", exact: true }).click();
  await titles.filter({ hasText: `Benchmark node ${nodeCount - 1}` }).waitFor();
  assert.equal(await viewport.evaluate(el => el.scrollTop), previousTop, "Reactivation must restore scroll position");

  await page.evaluate(async () => {
    const { api } = await import("/src/browser-api.ts");
    api.listNodes = async () => [
      { tag: "Replacement node", outbound_type: "trojan", group_now: null, group_all: null },
    ];
  });
  await page.getByRole("button", { name: "Home", exact: true }).click();
  await page.getByRole("button", { name: "Nodes", exact: true }).click();
  await panel.getByText("Replacement node", { exact: true }).waitFor();
  assert.equal(await titles.count(), 1, "A smaller subscription must replace the old window");
  assert.equal(await viewport.evaluate(el => el.scrollTop), 0, "A smaller list must clamp its restored offset");
  assert.deepEqual(errors, [], "The browser must not report uncaught errors");

  console.log(JSON.stringify({
    nodes: nodeCount,
    groupMembers: nodeCount,
    firstCount,
    firstMembers,
    lastMembers,
    lastCount,
    scrollRestored: true,
    shrinkClamped: true,
    errors,
  }));
} finally {
  try {
    await browser?.close();
  } finally {
    await server.close();
  }
}
