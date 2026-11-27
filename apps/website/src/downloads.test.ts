// SPDX-License-Identifier: GPL-3.0-or-later

import assert from "node:assert/strict";
import test from "node:test";
import {
  detectLinuxArch,
  detectPlatform,
  installerDownloadUrl,
  installerFilename,
  linuxInstallerDownloadUrl,
  linuxInstallerFilename,
  linuxInstallerVariantsForArch,
  linuxPrimaryInstallerVariant,
  refineLinuxArch,
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
    installerDownloadUrl("0.1.23", "linux"),
    "https://github.com/yilong-musk/ice-box/releases/download/v0.1.23/ice-box_0.1.23_amd64.AppImage",
  );
  assert.equal(
    installerDownloadUrl("0.1.23", "android"),
    "https://github.com/yilong-musk/ice-box/releases/download/v0.1.23/ice-box_0.1.23_android_arm64.apk",
  );
});

test("linux installer variants match all four published release packages", () => {
  assert.equal(
    linuxInstallerFilename("v0.4.0", "appimage-amd64"),
    "ice-box_0.4.0_amd64.AppImage",
  );
  assert.equal(
    linuxInstallerDownloadUrl("0.4.0", "deb-amd64"),
    "https://github.com/yilong-musk/ice-box/releases/download/v0.4.0/ice-box_0.4.0_amd64.deb",
  );
  assert.equal(
    linuxInstallerDownloadUrl("0.4.0", "appimage-aarch64"),
    "https://github.com/yilong-musk/ice-box/releases/download/v0.4.0/ice-box_0.4.0_aarch64.AppImage",
  );
  assert.equal(
    linuxInstallerDownloadUrl("0.4.0", "deb-arm64"),
    "https://github.com/yilong-musk/ice-box/releases/download/v0.4.0/ice-box_0.4.0_arm64.deb",
  );
  assert.deepEqual(linuxInstallerVariantsForArch("amd64"), {
    appimage: "appimage-amd64",
    deb: "deb-amd64",
  });
  assert.deepEqual(linuxInstallerVariantsForArch("arm64"), {
    appimage: "appimage-aarch64",
    deb: "deb-arm64",
  });
  assert.equal(
    linuxPrimaryInstallerVariant("amd64"),
    linuxInstallerVariantsForArch("amd64").appimage,
  );
  assert.equal(
    linuxPrimaryInstallerVariant("arm64"),
    linuxInstallerVariantsForArch("arm64").appimage,
  );
});

test("rejects a version that could change the download path", () => {
  assert.throws(() => installerDownloadUrl("../secret", "macos"));
  assert.throws(() => installerDownloadUrl("0.1", "windows"));
  assert.throws(() => linuxInstallerDownloadUrl("../secret", "deb-amd64"));
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
    detectPlatform("Mozilla/5.0 (X11; Linux x86_64)"),
    "linux",
  );
  assert.equal(
    detectPlatform("Mozilla/5.0 (X11; Linux aarch64)"),
    "linux",
  );
  assert.equal(
    detectPlatform(
      "Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36",
    ),
    null,
  );
  assert.equal(
    detectPlatform(
      "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit",
    ),
    null,
  );
});

test("detectLinuxArch distinguishes amd64 and arm64", () => {
  assert.equal(detectLinuxArch("Mozilla/5.0 (X11; Linux x86_64)"), "amd64");
  assert.equal(detectLinuxArch("Mozilla/5.0 (X11; Linux aarch64)"), "arm64");
  assert.equal(detectLinuxArch("Mozilla/5.0 (X11; Linux arm64)"), "arm64");
});

test("refineLinuxArch resolves arm64 on Chromium with a frozen x86_64 UA", async () => {
  const frozenUa =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
  assert.equal(detectLinuxArch(frozenUa), "amd64");

  const uaData = {
    async getHighEntropyValues(hints: readonly string[]) {
      assert.equal(this, uaData);
      assert.deepEqual(hints, ["architecture"]);
      return { architecture: "arm" };
    },
  };
  const arch = await refineLinuxArch({
    userAgent: frozenUa,
    userAgentData: uaData,
  });
  assert.equal(arch, "arm64");
  assert.equal(linuxPrimaryInstallerVariant(arch), "appimage-aarch64");
  assert.equal(
    linuxInstallerDownloadUrl("0.5.0", linuxPrimaryInstallerVariant(arch)),
    "https://github.com/yilong-musk/ice-box/releases/download/v0.5.0/ice-box_0.5.0_aarch64.AppImage",
  );

  assert.equal(
    await refineLinuxArch({
      userAgent: frozenUa,
      userAgentData: {
        getHighEntropyValues: async () => ({ architecture: "arm64" }),
      },
    }),
    "arm64",
  );
  assert.equal(
    await refineLinuxArch({
      userAgent: frozenUa,
      userAgentData: {
        getHighEntropyValues: async () => ({ architecture: "x86" }),
      },
    }),
    "amd64",
  );
});

test("refineLinuxArch keeps sync fallback when userAgentData is absent or rejects", async () => {
  assert.equal(
    await refineLinuxArch({ userAgent: "Mozilla/5.0 (X11; Linux x86_64)" }),
    "amd64",
  );
  assert.equal(
    await refineLinuxArch({ userAgent: "Mozilla/5.0 (X11; Linux aarch64)" }),
    "arm64",
  );
  assert.equal(
    await refineLinuxArch({
      userAgent: "Mozilla/5.0 (X11; Linux aarch64)",
      userAgentData: {
        getHighEntropyValues: async () => {
          throw new Error("denied");
        },
      },
    }),
    "arm64",
  );
  assert.equal(
    await refineLinuxArch({
      userAgent: "Mozilla/5.0 (X11; Linux x86_64)",
      userAgentData: {
        getHighEntropyValues: async () => ({ architecture: "" }),
      },
    }),
    "amd64",
  );
});

test("detectPlatform does not take an iPad in desktop mode for a Mac", () => {
  const desktopSafari =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";
  assert.equal(detectPlatform(desktopSafari, 5), null);
  assert.equal(detectPlatform(desktopSafari, 0), "macos");
});
