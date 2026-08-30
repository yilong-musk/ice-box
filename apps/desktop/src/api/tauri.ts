/** Typed wrappers around Tauri invoke (architecture §14). */

import { invoke } from "@tauri-apps/api/core";

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
  /** Privileged helper installed + authorized (read-only probe); drives the
   *「安装/卸载辅助组件」actions. */
  helper_installed: boolean;
  /** The helper's root-owned core differs from the app's bundled core (app
   * updated): only one core version may exist, so TUN stays blocked until
   * the helper is refreshed. */
  helper_stale: boolean;
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
  allow_lan: boolean;
  proxy_mode: ProxyMode;
  tun: TunSettings;
};

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

export type AppErrorPayload = {
  code: string;
  message: string;
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

/**
 * Error codes whose raw message is technical / English and confusing in the UI;
 * display the friendly text instead. Codes not listed keep `${code}: ${message}`.
 */
const FRIENDLY_ERROR_CODES: Record<string, string> = {
  "config.empty_outbounds":
    "没有可用的订阅节点，请先在「订阅」页导入订阅，或保持仅直连模式运行",
  // TUN error codes (plan §5 T2 / §4.3): the raw messages can be English /
  // technical; surface actionable Chinese text instead.
  "tun.not_supported": "当前平台暂不支持 TUN 模式",
  "tun.permission_required":
    "启用 TUN 需要系统权限：请先安装并授权受信任的辅助组件（点击下方按钮，将弹出系统授权密码框），当前未修改任何系统配置",
  "tun.helper_install_cancelled":
    "已取消系统授权，未修改任何系统配置。可在需要时重新点击「安装辅助组件」",
  "tun.helper_install_failed": "辅助组件安装失败，未修改任何系统配置",
  "tun.helper_not_ready":
    "辅助组件已安装，但守护进程尚未就绪，请稍后重试",
  "tun.helper_stale":
    "辅助组件仍在使用旧版内核：应用已更新内核版本，请先在设置中点击「更新辅助组件」（将弹出系统授权密码框）",
  "tun.apply_failed": "TUN 启动失败，已回滚到切换前的状态",
  "tun.restore_failed": "TUN 清理未确认，已停止内核以保持系统一致",
  "tun.healthcheck_failed": "TUN 就绪检查未通过，已释放捕获",
  "tun.recovery_required":
    "TUN 清理未确认，已阻止新的 TUN 激活；请先执行「重试恢复」",
};

export function formatInvokeError(err: unknown): string {
  if (err && typeof err === "object") {
    const o = err as Record<string, unknown>;
    if (typeof o.code === "string" && typeof o.message === "string") {
      return FRIENDLY_ERROR_CODES[o.code] ?? `${o.code}: ${o.message}`;
    }
    if (typeof o.message === "string") return o.message;
  }
  return String(err);
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
  getLogView: (n: number) =>
    invoke<string[]>("get_log_view", { req: { n } }),
  getRuntimeConfig: () => invoke<string>("get_runtime_config"),
  revealDataDir: () => invoke<void>("reveal_data_dir"),
  getSettings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) =>
    invoke<void>("save_settings", { settings }),
  setProxyMode: (mode: ProxyMode) =>
    invoke<void>("set_proxy_mode", { req: { mode } }),
  addSubscription: (url: string, name?: string) =>
    invoke<SubscriptionMeta>("add_subscription", {
      req: { url, name: name ?? null },
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
  applySubscriptions: () => invoke<void>("apply_subscriptions"),
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
