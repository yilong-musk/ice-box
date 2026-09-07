// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { formatProgress } from "./UpdateAvailableDialog";

describe("formatProgress", () => {
  it("shows a percent when the content length is known", () => {
    expect(formatProgress(50, 100)).toBe("正在下载更新… 50%");
    expect(formatProgress(200, 100)).toBe("正在下载更新… 100%");
  });

  it("falls back to bytes, then the generic installing copy", () => {
    expect(formatProgress(12, null)).toBe("已下载 12 字节");
    expect(formatProgress(0, null)).toBe("正在下载更新…");
    expect(formatProgress(0, 0)).toBe("正在下载更新…");
  });
});
