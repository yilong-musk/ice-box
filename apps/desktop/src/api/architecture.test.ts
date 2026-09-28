// SPDX-License-Identifier: GPL-3.0-or-later

import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { describe, expect, it } from "vitest";

const desktop = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap(entry => {
    const file = path.join(dir, entry.name);
    return entry.isDirectory() ? sources(file) : [file];
  });
}

describe("application architecture boundaries", () => {
  it("keeps shared views independent of the desktop transport", () => {
    const violations = sources(path.join(desktop, "src"))
      .filter(file => /[.](ts|tsx)$/.test(file) && !file.includes(".test."))
      .filter(file => readFileSync(file, "utf8").includes("api/tauri"));
    expect(violations).toEqual([]);
  });

  it("keeps application use cases independent of Tauri and IPC commands", () => {
    const violations = sources(path.join(desktop, "src-tauri/src/application"))
      .filter(file => file.endsWith(".rs"))
      .filter(file => /tauri::|crate::commands/.test(readFileSync(file, "utf8")));
    expect(violations).toEqual([]);
  });

  it("declares a transport-neutral API instead of inferring it from Tauri", () => {
    const contract = readFileSync(path.join(desktop, "src/api/contracts.ts"), "utf8");
    expect(contract).toContain("export interface ApiContract");
    expect(contract).not.toContain("@tauri-apps");
    expect(contract).not.toContain("typeof import");
    const website = readFileSync(path.join(desktop, "../website/vite.config.ts"), "utf8");
    expect(website).toContain('find: "@platform/api"');
    expect(website).not.toContain("resolveId");
  });
});
