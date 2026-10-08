// SPDX-License-Identifier: GPL-3.0-or-later

/** Desktop transport implementation of the shared application contract. */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ApiContract, AppErrorPayload, AppSettings, CheckAppUpdateResponse, DelayTestResponse, ListRulesRequest, ListRulesResponse, NodeInfo, ProxyMode, RuleOverview, SettingsPatch, StatusResponse, SubscriptionAutoUpdateInterval, SubscriptionMeta, TrafficDelta, TrafficPoint, TrafficSnapshot, UiMessage, UpdateProgressPayload } from "./contracts";

export const api = {
  getStatus: () => invoke<StatusResponse>("get_status"),
  setHomeActive: (active: boolean) => invoke<void>("set_home_active", { active }),
  releaseLogView: () => invoke<void>("release_log_view"),
  releaseRuleSearch: () => invoke<void>("release_rule_keyword_cache"),
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
  /** Emitted after a state mutation the window did not initiate (tray menu
   * actions, launch-time restore) so pages re-read status and settings. */
  listenStateChanged: (handler: () => void) =>
    listen("app://state-changed", () => handler()),
  listenWindowHidden: (handler: () => void) =>
    listen("window://hidden", () => handler()),
  listenWindowShown: (handler: () => void) =>
    listen("window://shown", () => handler()),
  /** Tray update prompt clicked: open Settings → App Updates, the same
   * destination as the sidebar arrow. */
  listenTrayUpdateClick: (handler: () => void) =>
    listen("app-update://open", () => handler()),
  listenTrafficSample: (
    handler: (payload: TrafficPoint) => void,
  ) =>
    listen<TrafficPoint>("traffic://sample", (event) =>
      handler(event.payload),
    ),
  start: () => invoke<void>("start"),
  stopSystemProxy: () => invoke<void>("stop_system_proxy"),
  stop: () => invoke<void>("stop"),
  /** On-demand TUN recovery retry (`docs/tun.md`); never enables capture. */
  recoverTun: () => invoke<UiMessage[]>("recover_tun"),
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
  getRuntimeConfig: () => invoke<string>("get_runtime_config"),
  revealDataDir: () => invoke<void>("reveal_data_dir"),
  /** Copy the session-only Mixed command (Rust owns the text and clipboard). */
  copyProxyCommand: () => invoke<void>("copy_proxy_command"),
  /** Open the default terminal with session-only Mixed proxy env vars. */
  openProxyTerminal: () => invoke<void>("open_proxy_terminal"),
  getSettings: () => invoke<AppSettings>("get_settings"),
  /** First-frame restore of last-session capture. No-op when it was off. */
  restoreLaunchProxy: () => invoke<void>("restore_launch_proxy"),
  saveSettings: (patch: SettingsPatch) =>
    invoke<void>("save_settings", { patch }),
  /** `startup` marks the launch round, which the backend runs even inside the
   * 24h in-session cooldown so every app start checks GitHub once. */
  checkAppUpdate: (background = false, startup = false) =>
    invoke<CheckAppUpdateResponse>("check_app_update", {
      req: { background, startup },
    }),
  recordUpdatePrompt: () => invoke<void>("record_update_prompt"),
  /** Persist `last_check_at` after the background retry ladder is exhausted. */
  recordAppUpdateCheck: () => invoke<void>("record_app_update_check"),
  skipAppUpdate: (version: string) =>
    invoke<void>("skip_app_update", { req: { version } }),
  installAppUpdate: () => invoke<void>("install_app_update"),
  openAppDownload: async () => {},
  requestBatteryExemption: async () => {},
  openNetworkSettings: async () => {},
  openVpnSettings: async () => {},
  listenAppUpdateProgress: (
    handler: (payload: UpdateProgressPayload) => void,
  ) =>
    listen<UpdateProgressPayload>("app-update://progress", (event) =>
      handler(event.payload),
    ),
  setTrayLanguage: (language: "zh" | "en") =>
    invoke<void>("set_tray_language", { language }),
  /** Tray update prompt: mirror the version the sidebar arrow offers, or
   * `null` to drop the item when automatic checks are off. */
  setTrayUpdateAvailable: (version: string | null) =>
    invoke<void>("set_tray_update_available", { version }),
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
} satisfies ApiContract;
