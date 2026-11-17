// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { t } from "./i18n";
import {
  AUTO_UPDATE_INTERVALS,
  AUTO_UPDATE_NEVER,
  autoUpdateChoiceValue,
  autoUpdateNeverLabel,
  intervalLabel,
  parseAutoUpdateChoice,
  retainedAutoUpdateInterval,
} from "./subscriptionAutoUpdate";

describe("autoUpdateChoiceValue", () => {
  it("shows the stored interval while auto-update is on", () => {
    expect(
      autoUpdateChoiceValue({ auto_update: true, auto_update_interval: "six_hours" }),
    ).toBe("six_hours");
  });

  it("falls back to the default interval when an old entry has none", () => {
    expect(
      autoUpdateChoiceValue({ auto_update: true, auto_update_interval: null }),
    ).toBe("one_hour");
  });

  it("shows Never whenever auto-update is off", () => {
    expect(
      autoUpdateChoiceValue({ auto_update: false, auto_update_interval: "six_hours" }),
    ).toBe(AUTO_UPDATE_NEVER);
  });
});

describe("parseAutoUpdateChoice", () => {
  it("maps Never to auto-update off", () => {
    expect(parseAutoUpdateChoice("never")).toEqual({ autoUpdate: false });
  });

  it("maps every listed interval to auto-update on", () => {
    for (const interval of AUTO_UPDATE_INTERVALS) {
      expect(parseAutoUpdateChoice(interval)).toEqual({
        autoUpdate: true,
        interval,
      });
    }
  });

  it("rejects a value the list does not offer", () => {
    expect(parseAutoUpdateChoice("two_hours")).toBeNull();
    expect(parseAutoUpdateChoice("")).toBeNull();
  });
});

describe("retainedAutoUpdateInterval", () => {
  it("keeps the interval a subscription still stores so turning it back on restores it", () => {
    expect(
      retainedAutoUpdateInterval({
        auto_update: false,
        auto_update_interval: "twelve_hours",
      }),
    ).toBe("twelve_hours");
  });

  it("uses the default when nothing was stored", () => {
    expect(
      retainedAutoUpdateInterval({ auto_update: false, auto_update_interval: null }),
    ).toBe("one_hour");
  });
});

describe("option labels", () => {
  it("reads Never and the intervals from the dictionary", () => {
    expect(autoUpdateNeverLabel()).toBe(t("subs.autoUpdateNever"));
    expect(intervalLabel("three_hours")).toBe(t("subs.interval.three_hours"));
  });
});
