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
    expect(contract).toContain("export interface CoreApi");
    expect(contract).toContain("export interface DesktopCaptureApi");
    expect(contract).toContain("export interface DesktopShellApi");
    expect(contract).toContain(
      "export interface ApiContract extends CoreApi, DesktopCaptureApi, DesktopShellApi",
    );
    expect(contract).not.toContain("@tauri-apps");
    expect(contract).not.toContain("typeof import");
    const website = readFileSync(path.join(desktop, "../website/vite.config.ts"), "utf8");
    expect(website).toContain('find: "@platform/api"');
    expect(website).toContain('find: "@platform/shell"');
    expect(website).not.toContain("resolveId");
    const desktopVite = readFileSync(path.join(desktop, "vite.config.ts"), "utf8");
    expect(desktopVite).toContain('"@platform/shell"');
  });

  it("gates product behavior on status flags instead of the user agent", () => {
    const roots = ["pages", "components"].map(dir => path.join(desktop, "src", dir));
    const violations = roots.flatMap(sources).filter(file => {
      if (!/[.](ts|tsx)$/.test(file) || file.includes(".test.")) return false;
      return /userAgent|navigator\.platform/.test(readFileSync(file, "utf8"));
    });
    expect(violations).toEqual([]);
  });

  it("keeps ice-app free of Tauri and desktop capture crates", () => {
    const manifest = readFileSync(
      path.resolve(desktop, "../../crates/ice-app/Cargo.toml"),
      "utf8",
    );
    for (const forbidden of ["tauri", "ice-proxy-sys", "ice-tun", "ice-helper", "ice-elevate"]) {
      expect(manifest).not.toContain(forbidden);
    }
  });
});
