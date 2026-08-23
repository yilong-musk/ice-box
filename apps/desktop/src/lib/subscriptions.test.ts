import { describe, expect, it } from "vitest";
import {
  extractApplyWarning,
  formatApplyWarning,
  isInsecureSubscriptionUrl,
} from "./subscriptions";

describe("isInsecureSubscriptionUrl", () => {
  it("flags http URLs", () => {
    expect(isInsecureSubscriptionUrl("http://example.com/sub")).toBe(true);
  });

  it("allows https URLs", () => {
    expect(isInsecureSubscriptionUrl("https://example.com/sub")).toBe(false);
  });
});

describe("apply warning helpers", () => {
  it("extracts apply_warning from mutation payload", () => {
    const w = extractApplyWarning({
      id: "x",
      apply_warning: { code: "core.invalid_state", message: "reload failed" },
    });
    expect(w).toEqual({ code: "core.invalid_state", message: "reload failed" });
    expect(formatApplyWarning(w!)).toBe("core.invalid_state: reload failed");
  });

  it("returns null when no warning", () => {
    expect(extractApplyWarning({ id: "x" })).toBeNull();
  });
});
