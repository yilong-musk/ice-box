// SPDX-License-Identifier: GPL-3.0-or-later

/** Typed wrappers around Tauri invoke (architecture §14). */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { t, type MessageKey } from "../lib/i18n";
import { isErrorCode, type ErrorCode } from "./errorCodes";

export type CoreStatus =
  | "stopped"
  | "starting"
  | "running"
  | "stopping"
  | "error";

export type CoreState = {
  status: CoreStatus;
  message: string | null;
  inbound_host: string | null;
  inbound_port: number | null;
};

/** Active traffic-capture backend (plan §4.3; derived only from the runtime controller). */
export type TrafficCapture = "inactive" | "system_proxy" | "tun";

/** TUN capture lifecycle (plan §4.3). */
export type TunStatus =
  | "disabled"
  | "preparing"
  | "enabled"
  | "stopping"
  | "permission_required"
  | "error"
  | "recovery_required";

export type StatusResponse = {
  core: CoreState;
  subscription_count: number;
  proxy_recovery_warning: string | null;
  system_proxy_applied: boolean | null;
  /** On-disk applied flag; drives「停止代理服务」when OS proxy was changed externally. */
  system_proxy_recorded: boolean | null;
  /** False when the platform has no system-proxy backend (e.g. Linux). */
  system_proxy_available: boolean;
  // --- TUN capture status (plan §4.3) ---
  /** `inactive` means no backend is claimed; `tun_status=recovery_required` blocks fallback. */
  traffic_capture: TrafficCapture;
  /** Committed settings desire (`settings.tun.enabled`); not proof TUN is active. */
  configured_tun: boolean;
  tun_status: TunStatus;
  tun_interface: string | null;
  tun_error: AppErrorPayload | null;
  capture_transition_id: string | null;
  /** False when the platform gate is pending/failed; the switch stays disabled. */
  tun_available: boolean;
  tun_unavailable_reason: string | null;
  /** True when the platform must not surface TUN controls at all (Windows:
   * TUN gate blocked upstream); the frontend hides the TUN card/switches. */
  tun_ui_hidden: boolean;
  /** Privileged helper installed + authorized (read-only probe); drives the
   *「安装/卸载辅助组件」actions. */
  helper_installed: boolean;
  /** Whether this platform has an installable privileged helper at all
   * (macOS only in this release). When false, the helper actions and the
   * install-before-enable guide are hidden. */
  helper_supported: boolean;
  /** The helper's root-owned core differs from the app's bundled core (app
   * updated): only one core version may exist, so TUN stays blocked until
   * the helper is refreshed. */
  helper_stale: boolean;
  /** Windows scheduled-task elevation installed (plan B): when true, TUN
   * start/stop runs without any elevation prompt; when false the frontend
   * runs the one-time `ensureTunElevation` (single UAC) before persisting
   * the TUN-on next-start desire. Always true on non-Windows hosts. */
  tun_elevation_ready: boolean;
};

export type ProxyMode = "rule" | "global" | "direct";

/** Validated TUN capture parameters (plan §4.1). Only `enabled` is user-facing. */
export type TunSettings = {
  enabled: boolean;
  interface_name: string | null;
  ipv4_address: string;
  ipv6_address: string;
  mtu: number;
  auto_route: boolean;
  strict_route: boolean;
  stack: string;
  dns_hijack: boolean;
};

export type AppSettings = {
  mixed_listen: string;
  mixed_port: number;
  clash_api_listen: string;
  clash_api_port: number;
  selected_tag: string | null;
  auto_set_system_proxy: boolean;
  /** Last Home power-button state; launch restores capture when true. */
  proxy_service_enabled: boolean;
  allow_lan: boolean;
  proxy_mode: ProxyMode;
  tun: TunSettings;
  auto_default_rules: boolean;
  /** "system" follows the OS locale; otherwise an explicit UI language. */
  language: "system" | "zh" | "en";
  /** Background update checks and the auto prompt. Defaults to on. */
  check_app_updates: boolean;
  /** sing-box `log.level`: `warn` (default) or `info` for debug sessions. */
  core_log_level: "warn" | "info";
};

export type SubscriptionAutoUpdateInterval =
  | "one_hour"
  | "three_hours"
  | "six_hours"
  | "twelve_hours"
  | "twenty_four_hours";

export type SubscriptionMeta = {
  id: string;
  name: string;
  url: string;
  active: boolean;
  format: string;
  node_count: number;
  group_count: number;
  rule_count: number;
  has_dns: boolean;
  parse_warnings: string[];
  last_updated: string | null;
  last_error: string | null;
  etag: string | null;
  last_modified: string | null;
  auto_update: boolean;
  auto_update_interval: SubscriptionAutoUpdateInterval | null;
};

export type NodeInfo = {
  tag: string;
  outbound_type: string;
  /** Live member currently used by a strategy group (Clash API `now`); null when core not running. */
  group_now: string | null;
  /** Live member tags of a strategy group; null for leaf nodes or when core not running. */
  group_all: string[] | null;
};

export type DelayTestResponse = {
  tag: string;
  delay_ms: number;
};

export type TrafficSample = {
  up: number;
  down: number;
};

export type TrafficPoint = TrafficSample & { t: number };

export type TrafficSnapshot = {
  points: TrafficPoint[];
  latest: TrafficSample | null;
  /** Highest observed rate during the current proxy run. */
  peak: TrafficSample | null;
};

export type TrafficDelta = {
  generation: number;
  cursor: number | null;
  points: TrafficPoint[];
  latest: TrafficSample | null;
  peak: TrafficSample | null;
};

export type SettingsPatch = {
  mixed_listen?: string;
  mixed_port?: number;
  clash_api_listen?: string;
  clash_api_port?: number;
  selected_tag?: string | null;
  auto_set_system_proxy?: boolean;
  allow_lan?: boolean;
  proxy_mode?: ProxyMode;
  tun?: Partial<TunSettings> & { interface_name?: string | null };
  auto_default_rules?: boolean;
  language?: "system" | "zh" | "en";
  check_app_updates?: boolean;
  core_log_level?: "warn" | "info";
};

export type AppErrorPayload = {
  code: ErrorCode | string;
  message: string;
};

export type CheckAppUpdateResponse = {
  available: boolean;
  version: string | null;
  notes: string | null;
  skipped: boolean;
  should_prompt: boolean;
};

export type UpdateProgressPayload = {
  phase: string;
  downloaded: number;
  content_length: number | null;
};

export type RuleTypeCount = {
  rule_type: string;
  count: number;
};

export type RuleOverview = {
  total: number;
  /** Disabled fingerprints matching a current rule (subscription or custom). */
  disabled: number;
  custom: number;
  rule_sets: number;
  /** Subscription rule counts by classified type, most frequent first. */
  types: RuleTypeCount[];
};

export type RuleRow = {
  /** Position in the active subscription's route.rules; null for custom rules. */
  index: number | null;
  fingerprint: string;
  rule: Record<string, unknown>;
  custom: boolean;
  disabled: boolean;
  rule_type: string;
};

export type ListRulesRequest = {
  keyword?: string | null;
  type?: string | null;
  /** "all" | "disabled" | "enabled" */
  disabled?: "all" | "disabled" | "enabled" | null;
  /** true = custom rules only, false = subscription rules only. */
  custom?: boolean | null;
  offset: number;
  limit: number;
};

export type ListRulesResponse = {
  total: number;
  offset: number;
  limit: number;
  items: RuleRow[];
};

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
  "update.check_failed": "error.update.check_failed",
  "update.feed_unavailable": "error.update.feed_unavailable",
  "update.install_failed": "error.update.install_failed",
  "update.disabled": "error.update.disabled",
} as const satisfies Record<ErrorCode, MessageKey>;

/**
 * Known IPC codes are translated via `t()`. Unknown payloads keep
 * `code: message` so a new backend code still surfaces.
 */
export function formatInvokeError(err: unknown): string {
  if (err && typeof err === "object") {
    const o = err as Record<string, unknown>;
    if (typeof o.code === "string" && typeof o.message === "string") {
      if (isErrorCode(o.code)) {
        return `${t(ERROR_MESSAGE_KEYS[o.code])} (${o.code})`;
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

export const api = {
  getStatus: () => invoke<StatusResponse>("get_status"),
  listSubscriptions: () => invoke<SubscriptionMeta[]>("list_subscriptions"),
  listNodes: () => invoke<NodeInfo[]>("list_nodes"),
  setSelectedNode: (tag: string) =>
    invoke<void>("set_selected_node", { req: { tag } }),
  setGroupSelection: (group: string, member: string) =>
    invoke<void>("set_group_selection", { req: { group, member } }),
  testNodeDelay: (tag: string) =>
    invoke<DelayTestResponse>("test_node_delay", { req: { tag } }),
  getTrafficSnapshot: () => invoke<TrafficSnapshot>("get_traffic_snapshot"),
  getTrafficSince: (cursor?: number | null) =>
    invoke<TrafficDelta>("get_traffic_since", { cursor: cursor ?? null }),
  listenCoreStatusChanged: (handler: () => void) =>
    listen("core://status-changed", () => handler()),
  listenWindowHidden: (handler: () => void) =>
    listen("window://hidden", () => handler()),
  listenWindowShown: (handler: () => void) =>
    listen("window://shown", () => handler()),
  listenTrafficSample: (
    handler: (payload: TrafficPoint) => void,
  ) =>
    listen<TrafficPoint>("traffic://sample", (event) =>
      handler(event.payload),
    ),
  start: () => invoke<void>("start"),
  stopSystemProxy: () => invoke<void>("stop_system_proxy"),
  stop: () => invoke<void>("stop"),
  /** On-demand TUN recovery retry (plan §4.3); never enables capture. */
  recoverTun: () => invoke<string | null>("recover_tun"),
  /** Install + authorize the privileged helper via the system authorization
   * dialog (unsigned elevation path). macOS only; cancel modifies nothing. */
  installHelper: () => invoke<void>("install_helper"),
  /** Uninstall the privileged helper via the system authorization dialog. */
  uninstallHelper: () => invoke<void>("uninstall_helper"),
  /** One-time Windows TUN elevation setup (plan B): installs the scheduled
   * task that runs the TUN core elevated — the single UAC prompt the user
   * ever sees. No-op when already installed / not Windows. */
  ensureTunElevation: () => invoke<void>("ensure_tun_elevation"),
  /** Remove the Windows TUN scheduled task (one UAC prompt). */
  removeTunElevation: () => invoke<void>("remove_tun_elevation"),
  getLogView: (n: number) =>
    invoke<string[]>("get_log_view", { req: { n } }),
  clearLogs: () => invoke<void>("clear_logs"),
  getRuntimeConfig: () => invoke<string>("get_runtime_config"),
  revealDataDir: () => invoke<void>("reveal_data_dir"),
  getSettings: () => invoke<AppSettings>("get_settings"),
  /** First-frame restore of last-session capture. No-op when it was off. */
  restoreLaunchProxy: () => invoke<void>("restore_launch_proxy"),
  saveSettings: (patch: SettingsPatch) =>
    invoke<void>("save_settings", { patch }),
  checkAppUpdate: (background = false) =>
    invoke<CheckAppUpdateResponse>("check_app_update", {
      req: { background },
    }),
  recordUpdatePrompt: () => invoke<void>("record_update_prompt"),
  skipAppUpdate: (version: string) =>
    invoke<void>("skip_app_update", { req: { version } }),
  installAppUpdate: () => invoke<void>("install_app_update"),
  listenAppUpdateProgress: (
    handler: (payload: UpdateProgressPayload) => void,
  ) =>
    listen<UpdateProgressPayload>("app-update://progress", (event) =>
      handler(event.payload),
    ),
  setTrayLanguage: (language: "zh" | "en") =>
    invoke<void>("set_tray_language", { language }),
  setProxyMode: (mode: ProxyMode) =>
    invoke<void>("set_proxy_mode", { req: { mode } }),
  addSubscription: (
    url: string,
    name?: string,
    autoUpdate = false,
    interval: SubscriptionAutoUpdateInterval = "one_hour",
  ) =>
    invoke<SubscriptionMeta>("add_subscription", {
      req: {
        url,
        name: name ?? null,
        auto_update: autoUpdate,
        auto_update_interval: autoUpdate ? interval : null,
      },
    }),
  removeSubscription: (id: string) =>
    invoke<{ ok: boolean; apply_warning?: AppErrorPayload }>(
      "remove_subscription",
      { req: { id } },
    ),
  updateSubscription: (id: string) =>
    invoke<SubscriptionMeta>("update_subscription", { req: { id } }),
  updateAllSubscriptions: () =>
    invoke<unknown>("update_all_subscriptions"),
  setSubscriptionActive: (id: string, active: boolean) =>
    invoke<SubscriptionMeta>("set_active_subscription", {
      req: { id, active },
    }),
  setSubscriptionAutoUpdate: (
    id: string,
    autoUpdate: boolean,
    interval: SubscriptionAutoUpdateInterval,
  ) =>
    invoke<SubscriptionMeta>("set_auto_update_subscription", {
      req: { id, auto_update: autoUpdate, auto_update_interval: interval },
    }),
  getRuleOverview: () => invoke<RuleOverview>("get_rule_overview"),
  listRules: (req: ListRulesRequest) =>
    invoke<ListRulesResponse>("list_rules", { req }),
  setRuleDisabled: (fingerprint: string, disabled: boolean) =>
    invoke<{ ok: boolean; disabled: boolean; apply_warning?: AppErrorPayload }>(
      "set_rule_disabled",
      { req: { fingerprint, disabled } },
    ),
  addCustomRule: (rule: Record<string, unknown>) =>
    invoke<{ ok: boolean; fingerprint: string; apply_warning?: AppErrorPayload }>(
      "add_custom_rule",
      { req: { rule } },
    ),
  removeCustomRule: (fingerprint: string) =>
    invoke<{ ok: boolean; apply_warning?: AppErrorPayload }>(
      "remove_custom_rule",
      { req: { fingerprint } },
    ),
};
