// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { t } from "./i18n";
import {
  extractApplyWarning,
  formatApplyWarning,
  isInsecureSubscriptionUrl,
  subscriptionTrafficView,
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
    expect(formatApplyWarning(w!)).toBe(
      `${t("error.core.invalid_state")} (core.invalid_state): reload failed`,
    );
  });

  it("returns null when no warning", () => {
    expect(extractApplyWarning({ id: "x" })).toBeNull();
  });
});

describe("subscriptionTrafficView", () => {
  const now = new Date(2026, 8, 1, 12, 0, 0).getTime();

  /** Flower: `Traffic: 11.84 GB | 150 GB` / `Expire: 2026-09-26`. */
  it("parses used / total and a dated expiry out of the proxy list", () => {
    const view = subscriptionTrafficView(null, [
      "Traffic: 11.84 GB | 150 GB",
      "Expire: 2026-09-26",
    ], now);
    expect(view?.usage).toBe(
      t("subs.trafficUsedTotal", {
        used: "11.84GB",
        total: "150GB",
        percent: "7.89",
      }),
    );
    expect(view?.expiry).toBe(t("subs.trafficExpire", { date: "2026-09-26" }));
    expect(view?.expired).toBe(false);
  });

  /** LiangXin: counters in the header, `剩余流量` / `套餐到期：长期有效` in the body. */
  it("merges header counters with a remaining-only entry and a long-term expiry", () => {
    const view = subscriptionTrafficView(
      {
        upload: 7_118_713,
        download: 383_477_544,
        total: 1_099_511_627_776,
        expire: null,
      },
      ["剩余流量：1023.64 GB", "套餐到期：长期有效"],
      now,
    );
    expect(view?.usage).toBe(
      t("subs.trafficUsedTotal", {
        used: "372.5MB",
        total: "1TB",
        percent: "0.04",
      }),
    );
    expect(view?.expiry).toBe(t("subs.trafficNeverExpires"));
    expect(view?.expired).toBe(false);
  });

  it("falls back to the side the provider reports alone", () => {
    const remainingOnly = subscriptionTrafficView(null, ["剩余流量：1023.64 GB"], now);
    expect(remainingOnly?.usage).toBe(
      t("subs.trafficRemaining", { remaining: "1023.64GB" }),
    );
    const totalOnly = subscriptionTrafficView(null, ["套餐流量：100 GB"], now);
    expect(totalOnly?.usage).toBe(t("subs.trafficTotal", { total: "100GB" }));
    expect(totalOnly?.expiry).toBeNull();
  });

  it("marks a past expiry", () => {
    const view = subscriptionTrafficView(
      null,
      ["Traffic: 1 GB | 100 GB", "Expire: 2026-01-01"],
      now,
    );
    expect(view?.expired).toBe(true);
    expect(view?.expiry).toBe(t("subs.trafficExpired", { date: "2026-01-01" }));
  });

  it("reports an overshoot past the provider's own quota", () => {
    const view = subscriptionTrafficView(
      { upload: 10, download: 0, total: 5 },
      null,
      now,
    );
    expect(view?.usage).toBe(
      t("subs.trafficUsedTotal", {
        used: "10B",
        total: "5B",
        percent: "200",
      }),
    );
  });

  it("reads the second amount of a used-labelled entry as the total", () => {
    const view = subscriptionTrafficView(null, ["Used: 11.84 GB | 150 GB"], now);
    expect(view?.usage).toBe(
      t("subs.trafficUsedTotal", {
        used: "11.84GB",
        total: "150GB",
        percent: "7.89",
      }),
    );
  });

  it("derives usage from the entries when the header only reports a quota", () => {
    const view = subscriptionTrafficView(
      { upload: 0, download: 0, total: 500 * 1024 ** 3, expire: null },
      ["剩余流量：499 GB"],
      now,
    );
    expect(view?.usage).toBe(
      t("subs.trafficUsedTotal", {
        used: "1GB",
        total: "500GB",
        percent: "0.2",
      }),
    );
  });

  it("ignores entries it cannot read", () => {
    expect(
      subscriptionTrafficView(null, ["距离下次重置剩余：19 天"], now),
    ).toBeNull();
    expect(subscriptionTrafficView(null, [], now)).toBeNull();
  });
});
