import { describe, expect, it } from "vitest";
import { formatInvokeError } from "./tauri";

describe("formatInvokeError", () => {
  it("formats AppError payload", () => {
    expect(
      formatInvokeError({ code: "core.not_found", message: "missing binary" }),
    ).toBe("core.not_found: missing binary");
  });

  it("translates known friendly codes", () => {
    expect(
      formatInvokeError({
        code: "config.empty_outbounds",
        message: "no active subscription",
      }),
    ).toBe("没有可用的订阅节点，请先在「订阅」页导入订阅，或保持仅直连模式运行");
  });

  it("falls back to message field", () => {
    expect(formatInvokeError({ message: "network down" })).toBe("network down");
  });

  it("stringifies unknown values", () => {
    expect(formatInvokeError("boom")).toBe("boom");
  });
});
