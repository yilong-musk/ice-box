// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Direct installer links for the marketing site.
 *
 * Filenames follow docs/release-process.md. Each URL is a GitHub Release
 * asset, which responds with Content-Disposition: attachment, so the browser
 * saves the file without opening the releases page.
 */

export const INSTALLER_PLATFORMS = ["macos", "windows", "linux", "android"] as const;

export type InstallerPlatform = (typeof INSTALLER_PLATFORMS)[number];

export const LINUX_INSTALLER_VARIANTS = [
  "appimage-amd64",
  "deb-amd64",
  "appimage-aarch64",
  "deb-arm64",
] as const;

export type LinuxInstallerVariant = (typeof LINUX_INSTALLER_VARIANTS)[number];

const RELEASE_BASE = "https://github.com/yilong-musk/ice-box/releases/download";

const FILENAME: Record<InstallerPlatform, (version: string) => string> = {
  macos: (version) => `ice-box_${version}_aarch64.dmg`,
  windows: (version) => `ice-box_${version}_x64-setup.exe`,
  linux: (version) => `ice-box_${version}_amd64.AppImage`,
  android: (version) => `ice-box_${version}_android_arm64.apk`,
};

const LINUX_VARIANT_FILENAME: Record<LinuxInstallerVariant, (version: string) => string> = {
  "appimage-amd64": (version) => `ice-box_${version}_amd64.AppImage`,
  "deb-amd64": (version) => `ice-box_${version}_amd64.deb`,
  "appimage-aarch64": (version) => `ice-box_${version}_aarch64.AppImage`,
  "deb-arm64": (version) => `ice-box_${version}_arm64.deb`,
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

export function linuxInstallerFilename(
  version: string,
  variant: LinuxInstallerVariant,
): string {
  return LINUX_VARIANT_FILENAME[variant](bareVersion(version));
}

export function linuxInstallerDownloadUrl(
  version: string,
  variant: LinuxInstallerVariant,
): string {
  const bare = bareVersion(version);
  return `${RELEASE_BASE}/v${bare}/${linuxInstallerFilename(bare, variant)}`;
}

/** Detect Linux CPU architecture from the browser user agent. */
export function detectLinuxArch(userAgent: string): "amd64" | "arm64" {
  return /aarch64|arm64/i.test(userAgent) ? "arm64" : "amd64";
}

/**
 * The installer that matches this browser, when we ship one for it. The hero
 * button downloads it directly, so a device we have no installer for must
 * come back null. iPadOS Safari asks for desktop sites with a Mac user agent;
 * a touch screen (`maxTouchPoints`) tells it apart from a Mac.
 */
export function detectPlatform(
  userAgent: string,
  maxTouchPoints = 0,
): InstallerPlatform | null {
  if (/Android/i.test(userAgent)) return "android";
  if (/iPhone|iPad|iPod/i.test(userAgent)) return null;
  if (/Windows/i.test(userAgent)) return "windows";
  if (/Macintosh|Mac OS X/i.test(userAgent)) return maxTouchPoints > 1 ? null : "macos";
  if (/Linux/i.test(userAgent) && !/CrOS/i.test(userAgent)) return "linux";
  return null;
}
