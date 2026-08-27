import { describe, expect, it } from "vitest";
import { version as packageVersion } from "../../package.json";
import { APP_VERSION } from "./appVersion";

describe("APP_VERSION", () => {
  it("matches the desktop package version", () => {
    expect(APP_VERSION).toBe(packageVersion);
    expect(APP_VERSION).toMatch(/^\d+\.\d+\.\d+/);
  });
});
