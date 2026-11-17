// SPDX-License-Identifier: GPL-3.0-or-later

import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

const { api } = await import("./tauri");

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue({});
});

describe("addSubscription", () => {
  it("sends the kept interval when auto-update starts off", async () => {
    await api.addSubscription("https://example.com/a", undefined, false, "twelve_hours");

    // The backend only clears the flag, so the interval must reach it even
    // while auto-update is off: it is what a later enable restores.
    expect(invoke).toHaveBeenCalledWith("add_subscription", {
      req: {
        url: "https://example.com/a",
        name: null,
        auto_update: false,
        auto_update_interval: "twelve_hours",
      },
    });
  });

  it("defaults the interval instead of sending null", async () => {
    await api.addSubscription("https://example.com/a");

    expect(invoke).toHaveBeenCalledWith("add_subscription", {
      req: {
        url: "https://example.com/a",
        name: null,
        auto_update: false,
        auto_update_interval: "one_hour",
      },
    });
  });

  it("sends the chosen interval when auto-update starts on", async () => {
    await api.addSubscription("https://example.com/a", "a", true, "six_hours");

    expect(invoke).toHaveBeenCalledWith("add_subscription", {
      req: {
        url: "https://example.com/a",
        name: "a",
        auto_update: true,
        auto_update_interval: "six_hours",
      },
    });
  });
});

describe("setSubscriptionAutoUpdate", () => {
  it("always sends the interval with the flag", async () => {
    await api.setSubscriptionAutoUpdate("11111111-1111-1111-1111-111111111111", false, "six_hours");

    expect(invoke).toHaveBeenCalledWith("set_auto_update_subscription", {
      req: {
        id: "11111111-1111-1111-1111-111111111111",
        auto_update: false,
        auto_update_interval: "six_hours",
      },
    });
  });
});
