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

  it("translates TUN error codes to actionable text", () => {
    expect(
      formatInvokeError({
        code: "tun.not_supported",
        message: "Windows TUN gate pending",
      }),
    ).toBe("当前平台暂不支持 TUN 模式");
    expect(
      formatInvokeError({
        code: "tun.permission_required",
        message: "sudo -n failed",
      }),
    ).toContain("需要系统权限");
    expect(
      formatInvokeError({
        code: "tun.recovery_required",
        message: "cleanup unverified",
      }),
    ).toContain("重试恢复");
  });

  it("falls back to message field", () => {
    expect(formatInvokeError({ message: "network down" })).toBe("network down");
  });

  it("stringifies unknown values", () => {
    expect(formatInvokeError("boom")).toBe("boom");
  });
});
