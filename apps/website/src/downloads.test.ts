// SPDX-License-Identifier: GPL-3.0-or-later

import assert from "node:assert/strict";
import test from "node:test";
import {
  detectPlatform,
  installerDownloadUrl,
  installerFilename,
} from "./downloads.ts";

test("installer urls are the published release assets", () => {
  assert.equal(
    installerFilename("v0.1.23", "macos"),
    "ice-box_0.1.23_aarch64.dmg",
  );
  assert.equal(
    installerDownloadUrl("0.1.23", "macos"),
    "https://github.com/yilong-musk/ice-box/releases/download/v0.1.23/ice-box_0.1.23_aarch64.dmg",
  );
  assert.equal(
    installerDownloadUrl("0.1.23", "windows"),
    "https://github.com/yilong-musk/ice-box/releases/download/v0.1.23/ice-box_0.1.23_x64-setup.exe",
  );
  assert.equal(
    installerDownloadUrl("0.1.23", "android"),
    "https://github.com/yilong-musk/ice-box/releases/download/v0.1.23/ice-box_0.1.23_android_arm64.apk",
  );
});

test("rejects a version that could change the download path", () => {
  assert.throws(() => installerDownloadUrl("../secret", "macos"));
  assert.throws(() => installerDownloadUrl("0.1", "windows"));
});

test("detectPlatform picks the installer we ship", () => {
  assert.equal(
    detectPlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64)"),
    "windows",
  );
  assert.equal(
    detectPlatform("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)"),
    "macos",
  );
  assert.equal(
    detectPlatform("Mozilla/5.0 (Linux; Android 14; Pixel 8)"),
    "android",
  );
  assert.equal(
    detectPlatform(
      "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit",
    ),
    null,
  );
  assert.equal(detectPlatform("Mozilla/5.0 (X11; Linux x86_64)"), null);
});
