// SPDX-License-Identifier: GPL-3.0-or-later

import { beforeEach, describe, expect, it } from "vitest";
import { persistLanguagePreference, t } from "../../lib/i18n";
import { formatUpdateError } from "./Update";

describe("formatUpdateError", () => {
  beforeEach(() => {
    persistLanguagePreference("en");
  });

  it("maps a structured feed_unavailable payload to the catalog copy", () => {
    expect(
      formatUpdateError({
        code: "update.feed_unavailable",
        message: "Could not fetch a valid release JSON from the remote",
      }),
    ).toBe(t("settings.updateFeedUnavailable"));
  });

  it("maps a structured check_failed payload to the proxy hint", () => {
    expect(
      formatUpdateError({
        code: "update.check_failed",
        message: "error sending request for url",
      }),
    ).toBe(t("settings.updateCheckFailed"));
  });

  it("maps a code:message string the same way as a payload", () => {
    expect(
      formatUpdateError(
        "update.feed_unavailable: Could not fetch a valid release JSON from the remote",
      ),
    ).toBe(t("settings.updateFeedUnavailable"));
  });

  it("does not treat a mention of an update code as the error code", () => {
    expect(
      formatUpdateError("network error (docs mention update.feed_unavailable)"),
    ).toBe("network error (docs mention update.feed_unavailable)");
  });

  it("does not remap an already-translated formatInvokeError string", () => {
    const formatted =
      "Update catalog is not available yet (update.feed_unavailable): Could not fetch a valid release JSON from the remote";
    expect(formatUpdateError(formatted)).toBe(formatted);
  });
});
