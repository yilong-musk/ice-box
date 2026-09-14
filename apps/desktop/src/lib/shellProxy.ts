// SPDX-License-Identifier: GPL-3.0-or-later

import { detectWindowChrome } from "./windowChrome";
import { normalizeListenHost } from "./listenValidation";

export type ShellProxyPlatform = "windows" | "posix";

/** Windows gets PowerShell session env vars; macOS and Linux get POSIX `export`. */
export function detectShellProxyPlatform(
  userAgent = typeof navigator === "undefined" ? "" : navigator.userAgent,
  platform = typeof navigator === "undefined" ? "" : navigator.platform,
): ShellProxyPlatform {
  return detectWindowChrome(userAgent, platform) === "windows-custom"
    ? "windows"
    : "posix";
}

/** Bind address for a local terminal: unspecified listens become loopback. */
export function shellProxyHost(listen: string): string {
  const host = normalizeListenHost(listen);
  if (host === "" || host === "0.0.0.0" || host === "::") {
    return "127.0.0.1";
  }
  return host;
}

export function shellProxyHttpUrl(host: string, port: number): string {
  const wrapped = host.includes(":") && !host.startsWith("[") ? `[${host}]` : host;
  return `http://${wrapped}:${port}`;
}

export function shellProxySocksUrl(host: string, port: number): string {
  const wrapped = host.includes(":") && !host.startsWith("[") ? `[${host}]` : host;
  return `socks5://${wrapped}:${port}`;
}

export function resolveShellProxyEndpoint(
  settings: { mixed_listen: string; mixed_port: number } | null,
  inbound?: { host: string | null; port: number | null },
): { host: string; port: number } | null {
  const port = inbound?.port ?? settings?.mixed_port;
  if (port === undefined || port === null || !Number.isFinite(port) || port <= 0) {
    return null;
  }
  const rawHost = inbound?.host || settings?.mixed_listen || "127.0.0.1";
  return { host: shellProxyHost(rawHost), port };
}

/**
 * One line for the *current terminal session only*.
 *
 * POSIX uses shell `export` (process env). Windows uses `$env:` (process env).
 * Do not use `setx`, `[Environment]::SetEnvironmentVariable(..., User|Machine)`,
 * or profile edits — those would persist beyond this window.
 */
export function formatShellProxyCommand(
  platform: ShellProxyPlatform,
  host: string,
  port: number,
): string {
  const http = shellProxyHttpUrl(host, port);
  const socks = shellProxySocksUrl(host, port);
  const noProxy = "localhost,127.0.0.1,::1";
  if (platform === "windows") {
    // Process-scoped only; closes with this PowerShell window.
    return [
      `$env:HTTP_PROXY='${http}'`,
      `$env:HTTPS_PROXY='${http}'`,
      `$env:ALL_PROXY='${socks}'`,
      `$env:NO_PROXY='${noProxy}'`,
    ].join("; ");
  }
  // Shell-session only; does not write ~/.bashrc or system env files.
  return [
    `export http_proxy=${http}`,
    `https_proxy=${http}`,
    `all_proxy=${socks}`,
    `no_proxy=${noProxy}`,
  ].join(" ");
}

export async function copyText(text: string): Promise<boolean> {
  try {
    if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // Fall through to execCommand for webviews that deny the Clipboard API.
  }
  if (typeof document === "undefined") return false;
  try {
    const el = document.createElement("textarea");
    el.value = text;
    el.setAttribute("readonly", "");
    el.style.position = "fixed";
    el.style.left = "-9999px";
    document.body.appendChild(el);
    el.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(el);
    return ok;
  } catch {
    return false;
  }
}
