// SPDX-License-Identifier: GPL-3.0-or-later

/** Platform-neutral DTOs and application API. No transport imports. */
import type { ErrorCode } from "./errorCodes";


export type CoreStatus =
  | "stopped"
  | "starting"
  | "running"
  | "stopping"
  | "error";


/** Backend i18n payload: frontend runs `t(key, params)` (FE-5). */
export type UiMessage = {
  key: string;
  params?: Record<string, string>;
};


export type CoreState = {
  status: CoreStatus;
  message: UiMessage | null;
  inbound_host: string | null;
  inbound_port: number | null;
};


/** Active traffic-capture backend (`docs/tun.md`; derived only from the runtime controller). */
export type TrafficCapture = "inactive" | "system_proxy" | "tun";


/** TUN capture lifecycle (`docs/tun.md`). */
export type TunStatus =
  | "disabled"
  | "preparing"
  | "enabled"
  | "stopping"
  | "permission_required"
  | "error"
  | "recovery_required";


/** Per-process memory figure for the Home memory row: the physical footprint
 * on macOS (Activity Monitor's "Memory" column) and the private working set on
 * Windows. Only the core and the app's main process are measured — WebView
 * helpers and the privileged helper daemon are out of scope. `null` parts are
 * unreadable (e.g. a privileged macOS core); `total_bytes` sums what was
 * read. */
export type MemoryUsage = {
  app_bytes: number | null;
  core_bytes: number | null;
  total_bytes: number;
};


/** Home's current exit. Member lists stay on the Nodes page. */
export type SelectedOutbound = {
  tag: string;
  outbound_type: string;
  group_now: string | null;
};


export type StatusResponse = {
  /** Optional for compatibility with older IPC fixtures and clients. */
  revision?: number;
  sampled_at_ms?: number;
  refresh_pending?: boolean;
  workers?: Array<{ name: string; alive: boolean; restarts: number }>;
  diagnostics?: {
    checked_at_ms: number | null;
    age_ms: number | null;
    stale: boolean;
    error: UiMessage | null;
  };
  core: CoreState;
  subscription_count: number;
  /** Process memory for the Home memory row (core + app main process). */
  memory: MemoryUsage;
  proxy_recovery_warning: UiMessage[] | null;
  system_proxy_applied: boolean | null;
  /** On-disk applied flag; permits stopping capture after external OS changes. */
  system_proxy_recorded: boolean | null;
  /** False when the platform has no system-proxy backend (e.g. Linux). */
  system_proxy_available: boolean;
  // --- TUN capture status ---
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
  tun_unavailable_reason: UiMessage | null;
  /** True when the platform must not surface TUN controls at all (Windows:
   * TUN gate blocked upstream); the frontend hides the TUN card/switches. */
  tun_ui_hidden: boolean;
  /** Privileged helper installed + authorized (read-only probe); drives the
   * helper installation and removal actions. */
  helper_installed: boolean;
  /** Whether this platform has an installable privileged helper at all
   * (macOS only in this release). When false, the helper actions and the
   * install-before-enable guide are hidden. */
  helper_supported: boolean;
  /** Whether this platform can register an OS login item at all (macOS and
   * Windows). When false the Settings page hides the Startup card, so the
   * switch never offers an action that can only fail. */
  launch_at_login_supported: boolean;
  /** The helper's root-owned core differs from the app's bundled core (app
   * updated): only one core version may exist, so TUN stays blocked until
   * the helper is refreshed. */
  helper_stale: boolean;
  /** Windows scheduled-task elevation installed (plan B): when true, TUN
   * start/stop runs without any elevation prompt; when false the frontend
   * runs the one-time `ensureTunElevation` (single UAC) before persisting
   * the TUN-on next-start desire. Always true on non-Windows hosts. */
  tun_elevation_ready: boolean;
  /** Whether Settings can offer a menu-bar readout. macOS only; never inferred
   * from the user agent (an iPhone webview would look like macOS). */
  tray_display_supported: boolean;
  /** Mobile VPN consent. Omitted on desktop. */
  vpn_permission?: "unknown" | "granted" | "denied";
  /** Mobile tunnel lifecycle. Omitted on desktop. */
  tunnel_status?: "stopped" | "connecting" | "connected" | "disconnecting" | "error";
  /** Android only. True when the app is exempt from battery optimization. */
  battery_unrestricted?: boolean;
  /** Android only. True when Private DNS is pinned to a hostname. */
  private_dns_strict?: boolean;
  /** Android only. True when this app is the system always-on VPN. */
  always_on_vpn?: boolean;
  /** Active profile has at least one outbound. Omitted by older fixtures. */
  has_nodes?: boolean;
  /** Resolved exit for the Home row. */
  selected_outbound?: SelectedOutbound | null;
};


export type ProxyMode = "rule" | "global" | "direct";


/** macOS menu-bar item: icon with the live speed readout, icon only, or
 * readout only. Stored on every platform; the Settings page shows the control
 * on macOS only. */
export type TrayDisplayMode = "icon_and_speed" | "icon" | "speed";


/** Validated TUN capture parameters (`docs/tun.md`). Only `enabled` is user-facing. */
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
  /** Logs page shows every parsed line. Default is connections and important events. */
  log_debug: boolean;
  /** macOS menu-bar item: icon + live speed, icon only, or speed only. */
  tray_display_mode: TrayDisplayMode;
  /**
   * OS login item: start at login into the tray and restore the last capture
   * state without opening the window.
   */
  launch_at_login: boolean;
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
  parse_warnings: UiMessage[];
  last_updated: string | null;
  last_error: UiMessage | null;
  etag: string | null;
  last_modified: string | null;
  /** Provider traffic counters from the last successful fetch, when reported. */
  userinfo: SubscriptionUserInfo | null;
  /** Usage / expiry entries the provider embeds in the proxy list, verbatim. */
  provider_info: string[];
  auto_update: boolean;
  auto_update_interval: SubscriptionAutoUpdateInterval | null;
};


export type SubscriptionUserInfo = {
  upload: number;
  download: number;
  /** Total quota in bytes; `0` when the provider reports no quota. */
  total: number;
  /** Unix seconds until the subscription expires; absent when it has no expiry. */
  expire?: number | null;
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
  /** Home start/stop owns this; Settings omits it. */
  proxy_service_enabled?: boolean;
  allow_lan?: boolean;
  proxy_mode?: ProxyMode;
  tun?: Partial<TunSettings> & { interface_name?: string | null };
  auto_default_rules?: boolean;
  language?: "system" | "zh" | "en";
  check_app_updates?: boolean;
  log_debug?: boolean;
  tray_display_mode?: TrayDisplayMode;
  /** Registered by the dedicated login-item path, not the form autosave. */
  launch_at_login?: boolean;
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

/** Shared by the desktop shell and the mobile shell. */
export interface CoreApi {
  getStatus(): Promise<StatusResponse>;
  listSubscriptions(): Promise<SubscriptionMeta[]>;
  listNodes(): Promise<NodeInfo[]>;
  setSelectedNode(tag: string): Promise<void>;
  setGroupSelection(group: string, member: string): Promise<void>;
  testNodeDelay(tag: string): Promise<DelayTestResponse>;
  getTrafficSnapshot(): Promise<TrafficSnapshot>;
  getTrafficSince(cursor?: number | null): Promise<TrafficDelta>;
  listenCoreStatusChanged(handler: () => void): Promise<() => void>;
  listenStateChanged(handler: () => void): Promise<() => void>;
  listenTrafficSample(handler: (payload: TrafficPoint) => void): Promise<() => void>;
  start(): Promise<void>;
  stop(): Promise<void>;
  getLogView(n: number): Promise<string[]>;
  getRuntimeConfig(): Promise<string>;
  getSettings(): Promise<AppSettings>;
  saveSettings(patch: SettingsPatch): Promise<void>;
  setProxyMode(mode: ProxyMode): Promise<void>;
  addSubscription(url: string, name?: string, autoUpdate?: boolean, interval?: SubscriptionAutoUpdateInterval): Promise<SubscriptionMeta>;
  removeSubscription(id: string): Promise<{ ok: boolean; apply_warning?: AppErrorPayload }>;
  updateSubscription(id: string): Promise<SubscriptionMeta>;
  updateAllSubscriptions(): Promise<unknown>;
  setSubscriptionActive(id: string, active: boolean): Promise<SubscriptionMeta>;
  setSubscriptionAutoUpdate(id: string, autoUpdate: boolean, interval: SubscriptionAutoUpdateInterval): Promise<SubscriptionMeta>;
  /** Full subscription URL, or a portable sing-box document. The list keeps URLs redacted. */
  subscriptionShare(id: string, kind: "url" | "singbox"): Promise<string>;
  /** Write that sing-box document to a file. `"cancelled"` when the save dialog is dismissed. */
  exportSubscriptionSingbox(id: string, name: string, title: string): Promise<"saved" | "cancelled">;
  getRuleOverview(): Promise<RuleOverview>;
  listRules(req: ListRulesRequest): Promise<ListRulesResponse>;
  setRuleDisabled(fingerprint: string, disabled: boolean): Promise<{ ok: boolean; disabled: boolean; apply_warning?: AppErrorPayload }>;
  addCustomRule(rule: Record<string, unknown>): Promise<{ ok: boolean; fingerprint: string; apply_warning?: AppErrorPayload }>;
  removeCustomRule(fingerprint: string): Promise<{ ok: boolean; apply_warning?: AppErrorPayload }>;
}

/** Desktop capture: system proxy, TUN recovery, and the privileged helper. */
export interface DesktopCaptureApi {
  stopSystemProxy(): Promise<void>;
  recoverTun(): Promise<UiMessage[]>;
  installHelper(): Promise<void>;
  uninstallHelper(): Promise<void>;
  ensureTunElevation(): Promise<void>;
  removeTunElevation(): Promise<void>;
}

/** Desktop shell: window, tray, updater, data directory, and the proxy terminal. */
export interface DesktopShellApi {
  listenWindowHidden(handler: () => void): Promise<() => void>;
  listenWindowShown(handler: () => void): Promise<() => void>;
  listenTrayUpdateClick(handler: () => void): Promise<() => void>;
  revealDataDir(): Promise<void>;
  copyProxyCommand(): Promise<void>;
  openProxyTerminal(): Promise<void>;
  restoreLaunchProxy(): Promise<void>;
  checkAppUpdate(background?: boolean, startup?: boolean): Promise<CheckAppUpdateResponse>;
  recordUpdatePrompt(): Promise<void>;
  recordAppUpdateCheck(): Promise<void>;
  skipAppUpdate(version: string): Promise<void>;
  installAppUpdate(): Promise<void>;
  /** Android: open the release APK in the system browser. No-op on desktop. */
  openAppDownload(): Promise<void>;
  /** Android: system prompt to ignore battery optimization. No-op on desktop. */
  requestBatteryExemption(): Promise<void>;
  /** Android: open the system network screen. No-op on desktop. */
  openNetworkSettings(): Promise<void>;
  /** Android: open the system VPN screen. No-op on desktop. */
  openVpnSettings(): Promise<void>;
  listenAppUpdateProgress(handler: (payload: UpdateProgressPayload) => void): Promise<() => void>;
  setTrayLanguage(language: "zh" | "en"): Promise<void>;
  setTrayUpdateAvailable(version: string | null): Promise<void>;
  /** Home is visible. Desktop status polls refresh live group `now` only then. */
  setHomeActive(active: boolean): Promise<void>;
  /** Logs page visibility. Closing it drops the parsed tails; a later read does not rebuild them. */
  setLogViewActive(active: boolean): Promise<void>;
  /** Drop the lowercase rule index built for keyword search. */
  releaseRuleSearch(): Promise<void>;
}

/** Shared views depend on this full contract. The mobile transport no-ops the desktop-only methods. */
export interface ApiContract extends CoreApi, DesktopCaptureApi, DesktopShellApi {}
