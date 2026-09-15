// SPDX-License-Identifier: GPL-3.0-or-later

export function formatRate(bytesPerSec: number): string {
  if (bytesPerSec < 1024) return `${bytesPerSec} B/s`;
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
  return `${(bytesPerSec / (1024 * 1024)).toFixed(2)} MB/s`;
}

/** Byte counts (subscription quotas) as `372.5MB` / `11.84GB` / `1TB`. */
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
