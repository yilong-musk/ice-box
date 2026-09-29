// SPDX-License-Identifier: GPL-3.0-or-later

import { isMessageKey, t, type MessageKey } from "../lib/i18n";
import { isErrorCode, type ErrorCode } from "./errorCodes";
import type { UiMessage } from "./contracts";

const ERROR_MESSAGE_KEYS = {
  "core.not_found": "error.core.not_found",
  "core.spawn_failed": "error.core.spawn_failed",
  "core.healthcheck_failed": "error.core.healthcheck_failed",
  "core.invalid_state": "error.core.invalid_state",
  "core.adopt_rejected": "error.core.adopt_rejected",
  "core.api_failed": "error.core.api_failed",
  "config.empty_outbounds": "error.config.empty_outbounds",
  "config.invalid": "error.config.invalid",
  "proxy.apply_failed": "error.proxy.apply_failed",
  "proxy.apply_failed_core_reloaded": "error.proxy.apply_failed_core_reloaded",
  "proxy.restore_failed": "error.proxy.restore_failed",
  "proxy.backup_corrupt": "error.proxy.backup_corrupt",
  "settings.reset": "error.settings.reset",
  "logs.oversized": "error.logs.oversized",
  "sub.fetch_failed": "error.sub.fetch_failed",
  "sub.unknown_format": "error.sub.unknown_format",
  "sub.parse_failed": "error.sub.parse_failed",
  "sub.empty": "error.sub.empty",
  "sub.not_found": "error.sub.not_found",
  "sub.io": "error.sub.io",
  "app.lock_poisoned": "error.app.lock_poisoned",
  "app.autostart_failed": "error.app.autostart_failed",
  "tun.not_supported": "error.tun.not_supported",
  "tun.permission_required": "error.tun.permission_required",
  "tun.apply_failed": "error.tun.apply_failed",
  "tun.restore_failed": "error.tun.restore_failed",
  "tun.healthcheck_failed": "error.tun.healthcheck_failed",
  "tun.recovery_required": "error.tun.recovery_required",
  "tun.invalid_argument": "error.tun.invalid_argument",
  "tun.config_rejected": "error.tun.config_rejected",
  "tun.helper_stale": "error.tun.helper_stale",
  "tun.helper_install_failed": "error.tun.helper_install_failed",
  "tun.helper_install_cancelled": "error.tun.helper_install_cancelled",
  "tun.helper_not_ready": "error.tun.helper_not_ready",
  "tun.elevation_cancelled": "error.tun.elevation_cancelled",
  "tun.elevation_requires_admin": "error.tun.elevation_requires_admin",
  "update.check_failed": "error.update.check_failed",
  "update.feed_unavailable": "error.update.feed_unavailable",
  "update.install_failed": "error.update.install_failed",
  "update.disabled": "error.update.disabled",
} as const satisfies Record<ErrorCode, MessageKey>;

export function formatUiMessage(
  msg: UiMessage | string | null | undefined,
): string {
  if (!msg) return "";
  if (typeof msg === "string") return msg;
  const params = msg.params ?? {};
  if (msg.key === "ui.raw") return params.text ?? "";
  const label = isMessageKey(msg.key) ? t(msg.key, params) : msg.key;
  const detail = params.detail;
  if (detail && !label.includes(detail)) {
    return `${label}: ${detail}`;
  }
  return label;
}

/**
 * Known IPC codes are translated via `t()`. Unknown payloads keep
 * `code: message` so a new backend code still surfaces.
 */
export function formatInvokeError(err: unknown): string {
  if (err && typeof err === "object") {
    const o = err as Record<string, unknown>;
    if (typeof o.code === "string" && typeof o.message === "string") {
      if (isErrorCode(o.code)) {
        const label = `${t(ERROR_MESSAGE_KEYS[o.code])} (${o.code})`;
        const detail = o.message.trim();
        if (detail && !label.includes(detail)) {
          return `${label}: ${detail}`;
        }
        return label;
      }
      return `${o.code}: ${o.message}`;
    }
    if (typeof o.message === "string") return o.message;
  }
  return String(err);
}

/** Translate a recovery-banner fragment that starts with a known error code. */
export function formatDiagnostic(warning: string): string {
  return warning
    .split("；")
    .map((part) => {
      const trimmed = part.trim();
      const colon = trimmed.indexOf(":");
      const code = (colon >= 0 ? trimmed.slice(0, colon) : trimmed).trim();
      if (isErrorCode(code)) {
        const label = t(ERROR_MESSAGE_KEYS[code]);
        const rest = colon >= 0 ? trimmed.slice(colon + 1).trim() : "";
        return rest ? `${label}: ${rest}` : label;
      }
      return trimmed;
    })
    .filter(Boolean)
    .join("；");
}
