// SPDX-License-Identifier: GPL-3.0-or-later

import type { ConnectionRow } from "../api/contracts";

export type ConnectionSort = "newest" | "traffic";

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

/** Byte totals as `512 B`, `1.5 KB`, `12.34 MB`, `1.20 GB`. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(2)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

/** Case-insensitive match over everything a row shows. */
export function matchesKeyword(row: ConnectionRow, keyword: string): boolean {
  const needle = keyword.trim().toLowerCase();
  if (!needle) return true;
  return [
    row.host,
    row.destination_ip,
    row.destination_port,
    row.network,
    row.inbound,
    row.source,
    row.process_path,
    row.rule,
    ...row.chains,
  ].some((field) => field.toLowerCase().includes(needle));
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
  if (sort === "traffic") {
    sorted.sort(
      (a, b) => b.upload + b.download - (a.upload + a.download) || newest(a, b),
    );
  } else {
    sorted.sort(newest);
  }
  return sorted;
}
