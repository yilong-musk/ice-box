// SPDX-License-Identifier: GPL-3.0-or-later

/** Phone transport. Core commands match the desktop names; desktop-only methods do nothing. */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  ApiContract,
  AppErrorPayload,
  AppSettings,
  CheckAppUpdateResponse,
  DelayTestResponse,
  ListRulesRequest,
  ListRulesResponse,
  NodeInfo,
  ProxyMode,
  RuleOverview,
  SettingsPatch,
  StatusResponse,
  SubscriptionAutoUpdateInterval,
  SubscriptionMeta,
  TrafficDelta,
  TrafficPoint,
  TrafficSnapshot,
} from "../../desktop/src/api/contracts";

const noop = async () => {};
const unlisten = async () => () => {};

const noUpdate: CheckAppUpdateResponse = {
  available: false,
  version: null,
  notes: null,
  skipped: false,
  should_prompt: false,
};

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
  listenStateChanged: (handler: () => void) =>
    listen("app://state-changed", () => handler()),
  listenTrafficSample: (handler: (payload: TrafficPoint) => void) =>
    listen<TrafficPoint>("traffic://sample", (event) => handler(event.payload)),
  start: () => invoke<void>("start"),
  stop: () => invoke<void>("stop"),
  getLogView: (n: number) => invoke<string[]>("get_log_view", { req: { n } }),
  getRuntimeConfig: () => invoke<string>("get_runtime_config"),
  getSettings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (patch: SettingsPatch) => invoke<void>("save_settings", { patch }),
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
    invoke<{ ok: boolean; apply_warning?: AppErrorPayload }>("remove_subscription", {
      req: { id },
    }),
  updateSubscription: (id: string) =>
    invoke<SubscriptionMeta>("update_subscription", { req: { id } }),
  updateAllSubscriptions: () => invoke<unknown>("update_all_subscriptions"),
  setSubscriptionActive: (id: string, active: boolean) =>
    invoke<SubscriptionMeta>("set_active_subscription", { req: { id, active } }),
  setSubscriptionAutoUpdate: (
    id: string,
    autoUpdate: boolean,
    interval: SubscriptionAutoUpdateInterval,
  ) =>
    invoke<SubscriptionMeta>("set_auto_update_subscription", {
      req: { id, auto_update: autoUpdate, auto_update_interval: interval },
    }),
  getRuleOverview: () => invoke<RuleOverview>("get_rule_overview"),
  listRules: (req: ListRulesRequest) => invoke<ListRulesResponse>("list_rules", { req }),
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
    invoke<{ ok: boolean; apply_warning?: AppErrorPayload }>("remove_custom_rule", {
      req: { fingerprint },
    }),
  stopSystemProxy: noop,
  recoverTun: async () => [],
  installHelper: noop,
  uninstallHelper: noop,
  ensureTunElevation: noop,
  removeTunElevation: noop,
  listenWindowHidden: unlisten,
  listenWindowShown: unlisten,
  listenTrayUpdateClick: unlisten,
  revealDataDir: noop,
  copyProxyCommand: noop,
  openProxyTerminal: noop,
  restoreLaunchProxy: noop,
  checkAppUpdate: async () => noUpdate,
  recordUpdatePrompt: noop,
  recordAppUpdateCheck: noop,
  skipAppUpdate: async () => {},
  installAppUpdate: noop,
  listenAppUpdateProgress: unlisten,
  setTrayLanguage: noop,
  setTrayUpdateAvailable: noop,
} satisfies ApiContract;
