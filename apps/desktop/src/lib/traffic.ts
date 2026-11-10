// SPDX-License-Identifier: GPL-3.0-or-later

/** Byte totals as `512 B`, `1.5 KB`, `12.34 MB`, `1.20 GB`. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(2)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

/** Rates in the same units as {@link formatBytes}: `512 B/s`, `2.0 KB/s`. */
export function formatRate(bytesPerSec: number): string {
  return `${formatBytes(bytesPerSec)}/s`;
}

/**
 * Subscription quotas as `372.5MB` / `11.84GB` / `1TB`: compact, with
 * trailing zeros trimmed and units up to PB, to fit the subscription card.
 */
export function formatQuota(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0B";
  const units = ["KB", "MB", "GB", "TB", "PB"];
  if (bytes < 1024) return `${Math.round(bytes)}B`;
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const formatted = value.toFixed(2);
  const trimmed = formatted.includes(".")
    ? formatted.replace(/\.?0+$/, "")
    : formatted;
  return `${trimmed}${units[unit]}`;
}
