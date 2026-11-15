// SPDX-License-Identifier: GPL-3.0-or-later

import type { ConnectionRow } from "../api/contracts";

export type ConnectionSort = "newest" | "total" | "download" | "upload";

/** Sort orders in the order the picker lists them; the first is the default. */
export const CONNECTION_SORTS: readonly ConnectionSort[] = [
  "newest",
  "total",
  "download",
  "upload",
];

export const CONNECTION_SORT_STORAGE_KEY = "ice-box.connections.sort";

function isConnectionSort(value: unknown): value is ConnectionSort {
  return CONNECTION_SORTS.includes(value as ConnectionSort);
}

/** The sort order the viewer last picked; newest when nothing valid is stored
 * or storage is unavailable. */
export function readConnectionSort(): ConnectionSort {
  try {
    const raw = window.localStorage.getItem(CONNECTION_SORT_STORAGE_KEY);
    if (isConnectionSort(raw)) return raw;
  } catch {
    // Fall through to the default.
  }
  return CONNECTION_SORTS[0];
}

/** Remember the picked sort order across restarts. */
export function writeConnectionSort(sort: ConnectionSort): void {
  try {
    window.localStorage.setItem(CONNECTION_SORT_STORAGE_KEY, sort);
  } catch {
    // Storage is a convenience only: the page works without it.
  }
}

/** Narrow a raw picker value to a sort order, or null when it is not one. */
export function parseConnectionSort(value: string): ConnectionSort | null {
  return isConnectionSort(value) ? value : null;
}

/**
 * Prefill for a new custom rule taken from a connection's destination.
 * Outbound is omitted so the form keeps its default exit.
 */
export type ConnectionRuleDraft = {
  matcherKey: "domain_suffix" | "ip_cidr";
  text: string;
};

const IPV4_OCTET = /^(?:0|[1-9]\d{0,2})$/;

function isIPv4(value: string): boolean {
  const parts = value.split(".");
  if (parts.length !== 4) return false;
  return parts.every((part) => IPV4_OCTET.test(part) && Number(part) <= 255);
}

/** `::1`, `2001:db8::1`, and IPv4-mapped tails such as `::ffff:192.0.2.1`. */
function isIPv6(value: string): boolean {
  if (!value.includes(":") || value.split("::").length > 2) return false;
  if (value === "::") return true;
  const halves = value.split("::");
  const groups = halves.flatMap((half) => (half === "" ? [] : half.split(":")));
  if (groups.length === 0 || groups.some((group) => group === "")) return false;
  if (halves.length === 1 && groups.length !== 8) return false;
  if (halves.length === 2 && groups.length > 7) return false;
  return groups.every((group, index) => {
    if (index === groups.length - 1 && group.includes(".")) return isIPv4(group);
    return /^[0-9a-fA-F]{1,4}$/.test(group);
  });
}

/**
 * Host prefix for an address. sing-box `ip_cidr` is a prefix, so a bare IP
 * would be rejected; `/32` and `/128` match that one address.
 */
function toIpCidr(raw: string): string | null {
  let value = raw.trim();
  if (value.startsWith("[") && value.endsWith("]")) value = value.slice(1, -1);
  const zone = value.indexOf("%");
  if (zone !== -1) value = value.slice(0, zone);
  if (!value) return null;

  const slash = value.indexOf("/");
  const addr = slash === -1 ? value : value.slice(0, slash);
  const prefix = slash === -1 ? "" : value.slice(slash + 1);
  if (prefix !== "" && !/^\d+$/.test(prefix)) return null;
  const bits = prefix === "" ? null : Number(prefix);

  if (isIPv4(addr)) {
    if (bits !== null && bits > 32) return null;
    return `${addr}/${bits ?? 32}`;
  }
  if (isIPv6(addr)) {
    if (bits !== null && bits > 128) return null;
    return `${addr.toLowerCase()}/${bits ?? 128}`;
  }
  return null;
}

/**
 * Matcher prefill from the destination address type.
 *
 * A hostname becomes a `domain_suffix` rule. An IP (in `host` or
 * `destination_ip`) becomes `ip_cidr`. The port is not part of the address,
 * and the connection's outbound is left for the form default.
 */
export function connectionRuleDraft(row: ConnectionRow): ConnectionRuleDraft | null {
  const host = row.host.trim().replace(/\.+$/, "");
  if (host) {
    const cidr = toIpCidr(host);
    if (cidr) return { matcherKey: "ip_cidr", text: cidr };
    return { matcherKey: "domain_suffix", text: host.toLowerCase() };
  }
  const cidr = toIpCidr(row.destination_ip);
  return cidr ? { matcherKey: "ip_cidr", text: cidr } : null;
}

/** `host:port`, falling back to the destination IP (IPv6 bracketed). */
export function connectionTarget(row: ConnectionRow): string {
  const host =
    row.host ||
    (row.destination_ip.includes(":")
      ? `[${row.destination_ip}]`
      : row.destination_ip);
  if (!host) return row.destination_port ? `:${row.destination_port}` : "";
  return row.destination_port ? `${host}:${row.destination_port}` : host;
}

/**
 * The outbound chain in the order traffic takes it: the group the route
 * picked first, the exit node last. The core lists it the other way round.
 */
export function connectionChain(row: ConnectionRow): string {
  return [...row.chains].reverse().join(" → ");
}

/**
 * The matcher of the rule that routed the connection. The core appends the
 * action (` => route(proxy)`), which the chain already shows.
 */
export function connectionRule(row: ConnectionRow): string {
  const arrow = row.rule.indexOf(" => ");
  return arrow === -1 ? row.rule : row.rule.slice(0, arrow);
}

/** Executable name of a process path (`/usr/bin/curl` → `curl`). */
export function processName(path: string): string {
  if (!path) return "";
  // The core may append ` (user)` to the path.
  const bare = path.replace(/ \([^)]*\)$/, "");
  const cut = Math.max(bare.lastIndexOf("/"), bare.lastIndexOf("\\"));
  return cut === -1 ? bare : bare.slice(cut + 1);
}

/** `42s`, `3m 05s`, `1h 02m`, `2d 03h`. */
export function formatDuration(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const pad = (n: number) => String(n).padStart(2, "0");
  if (total < 60) return `${total}s`;
  if (total < 3600) return `${Math.floor(total / 60)}m ${pad(total % 60)}s`;
  if (total < 86_400) {
    return `${Math.floor(total / 3600)}h ${pad(Math.floor((total % 3600) / 60))}m`;
  }
  return `${Math.floor(total / 86_400)}d ${pad(Math.floor((total % 86_400) / 3600))}h`;
}

/**
 * Row fields a keyword searches. The MCP `list_connections` keyword searches
 * the same ones (`SEARCH_FIELDS` in `src-tauri/src/application/connections.rs`);
 * a Rust test keeps the two lists equal.
 */
export const CONNECTION_SEARCH_FIELDS = [
  "host",
  "destination_ip",
  "destination_port",
  "network",
  "inbound",
  "source",
  "process_path",
  "rule",
  "chains",
] as const satisfies readonly (keyof ConnectionRow)[];

/** Case-insensitive match over everything a row shows. */
export function matchesKeyword(row: ConnectionRow, keyword: string): boolean {
  const needle = keyword.trim().toLowerCase();
  if (!needle) return true;
  return CONNECTION_SEARCH_FIELDS.flatMap((field) => row[field]).some((value) =>
    value.toLowerCase().includes(needle),
  );
}

/**
 * Order rows for display. Ties fall back to newest first and then to the id,
 * so rows with equal keys keep their place from one poll to the next.
 */
export function sortConnections(
  rows: readonly ConnectionRow[],
  sort: ConnectionSort,
): ConnectionRow[] {
  const newest = (a: ConnectionRow, b: ConnectionRow) =>
    (b.start_ms ?? 0) - (a.start_ms ?? 0) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
  const sorted = [...rows];
  if (sort === "total") {
    sorted.sort(
      (a, b) => b.upload + b.download - (a.upload + a.download) || newest(a, b),
    );
  } else if (sort === "download" || sort === "upload") {
    sorted.sort((a, b) => b[sort] - a[sort] || newest(a, b));
  } else {
    sorted.sort(newest);
  }
  return sorted;
}
