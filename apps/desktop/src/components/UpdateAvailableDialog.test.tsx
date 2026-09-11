// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { t } from "@/lib/i18n";
import { formatProgress } from "./UpdateAvailableDialog";

describe("formatProgress", () => {
  it("shows a percent when the content length is known", () => {
    expect(formatProgress(50, 100)).toBe(t("update.progressPct", { pct: 50 }));
    expect(formatProgress(200, 100)).toBe(t("update.progressPct", { pct: 100 }));
  });

  it("falls back to bytes, then the generic installing copy", () => {
    expect(formatProgress(12, null)).toBe(t("update.progressBytes", { bytes: 12 }));
    expect(formatProgress(0, null)).toBe(t("update.installing"));
    expect(formatProgress(0, 0)).toBe(t("update.installing"));
  });
});
