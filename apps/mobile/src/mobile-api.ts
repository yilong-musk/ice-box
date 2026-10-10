// SPDX-License-Identifier: GPL-3.0-or-later

/** Phone transport. Core commands match the desktop names; desktop-only methods do nothing. */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { saveTextFile, singboxExportFilename } from "../../desktop/src/lib/saveFile";
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
  SubscriptionMeta,
  TrafficDelta,
  UpdateProgressPayload,
  TrafficPoint,
  TrafficSnapshot,
} from "../../desktop/src/api/contracts";

const noop = async () => {};
const unlisten = async () => () => {};

export const api = {
  getStatus: () => invoke<StatusResponse>("get_status"),
  setHomeActive: async () => {},
  setLogViewActive: async () => {},
  releaseRuleSearch: async () => {},
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
  addSubscription: (url: string, name?: string) =>
    invoke<SubscriptionMeta>("add_subscription", {
      req: {
        url,
        name: name ?? null,
        auto_update: false,
        auto_update_interval: null,
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
  setSubscriptionAutoUpdate: () =>
    Promise.reject({
      code: "sub.auto_update_unsupported",
      message: "subscription auto-update is not available on the phone",
    }),
  subscriptionShare: (id: string, kind: "url" | "singbox") =>
    invoke<string>("subscription_share", { req: { id, kind } }),
  exportSubscriptionSingbox: async (id: string, name: string) => {
    const text = await invoke<string>("subscription_share", {
      req: { id, kind: "singbox" },
    });
    saveTextFile(singboxExportFilename(name), text);
    return "saved" as const;
  },
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
  checkAppUpdate: (background = false, startup = false) =>
    invoke<CheckAppUpdateResponse>("check_app_update", {
      req: { background, startup },
    }),
  recordAppUpdateCheck: () => invoke<void>("record_app_update_check"),
  recordUpdatePrompt: noop,
  skipAppUpdate: async () => {},
  installAppUpdate: () => invoke<void>("install_app_update"),
  openAppDownload: () => invoke<void>("open_app_download"),
  requestBatteryExemption: () => invoke<void>("request_battery_exemption"),
  openNetworkSettings: () => invoke<void>("open_network_settings"),
  openVpnSettings: () => invoke<void>("open_vpn_settings"),
  listenAppUpdateProgress: (handler: (payload: UpdateProgressPayload) => void) =>
    listen<UpdateProgressPayload>("app-update://progress", (event) =>
      handler(event.payload),
    ),
  setTrayLanguage: noop,
  setTrayUpdateAvailable: noop,
} satisfies ApiContract;
