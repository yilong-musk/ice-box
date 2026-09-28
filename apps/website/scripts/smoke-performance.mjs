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
  await panel.getByText("Benchmark node 1", { exact: true }).waitFor();
  const viewport = panel.locator('[data-slot="scroll-area-viewport"]');
  const titles = panel.locator('[data-slot="item-title"]');
  const firstCount = await titles.count();
  assert(firstCount > 0 && firstCount < 25, "Only the visible node window should be mounted");

  await panel.getByRole("button", { name: "Benchmark group", exact: true }).click();
  const group = panel.locator('[id^="group-members-"]');
  await group.getByText("Benchmark node 1", { exact: true }).waitFor();
  const firstMembers = await group.locator("button").count();
  assert(firstMembers > 0 && firstMembers < 40, "Expanded members must remain virtualized");
  const memberHeight = await group.locator("button").first().evaluate(el =>
    el.parentElement.getBoundingClientRect().height,
  );
  const headerHeight = await titles.first().evaluate(el =>
    el.closest('[data-slot="item"]').getBoundingClientRect().height,
  );
  await viewport.evaluate((el, top) => { el.scrollTop = top - el.clientHeight; },
    headerHeight + nodeCount * memberHeight);
  await group.getByText(`Benchmark node ${nodeCount - 1}`, { exact: true }).waitFor();
  const lastMembers = await group.locator("button").count();
  assert(lastMembers > 0 && lastMembers < 40, "The last member must not require mounting every member");

  await viewport.evaluate(el => { el.scrollTop = el.scrollHeight; });
  await titles.filter({ hasText: `Benchmark node ${nodeCount - 1}` }).waitFor();
  const lastCount = await titles.count();
  const previousTop = await viewport.evaluate(el => el.scrollTop);
  assert(lastCount > 0 && lastCount < 25, "The bottom window must remain bounded");
  assert(previousTop > 0, "The test must reach the bottom of a scrollable list");

  await page.getByRole("button", { name: "Home", exact: true }).click();
  await viewport.waitFor({ state: "detached" });
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
