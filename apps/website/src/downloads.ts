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

export const CLI_TARGETS = [
  "darwin-aarch64",
  "windows-x86_64",
  "linux-x86_64",
  "linux-aarch64",
] as const;

export type CliTarget = (typeof CLI_TARGETS)[number];

export const CLI_SHELLS = ["unix", "windows"] as const;

export type CliShell = (typeof CLI_SHELLS)[number];

const RELEASE_BASE = "https://github.com/yilong-musk/ice-box/releases/download";
const RAW_MAIN_BASE = "https://raw.githubusercontent.com/yilong-musk/ice-box/main";

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

const CLI_FILENAME: Record<CliTarget, (version: string) => string> = {
  "darwin-aarch64": (version) => `ice-box-cli_${version}_darwin_aarch64.tar.gz`,
  "windows-x86_64": (version) => `ice-box-cli_${version}_windows_x86_64.zip`,
  "linux-x86_64": (version) => `ice-box-cli_${version}_linux_x86_64.tar.gz`,
  "linux-aarch64": (version) => `ice-box-cli_${version}_linux_aarch64.tar.gz`,
};

export const CLI_INSTALL_SCRIPT_URL: Record<CliShell, string> = {
  unix: `${RAW_MAIN_BASE}/scripts/install-cli.sh`,
  windows: `${RAW_MAIN_BASE}/scripts/install-cli.ps1`,
};

export const CLI_INSTALL_COMMAND: Record<CliShell, string> = {
  unix: `curl -fsSL ${CLI_INSTALL_SCRIPT_URL.unix} | bash`,
  windows: `irm ${CLI_INSTALL_SCRIPT_URL.windows} | iex`,
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

export function cliFilename(version: string, target: CliTarget): string {
  return CLI_FILENAME[target](bareVersion(version));
}

export function cliDownloadUrl(version: string, target: CliTarget): string {
  const bare = bareVersion(version);
  return `${RELEASE_BASE}/v${bare}/${cliFilename(bare, target)}`;
}

export function defaultCliShell(platform: InstallerPlatform | null): CliShell {
  return platform === "windows" ? "windows" : "unix";
}

export type LinuxArch = "amd64" | "arm64";

export interface UserAgentDataLike {
  getHighEntropyValues?: (
    hints: readonly string[],
  ) => Promise<{ architecture?: string }>;
}

export interface NavigatorLike {
  userAgent: string;
  userAgentData?: UserAgentDataLike;
}

/** Detect Linux CPU architecture synchronously from the browser user agent. */
export function detectLinuxArch(userAgent: string): LinuxArch {
  return /aarch64|arm64/i.test(userAgent) ? "arm64" : "amd64";
}

export interface LinuxInstallerVariantPair {
  appimage: "appimage-amd64" | "appimage-aarch64";
  deb: "deb-amd64" | "deb-arm64";
}

/** Map a Linux CPU architecture to its AppImage and .deb installer variants. */
export function linuxInstallerVariantsForArch(
  arch: LinuxArch,
): LinuxInstallerVariantPair {
  return arch === "arm64"
    ? { appimage: "appimage-aarch64", deb: "deb-arm64" }
    : { appimage: "appimage-amd64", deb: "deb-amd64" };
}

/** Map a Linux CPU architecture to its primary AppImage installer variant. */
export function linuxPrimaryInstallerVariant(
  arch: LinuxArch,
): "appimage-amd64" | "appimage-aarch64" {
  return linuxInstallerVariantsForArch(arch).appimage;
}

/**
 * Refine Linux CPU architecture via User-Agent Client Hints.
 *
 * Chromium freezes `navigator.userAgent` to `X11; Linux x86_64` even on arm64
 * hosts; `navigator.userAgentData.getHighEntropyValues(["architecture"])`
 * exposes the real architecture when supported. Falls back to the synchronous
 * UA detection when Client Hints are unavailable or rejected.
 */
export async function refineLinuxArch(nav: NavigatorLike): Promise<LinuxArch> {
  const fallback = detectLinuxArch(nav.userAgent);
  const getHints = nav.userAgentData?.getHighEntropyValues;
  if (typeof getHints !== "function") return fallback;
  try {
    const hints = await getHints.call(nav.userAgentData, ["architecture"]);
    const arch = hints?.architecture?.trim().toLowerCase();
    if (arch === "arm" || arch === "arm64" || arch === "aarch64") {
      return "arm64";
    }
    if (arch === "x86" || arch === "x86_64" || arch === "amd64") {
      return "amd64";
    }
  } catch {
    // Permission denied or unsupported hint: keep the synchronous UA fallback.
  }
  return fallback;
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
