// SPDX-License-Identifier: GPL-3.0-or-later

/** Stable IPC error codes generated from `ice_config::ErrorCode` (ARCH-3). */

export const ERROR_CODES = [
  "core.not_found",
  "core.spawn_failed",
  "core.healthcheck_failed",
  "core.invalid_state",
  "core.adopt_rejected",
  "core.api_failed",
  "config.empty_outbounds",
  "config.invalid",
  "proxy.apply_failed",
  "proxy.apply_failed_core_reloaded",
  "proxy.restore_failed",
  "proxy.backup_corrupt",
  "settings.reset",
  "logs.oversized",
  "sub.fetch_failed",
  "sub.unknown_format",
  "sub.parse_failed",
  "sub.empty",
  "sub.not_found",
  "sub.io",
  "app.lock_poisoned",
  "tun.not_supported",
  "tun.permission_required",
  "tun.apply_failed",
  "tun.restore_failed",
  "tun.healthcheck_failed",
  "tun.recovery_required",
  "tun.invalid_argument",
  "tun.config_rejected",
  "tun.helper_stale",
  "tun.helper_install_failed",
  "tun.helper_install_cancelled",
  "tun.helper_not_ready",
  "tun.elevation_cancelled",
  "update.check_failed",
  "update.feed_unavailable",
  "update.install_failed",
  "update.disabled",
] as const;

export type ErrorCode = (typeof ERROR_CODES)[number];

export function isErrorCode(value: string): value is ErrorCode {
  return (ERROR_CODES as readonly string[]).includes(value);
}
