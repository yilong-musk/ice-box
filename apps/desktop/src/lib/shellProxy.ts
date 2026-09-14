// SPDX-License-Identifier: GPL-3.0-or-later

import { normalizeListenHost } from "./listenValidation";

/** Bind address for a local terminal: unspecified listens become loopback. */
export function shellProxyHost(listen: string): string {
  const host = normalizeListenHost(listen);
  if (host === "" || host === "0.0.0.0" || host === "::") {
    return "127.0.0.1";
  }
  return host;
}

/** Longest host accepted before it is embedded in a generated shell command. */
const MAX_SHELL_PROXY_HOST_LEN = 255;

/**
 * True when `host` can be embedded in a generated shell command verbatim.
 *
 * The allow-list covers IPv4, bracketed IPv6, and hostnames; every accepted
 * character is inert in POSIX shells, `cmd.exe`, PowerShell, and AppleScript
 * string literals. Mirrors `proxy_terminal::is_safe_shell_proxy_host` in the
 * desktop backend, which also gates the tray items.
 */
export function isSafeShellProxyHost(host: string): boolean {
  if (host.length === 0 || host.length > MAX_SHELL_PROXY_HOST_LEN) return false;
  return /^[0-9A-Za-z._:\[\]-]+$/.test(host);
}

/**
 * Mixed endpoint the session helpers would use, or `null` when the card must
 * not offer them.
 *
 * The port falls back to the saved settings while the proxy service is
 * stopped: both controls stay available on purpose, so a command can be
 * prepared before the service starts. The command text itself is built in Rust
 * (`copy_proxy_command` / `open_proxy_terminal`); the UI only decides whether
 * to show the buttons.
 */
export function resolveShellProxyEndpoint(
  settings: { mixed_listen: string; mixed_port: number } | null,
  inbound?: { host: string | null; port: number | null },
): { host: string; port: number } | null {
  const port = inbound?.port ?? settings?.mixed_port;
  if (port === undefined || port === null || !Number.isFinite(port) || port <= 0) {
    return null;
  }
  const rawHost = inbound?.host || settings?.mixed_listen || "127.0.0.1";
  const host = shellProxyHost(rawHost);
  // `mixed_listen` is unvalidated while Allow LAN is on, so an unusable host
  // hides the controls instead of producing a command that could inject shell
  // syntax (the backend rejects the same host for the tray and the terminal).
  if (!isSafeShellProxyHost(host)) return null;
  return { host, port };
}
