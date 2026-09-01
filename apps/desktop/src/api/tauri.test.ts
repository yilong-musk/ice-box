import { describe, expect, it } from "vitest";
import { formatInvokeError } from "./tauri";

describe("formatInvokeError", () => {
  it("formats AppError payload", () => {
    expect(
      formatInvokeError({ code: "core.not_found", message: "missing binary" }),
    ).toBe("core.not_found: missing binary");
  });

  it("passes Rust-side error codes and messages through verbatim", () => {
    expect(
      formatInvokeError({
        code: "config.empty_outbounds",
        message: "no active subscription",
      }),
    ).toBe("config.empty_outbounds: no active subscription");
    expect(
      formatInvokeError({
        code: "tun.not_supported",
        message: "Windows TUN gate pending",
      }),
    ).toBe("tun.not_supported: Windows TUN gate pending");
    expect(
      formatInvokeError({
        code: "tun.recovery_required",
        message: "cleanup unverified",
      }),
    ).toContain("tun.recovery_required: cleanup unverified");
  });

  it("falls back to message field", () => {
    expect(formatInvokeError({ message: "network down" })).toBe("network down");
  });

  it("stringifies unknown values", () => {
    expect(formatInvokeError("boom")).toBe("boom");
  });
});
