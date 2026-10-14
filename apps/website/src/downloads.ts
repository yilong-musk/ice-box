// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Direct installer links for the marketing site.
 *
 * Filenames follow docs/release-process.md. Each URL is a GitHub Release
 * asset, which responds with Content-Disposition: attachment, so the browser
 * saves the file without opening the releases page.
 */

export const INSTALLER_PLATFORMS = ["macos", "windows", "android"] as const;

export type InstallerPlatform = (typeof INSTALLER_PLATFORMS)[number];

const RELEASE_BASE = "https://github.com/yilong-musk/ice-box/releases/download";

const FILENAME: Record<InstallerPlatform, (version: string) => string> = {
  macos: (version) => `ice-box_${version}_aarch64.dmg`,
  windows: (version) => `ice-box_${version}_x64-setup.exe`,
  android: (version) => `ice-box_${version}_android_arm64.apk`,
};

/** `v0.1.23` and `0.1.23` both become `0.1.23`. */
export function bareVersion(version: string): string {
  const bare = version.trim().replace(/^v/i, "");
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(bare)) {
    throw new Error(`Invalid ice-box version: ${version}`);
  }
  return bare;
}

export function installerFilename(
  version: string,
  platform: InstallerPlatform,
): string {
  return FILENAME[platform](bareVersion(version));
}

export function installerDownloadUrl(
  version: string,
  platform: InstallerPlatform,
): string {
  const bare = bareVersion(version);
  return `${RELEASE_BASE}/v${bare}/${installerFilename(bare, platform)}`;
}

/** The installer that matches this browser, when we ship one for it. */
export function detectPlatform(userAgent: string): InstallerPlatform | null {
  if (/Android/i.test(userAgent)) return "android";
  if (/iPhone|iPad|iPod/i.test(userAgent)) return null;
  if (/Windows/i.test(userAgent)) return "windows";
  if (/Macintosh|Mac OS X/i.test(userAgent)) return "macos";
  return null;
}
