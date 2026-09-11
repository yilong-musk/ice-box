// SPDX-License-Identifier: GPL-3.0-or-later

import { beforeEach, describe, expect, it } from "vitest";
import { persistLanguagePreference } from "../lib/i18n";
import { formatInvokeError } from "./tauri";

describe("formatInvokeError", () => {
  beforeEach(() => {
    persistLanguagePreference("en");
  });

  it("formats AppError payload", () => {
    expect(
      formatInvokeError({ code: "core.not_found", message: "missing binary" }),
    ).toBe("Core binary not found (core.not_found): missing binary");
  });

  it("localizes known codes and keeps the stable code", () => {
    expect(
      formatInvokeError({
        code: "config.empty_outbounds",
        message: "no active subscription",
      }),
    ).toBe("No usable outbounds (config.empty_outbounds): no active subscription");
    expect(
      formatInvokeError({
        code: "tun.not_supported",
        message: "Windows TUN gate pending",
      }),
    ).toBe(
      "TUN is not supported on this platform (tun.not_supported): Windows TUN gate pending",
    );
    expect(
      formatInvokeError({
        code: "tun.recovery_required",
        message: "cleanup unverified",
      }),
    ).toBe("TUN must be recovered first (tun.recovery_required): cleanup unverified");
    expect(
      formatInvokeError({
        code: "tun.helper_install_failed",
        message: "the TUN launcher exited with status 2: schtasks /Create /XML exited 1",
      }),
    ).toBe(
      "Failed to install the helper (tun.helper_install_failed): the TUN launcher exited with status 2: schtasks /Create /XML exited 1",
    );
    expect(
      formatInvokeError({
        code: "core.not_found",
        message: "Core binary not found",
      }),
    ).toBe("Core binary not found (core.not_found)");
  });

  it("falls back to message field", () => {
    expect(formatInvokeError({ message: "network down" })).toBe("network down");
  });

  it("stringifies unknown values", () => {
    expect(formatInvokeError("boom")).toBe("boom");
  });
});
