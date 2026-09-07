import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { version as packageVersion } from "../../package.json";
import { APP_VERSION } from "./appVersion";

describe("APP_VERSION", () => {
  it("matches the desktop package version", () => {
    expect(APP_VERSION).toBe(packageVersion);
    expect(APP_VERSION).toMatch(/^\d+\.\d+\.\d+/);
  });

  it("is imported by the website marketing chrome instead of a hardcoded string", () => {
    const websiteMain = readFileSync(
      resolve(dirname(fileURLToPath(import.meta.url)), "../../../website/src/main.js"),
      "utf8",
    );
    expect(websiteMain).toMatch(/from ["']\.\.\/\.\.\/desktop\/package\.json["']/);
    expect(websiteMain).not.toMatch(/\bv\d+\.\d+\.\d+\b/);
  });
});
