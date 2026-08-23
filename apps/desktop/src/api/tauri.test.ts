import { describe, expect, it } from "vitest";
import { formatInvokeError } from "./tauri";

describe("formatInvokeError", () => {
  it("formats AppError payload", () => {
    expect(
      formatInvokeError({ code: "core.not_found", message: "missing binary" }),
    ).toBe("core.not_found: missing binary");
  });

  it("falls back to message field", () => {
    expect(formatInvokeError({ message: "network down" })).toBe("network down");
  });

  it("stringifies unknown values", () => {
    expect(formatInvokeError("boom")).toBe("boom");
  });
});
