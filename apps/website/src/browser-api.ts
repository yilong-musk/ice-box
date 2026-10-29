// SPDX-License-Identifier: GPL-3.0-or-later

import type {
  ApiContract,
  AppSettings,
  AppProxyMode,
  CheckAppUpdateResponse,
  DelayTestResponse,
  InstalledApp,
  ListRulesRequest,
  ListRulesResponse,
  McpStatus,
  NodeInfo,
  ProxyMode,
  RuleOverview,
  StatusResponse,
  SubscriptionAutoUpdateInterval,
  SubscriptionMeta,
  TrafficDelta,
  TrafficSnapshot,
  SettingsPatch,
  UiMessage,
} from "../../../apps/desktop/src/api/contracts";
import { formatQuota } from "../../../apps/desktop/src/lib/traffic";
import { copyText } from "../../../apps/desktop/src/lib/logCopy";
import { saveTextFile, singboxExportFilename } from "../../../apps/desktop/src/lib/saveFile";

export type {
  AppErrorPayload,
  AppSettings,
  CoreState,
  DelayTestResponse,
  ListRulesRequest,
  ListRulesResponse,
  NodeInfo,
  ProxyMode,
  RuleOverview,
  RuleRow,
  StatusResponse,
  SubscriptionAutoUpdateInterval,
  SubscriptionMeta,
  TrafficDelta,
  TrafficPoint,
  TrafficSample,
  TrafficSnapshot,
  SettingsPatch,
  UiMessage,
} from "../../../apps/desktop/src/api/contracts";

/** README / CI screenshot mode (`demo.html?capture=1`) freezes traffic and skips mock latency. */
function isCaptureMode(): boolean {
  return typeof window !== "undefined" && new URLSearchParams(window.location.search).get("capture") === "1";
}

const CAPTURE_NOW = Date.UTC(2026, 0, 15, 12, 0, 0);
const DEMO_LANDING_IP = "203.0.113.42";
const DEMO_INITIAL_RUN_MS = 18 * 60_000 + 42_000;
const TOKYO_NODE = "🇯🇵 Tokyo / edge-01";
const SINGAPORE_NODE = "🇸🇬 Singapore / edge-02";

const settings: AppSettings = {
  mixed_listen: "127.0.0.1",
  mixed_port: 17890,
  clash_api_listen: "127.0.0.1",
  clash_api_port: 19090,
  selected_tag: TOKYO_NODE,
  auto_set_system_proxy: true,
  proxy_service_enabled: false,
  tray_display_mode: "icon_and_speed",
  launch_at_login: false,
  allow_lan: false,
  ipv6: true,
  prefer_ipv6: false,
  proxy_mode: "rule",
  app_proxies: {},
  auto_default_rules: true,
  language: "en",
  check_app_updates: true,
  probe_exit_ip: true,
  log_debug: false,
  tun: {
    enabled: false,
    interface_name: null,
    ipv4_address: "10.0.0.1/30",
    ipv6_address: "fdfe:dcba:9876::1/126",
    mtu: 9000,
    auto_route: true,
    strict_route: true,
    stack: "gvisor",
    dns_hijack: true,
  },
};

const personalNodes: NodeInfo[] = [
  { tag: TOKYO_NODE, outbound_type: "vmess", group_now: null, group_all: null },
  { tag: SINGAPORE_NODE, outbound_type: "trojan", group_now: null, group_all: null },
  { tag: "🇺🇸 Los Angeles / edge-03", outbound_type: "shadowsocks", group_now: null, group_all: null },
  { tag: "🇩🇪 Frankfurt / edge-04", outbound_type: "hysteria2", group_now: null, group_all: null },
  { tag: "🇰🇷 Seoul / edge-05", outbound_type: "vless", group_now: null, group_all: null },
  { tag: "🇭🇰 Hong Kong / edge-06", outbound_type: "trojan", group_now: null, group_all: null },
  { tag: "🇦🇺 Sydney / edge-07", outbound_type: "shadowsocks", group_now: null, group_all: null },
  { tag: "🇬🇧 London / edge-08", outbound_type: "vless", group_now: null, group_all: null },
  { tag: "🇺🇸 New York / edge-09", outbound_type: "vmess", group_now: null, group_all: null },
  { tag: "🇨🇦 Toronto / edge-10", outbound_type: "trojan", group_now: null, group_all: null },
  { tag: "🇳🇱 Amsterdam / edge-11", outbound_type: "hysteria2", group_now: null, group_all: null },
  { tag: "🇫🇷 Paris / edge-12", outbound_type: "shadowsocks", group_now: null, group_all: null },
  { tag: "🇮🇳 Mumbai / edge-13", outbound_type: "vless", group_now: null, group_all: null },
  { tag: "🇦🇪 Dubai / edge-14", outbound_type: "trojan", group_now: null, group_all: null },
  { tag: "🇧🇷 São Paulo / edge-15", outbound_type: "vmess", group_now: null, group_all: null },
  { tag: "🇨🇭 Zurich / edge-16", outbound_type: "hysteria2", group_now: null, group_all: null },
  { tag: "🇯🇵 Osaka / edge-17", outbound_type: "vless", group_now: null, group_all: null },
  { tag: "🇫🇮 Helsinki / edge-18", outbound_type: "trojan", group_now: null, group_all: null },
];

/**
 * Each demo subscription exposes a different node list, so switching profiles
 * changes the Nodes and Home pages. Every list keeps the two tags referenced
 * by the rules and log fixtures.
 */
const workNodes = personalNodes.slice(0, 12);

const streamingNodes: NodeInfo[] = [
  ...personalNodes,
  { tag: "🇯🇵 Fukuoka / edge-19", outbound_type: "trojan", group_now: null, group_all: null },
  { tag: "🇸🇬 Jurong / edge-20", outbound_type: "vless", group_now: null, group_all: null },
  { tag: "🇺🇸 Seattle / edge-21", outbound_type: "hysteria2", group_now: null, group_all: null },
  { tag: "🇺🇸 Miami / edge-22", outbound_type: "vmess", group_now: null, group_all: null },
  { tag: "🇨🇦 Vancouver / edge-23", outbound_type: "shadowsocks", group_now: null, group_all: null },
  { tag: "🇬🇧 Manchester / edge-24", outbound_type: "trojan", group_now: null, group_all: null },
  { tag: "🇩🇪 Berlin / edge-25", outbound_type: "vless", group_now: null, group_all: null },
  { tag: "🇫🇷 Marseille / edge-26", outbound_type: "hysteria2", group_now: null, group_all: null },
  { tag: "🇳🇱 Rotterdam / edge-27", outbound_type: "vmess", group_now: null, group_all: null },
  { tag: "🇨🇭 Geneva / edge-28", outbound_type: "shadowsocks", group_now: null, group_all: null },
  { tag: "🇸🇪 Stockholm / edge-29", outbound_type: "trojan", group_now: null, group_all: null },
  { tag: "🇳🇴 Oslo / edge-30", outbound_type: "vless", group_now: null, group_all: null },
  { tag: "🇮🇹 Milan / edge-31", outbound_type: "hysteria2", group_now: null, group_all: null },
  { tag: "🇪🇸 Madrid / edge-32", outbound_type: "vmess", group_now: null, group_all: null },
  { tag: "🇵🇱 Warsaw / edge-33", outbound_type: "shadowsocks", group_now: null, group_all: null },
  { tag: "🇹🇷 Istanbul / edge-34", outbound_type: "trojan", group_now: null, group_all: null },
  { tag: "🇿🇦 Johannesburg / edge-35", outbound_type: "vless", group_now: null, group_all: null },
  { tag: "🇦🇷 Buenos Aires / edge-36", outbound_type: "hysteria2", group_now: null, group_all: null },
];

/** Capture mode freezes timestamps, so README screenshots stay reproducible. */
function demoNowMs(): number {
  return isCaptureMode() ? CAPTURE_NOW : Date.now();
}

/**
 * Capture mode freezes expiration dates too, but the UI compares them with the
 * real clock. A date derived from `CAPTURE_NOW` would therefore age into
 * "Expired"; pin capture-mode expiry far enough ahead instead.
 */
const CAPTURE_EXPIRY_MS = Date.UTC(2030, 0, 1);

function demoExpiryMs(days: number): number {
  const base = isCaptureMode() ? CAPTURE_EXPIRY_MS : Date.now();
  return base + days * 24 * 3600_000;
}

/** Demo quota: 125 GiB of a 500 GiB plan. */
const DEMO_QUOTA = {
  upload: 4 * 1024 ** 3,
  download: 121 * 1024 ** 3,
  total: 500 * 1024 ** 3,
  expire: Math.floor(demoExpiryMs(45) / 1000),
};

const subscriptions: SubscriptionMeta[] = [
  {
    id: "demo-profile",
    name: "Personal profile",
    url: "https://demo.ice-box.dev/profile.yaml",
    active: true,
    format: "Clash",
    node_count: personalNodes.length,
    group_count: 3,
    rule_count: 42,
    has_dns: true,
    parse_warnings: [],
    last_updated: new Date(demoNowMs() - 4 * 60_000).toISOString(),
    last_error: null,
    etag: null,
    last_modified: null,
    // Both shapes a provider can publish usage in: the `subscription-userinfo`
    // counters and the info entries embedded in the proxy list.
    userinfo: { ...DEMO_QUOTA },
    provider_info: [
      `Traffic: ${formatQuota(DEMO_QUOTA.upload + DEMO_QUOTA.download)} | ${formatQuota(DEMO_QUOTA.total)}`,
      `Expire: ${new Date(DEMO_QUOTA.expire * 1000).toISOString().slice(0, 10)}`,
    ],
    auto_update: true,
    auto_update_interval: "one_hour",
  },
  {
    id: "demo-work",
    name: "Work profile",
    url: "https://demo.ice-box.dev/work-singbox.json",
    active: false,
    format: "sing-box",
    node_count: workNodes.length,
    group_count: 2,
    rule_count: 18,
    has_dns: false,
    parse_warnings: [],
    last_updated: new Date(demoNowMs() - 2 * 60 * 60_000).toISOString(),
    last_error: null,
    etag: "\"demo-work-3\"",
    last_modified: null,
    userinfo: {
      upload: 2 * 1024 ** 3,
      download: 16 * 1024 ** 3,
      total: 200 * 1024 ** 3,
      expire: Math.floor(demoExpiryMs(90) / 1000),
    },
    provider_info: [],
    auto_update: true,
    auto_update_interval: "six_hours",
    source: "remote",
  },
  {
    id: "demo-streaming",
    name: "Streaming profile",
    url: "https://demo.ice-box.dev/streaming.yaml",
    active: false,
    format: "Clash",
    node_count: streamingNodes.length,
    group_count: 4,
    rule_count: 0,
    has_dns: true,
    parse_warnings: [],
    last_updated: new Date(demoNowMs() - 26 * 60 * 60_000).toISOString(),
    last_error: null,
    etag: null,
    last_modified: new Date(demoNowMs() - 26 * 60 * 60_000).toUTCString(),
    // This provider reports usage in the list body instead of the header.
    userinfo: null,
    provider_info: [
      "Traffic: 28.4 GB | 100 GB",
      `Expire: ${new Date(demoExpiryMs(120)).toISOString().slice(0, 10)}`,
    ],
    auto_update: false,
    auto_update_interval: null,
    source: "remote",
  },
];

/** Nodes loaded by each demo subscription; the active one drives the UI. */
const nodesBySubscription: Record<string, NodeInfo[]> = {
  "demo-profile": personalNodes,
  "demo-work": workNodes,
  "demo-streaming": streamingNodes,
};

/**
 * Ordered favorite tags per demo subscription (newest first). Kept separate
 * from the shared node fixtures so switching subscriptions shows its own set
 * and `listNodes` never mutates the fixtures.
 */
const favoritesBySubscription: Record<string, string[]> = {};

function activeFavorites(): string[] {
  const id = activeSubscription()?.id;
  return (id && favoritesBySubscription[id]) || [];
}

function activeSubscription(): SubscriptionMeta | undefined {
  return subscriptions.find((sub) => sub.active);
}

function activeNodes(): NodeInfo[] {
  return nodesBySubscription[activeSubscription()?.id ?? ""] ?? personalNodes;
}

const ruleRows = [
  ["domain", "github.com", TOKYO_NODE],
  ["domain_suffix", "googleapis.com", SINGAPORE_NODE],
  ["geoip", "cn", "direct"],
  ["final", "match all remaining traffic", TOKYO_NODE],
].map(([rule_type, value, outbound], index) => ({
  index,
  fingerprint: `demo-rule-${index}`,
  rule: { [rule_type]: value, outbound },
  custom: false,
  disabled: false,
  rule_type,
}));

let running = true;
let runningSinceMs = Date.now() - DEMO_INITIAL_RUN_MS;
let trafficTick = 0;
let mcpEnabled = true;
let mcpRunning = true;
/** Keeps imported ids unique after a demo subscription is deleted. */
let subscriptionSequence = subscriptions.length;

function demoMcpStatus(): McpStatus {
  const bridgeExecutable = "/Applications/ice-box.app/Contents/MacOS/ice-box";
  const bridgeArgs = [
    "--mcp-stdio",
    "--data-dir",
    "/Users/demo/Library/Application Support/com.yilong-musk.icebox",
  ];
  return {
    supported: true,
    enabled: mcpEnabled,
    running: mcpRunning,
    endpoint: mcpRunning ? "127.0.0.1:43001" : null,
    client_count: mcpRunning ? 1 : 0,
    tool_count: 34,
    bridge_executable: bridgeExecutable,
    bridge_args: bridgeArgs,
    client_config: JSON.stringify(
      { mcpServers: { "ice-box": { command: bridgeExecutable, args: bridgeArgs } } },
      null,
      2,
    ),
    error: null,
  };
}

const delay = (ms = 80) =>
  isCaptureMode() ? Promise.resolve() : new Promise<void>((resolve) => window.setTimeout(resolve, ms));

function demoSingboxConfig(): string {
  return "{\n  \"outbounds\": [\n    { \"type\": \"selector\", \"tag\": \"proxy\", \"outbounds\": [\"direct\"] }\n  ],\n  \"route\": { \"final\": \"proxy\" }\n}";
}

export const api = {
  async getStatus(): Promise<StatusResponse> {
    await delay();
    return {
      core: { status: running ? "running" : "stopped", message: null, inbound_host: "127.0.0.1", inbound_port: 17890 },
      running_since_ms: running ? runningSinceMs : null,
      subscription_count: subscriptions.length,
      proxy_recovery_warning: [],
      system_proxy_applied: running,
      system_proxy_recorded: running,
      system_proxy_available: true,
      // Green-band demo figure: the Live Demo and its screenshots must show a
      // real number, never `—`.
      memory: running
        ? {
            app_bytes: 38 * 1024 * 1024,
            core_bytes: 14 * 1024 * 1024,
            total_bytes: 52 * 1024 * 1024,
          }
        : {
            app_bytes: 38 * 1024 * 1024,
            core_bytes: null,
            total_bytes: 38 * 1024 * 1024,
          },
      traffic_capture: running ? "system_proxy" : "inactive",
      configured_tun: settings.tun.enabled,
      tun_status: settings.tun.enabled ? "enabled" : "disabled",
      tun_interface: null,
      tun_error: null,
      capture_transition_id: null,
      tun_available: true,
      tun_unavailable_reason: null,
      tun_ui_hidden: false,
      helper_installed: true,
      helper_supported: true,
      launch_at_login_supported: true,
      helper_stale: false,
      tun_elevation_ready: true,
      tray_display_supported: false,
      has_nodes: activeNodes().length > 0,
      selected_outbound: (() => {
        const nodes = activeNodes();
        const node = nodes.find((item) => item.tag === settings.selected_tag) ?? nodes[0];
        if (!node) return null;
        return {
          tag: node.tag,
          outbound_type: node.outbound_type,
          group_now: node.group_now,
          landing_ip: running ? DEMO_LANDING_IP : null,
        };
      })(),
    };
  },
  async setHomeActive(): Promise<void> {},
  async setLogViewActive(): Promise<void> {},
  async releaseRuleSearch(): Promise<void> {},
  async getSettings(): Promise<AppSettings> { await delay(); return structuredClone(settings); },
  async restoreLaunchProxy(): Promise<void> {
    await delay();
    if (settings.proxy_service_enabled) {
      running = true;
      runningSinceMs = Date.now();
    }
  },
  async start(): Promise<void> {
    await delay(180);
    running = true;
    runningSinceMs = Date.now();
  },
  async saveSettings(patch: SettingsPatch): Promise<void> {
    await delay();
    Object.assign(settings, structuredClone(patch));
    if (patch.tun) {
      settings.tun = { ...settings.tun, ...structuredClone(patch.tun) };
    }
    if (settings.prefer_ipv6) settings.ipv6 = true;
    if (!settings.ipv6) settings.prefer_ipv6 = false;
  },
  async checkAppUpdate(): Promise<CheckAppUpdateResponse> {
    await delay();
    return { available: false, version: null, notes: null, skipped: false, should_prompt: false };
  },
  async recordUpdatePrompt(): Promise<void> {},
  async recordAppUpdateCheck(): Promise<void> {},
  async skipAppUpdate(): Promise<void> {},
  async installAppUpdate(): Promise<void> { await delay(); },
  async openAppDownload(): Promise<void> {},
  async requestBatteryExemption(): Promise<void> {},
  async openNetworkSettings(): Promise<void> {},
  async openVpnSettings(): Promise<void> {},
  async listenAppUpdateProgress(): Promise<() => void> {
    return () => {};
  },
  async setTrayLanguage(): Promise<void> {},
  async setTrayUpdateAvailable(): Promise<void> {},
  async listNodes(): Promise<NodeInfo[]> {
    await delay();
    const favorites = activeFavorites();
    return activeNodes().map((node) => {
      const rank = favorites.indexOf(node.tag);
      return {
        ...node,
        favorite: rank >= 0,
        favorite_rank: rank >= 0 ? rank : null,
      };
    });
  },
  async setSelectedNode(tag: string): Promise<void> { settings.selected_tag = tag; await delay(); },
  async setNodeFavorite(tag: string, favorite: boolean): Promise<void> {
    await delay();
    const id = activeSubscription()?.id;
    if (!id) return;
    const next = (favoritesBySubscription[id] ?? []).filter((item) => item !== tag);
    if (favorite) next.unshift(tag);
    if (next.length > 0) favoritesBySubscription[id] = next;
    else delete favoritesBySubscription[id];
  },
  async setGroupSelection(): Promise<void> { await delay(); },
  async testNodeDelay(tag: string): Promise<DelayTestResponse> { await delay(300); return { tag, delay_ms: 42 + Math.floor(Math.random() * 90) }; },
  async getTrafficSnapshot(): Promise<TrafficSnapshot> {
    await delay();
    if (isCaptureMode()) {
      const up = 240_200;
      const down = 1_380_000;
      return {
        points: Array.from({ length: 24 }, (_, index) => ({
          t: CAPTURE_NOW - (23 - index) * 2500,
          up: up * (0.55 + (index % 5) / 10),
          down: down * (0.58 + (index % 6) / 10),
        })),
        latest: { up, down },
        peak: { up: up * 1.2, down: down * 1.15 },
      };
    }
    trafficTick += 1;
    const up = running ? 180_000 + (trafficTick % 5) * 22_000 : 0;
    const down = running ? 1_300_000 + (trafficTick % 7) * 75_000 : 0;
    return { points: Array.from({ length: 24 }, (_, index) => ({ t: Date.now() - (23 - index) * 2500, up: up * (0.55 + (index % 5) / 10), down: down * (0.58 + (index % 6) / 10) })), latest: { up, down }, peak: { up: up * 1.2, down: down * 1.15 } };
  },
  async getTrafficSince(cursor?: number | null): Promise<TrafficDelta> {
    const snap = await this.getTrafficSnapshot();
    const points = cursor == null ? snap.points : snap.points.filter((p) => p.t > cursor);
    return {
      generation: 1,
      cursor: snap.points[snap.points.length - 1]?.t ?? null,
      points,
      latest: snap.latest,
      peak: snap.peak,
    };
  },
  async listenCoreStatusChanged(): Promise<() => void> {
    return () => {};
  },
  async listenStateChanged(_handler: () => void): Promise<() => void> {
    return () => {};
  },
  async listenWindowHidden(): Promise<() => void> {
    return () => {};
  },
  async listenWindowShown(): Promise<() => void> {
    return () => {};
  },
  async listenTrayUpdateClick(): Promise<() => void> {
    return () => {};
  },
  async listenTrafficSample(): Promise<() => void> {
    return () => {};
  },
  async stop(): Promise<void> { await delay(180); running = false; },
  async stopSystemProxy(): Promise<void> { await delay(180); running = false; },
  async recoverTun(): Promise<UiMessage[]> { await delay(); return []; },
  async installHelper(): Promise<void> { await delay(); },
  async uninstallHelper(): Promise<void> { await delay(); },
  async ensureTunElevation(): Promise<void> { await delay(); },
  async removeTunElevation(): Promise<void> { await delay(); },
  async getLogView(_n: number): Promise<string[]> {
    await delay();
    const user = [
      `INFO 09-08 22:04:31 api.github.com:443 → ${TOKYO_NODE}`,
      "INFO 09-08 22:04:16 ice_core: subscription Profile refreshed",
    ];
    if (!settings.log_debug) return user;
    return [
      ...user,
      "DEBUG 09-08 22:04:30 ice_core: probe loop tick",
      "INFO 09-08 22:04:29 [TCP] dial api.github.com:443",
    ];
  },
  async getRuntimeConfig(): Promise<string> { return JSON.stringify({ route: { final: TOKYO_NODE } }, null, 2); },
  async revealDataDir(): Promise<void> {},
  async copyProxyCommand(): Promise<void> { await delay(); },
  async openProxyTerminal(): Promise<void> { await delay(); },
  async setProxyMode(mode: ProxyMode): Promise<void> { settings.proxy_mode = mode; await delay(120); },
  async listApps(): Promise<InstalledApp[]> { return []; },
  async setAppProxy(_packageName: string, _mode: AppProxyMode): Promise<void> {},
  async listSubscriptions(): Promise<SubscriptionMeta[]> { await delay(); return [...subscriptions]; },
  async addSubscription(url: string, name?: string, autoUpdate = false, interval: SubscriptionAutoUpdateInterval = "one_hour"): Promise<SubscriptionMeta> {
    await delay(180);
    const sub: SubscriptionMeta = {
      id: `demo-${++subscriptionSequence}`,
      name: name || "Imported profile",
      url,
      active: false,
      format: "Clash",
      node_count: 0,
      group_count: 0,
      rule_count: 0,
      has_dns: false,
      parse_warnings: [],
      last_updated: new Date(demoNowMs()).toISOString(),
      last_error: null,
      etag: null,
      last_modified: null,
      userinfo: null,
      provider_info: [],
      auto_update: autoUpdate,
      auto_update_interval: autoUpdate ? interval : null,
      source: "remote",
    };
    subscriptions.push(sub);
    return sub;
  },
  async importSubscriptionFile(content: string, name?: string): Promise<SubscriptionMeta> {
    await delay(180);
    const trimmed = content.trim();
    if (!trimmed.startsWith("{") || (!trimmed.includes("\"outbounds\"") && !trimmed.includes("\"endpoints\""))) {
      throw { code: "sub.parse_failed", message: "invalid sing-box subscription: not a sing-box configuration" };
    }
    const sub: SubscriptionMeta = {
      id: `demo-${++subscriptionSequence}`,
      name: name || "Imported file",
      url: "",
      active: false,
      format: "sing_box",
      node_count: 0,
      group_count: 0,
      rule_count: 0,
      has_dns: false,
      parse_warnings: [],
      last_updated: new Date(demoNowMs()).toISOString(),
      last_error: null,
      etag: null,
      last_modified: null,
      userinfo: null,
      provider_info: [],
      auto_update: false,
      auto_update_interval: null,
      source: "file",
    };
    subscriptions.push(sub);
    return sub;
  },
  async removeSubscription(id: string): Promise<{ ok: boolean }> {
    await delay();
    const index = subscriptions.findIndex((sub) => sub.id === id);
    if (index >= 0) subscriptions.splice(index, 1);
    delete favoritesBySubscription[id];
    return { ok: true };
  },
  async updateSubscription(id: string): Promise<SubscriptionMeta> {
    await delay(220);
    const sub = subscriptions.find((item) => item.id === id) ?? subscriptions[0];
    sub.last_updated = new Date(demoNowMs()).toISOString();
    sub.last_error = null;
    return sub;
  },
  async setSubscriptionActive(id: string, active: boolean): Promise<SubscriptionMeta> {
    await delay();
    const sub = subscriptions.find((item) => item.id === id) ?? subscriptions[0];
    // A profile switch activates exactly one subscription.
    if (active) {
      for (const item of subscriptions) item.active = item.id === sub.id;
      // Keep the selection inside the newly active profile's node list.
      const nextNodes = nodesBySubscription[sub.id];
      if (nextNodes && !nextNodes.some((node) => node.tag === settings.selected_tag)) {
        settings.selected_tag = nextNodes[0]?.tag ?? null;
      }
    } else {
      sub.active = false;
    }
    return sub;
  },
  async setSubscriptionAutoUpdate(id: string, autoUpdate: boolean, interval: SubscriptionAutoUpdateInterval): Promise<SubscriptionMeta> { await delay(); const sub = subscriptions.find((item) => item.id === id) ?? subscriptions[0]; sub.auto_update = autoUpdate; sub.auto_update_interval = autoUpdate ? interval : null; return sub; },
  async subscriptionShare(id: string, kind: "url" | "singbox"): Promise<string> {
    await delay();
    const sub = subscriptions.find((item) => item.id === id) ?? subscriptions[0];
    if (kind === "url") return sub.url;
    return demoSingboxConfig();
  },
  async copySubscriptionShare(id: string, kind: "url" | "singbox"): Promise<void> {
    await delay();
    const sub = subscriptions.find((item) => item.id === id) ?? subscriptions[0];
    await copyText(kind === "url" ? sub.url : demoSingboxConfig());
  },
  async exportSubscriptionSingbox(id: string, name: string): Promise<"saved" | "cancelled"> {
    await delay();
    const sub = subscriptions.find((item) => item.id === id) ?? subscriptions[0];
    saveTextFile(singboxExportFilename(name || sub.name), demoSingboxConfig());
    return "saved";
  },
  async getRuleOverview(): Promise<RuleOverview> { await delay(); return { total: ruleRows.length, disabled: ruleRows.filter((row) => row.disabled).length, custom: 0, rule_sets: 2, types: [{ rule_type: "domain", count: 2 }, { rule_type: "geoip", count: 1 }] }; },
  async listRules(req: ListRulesRequest): Promise<ListRulesResponse> { await delay(); const keyword = req.keyword?.toLowerCase() ?? ""; const filtered = ruleRows.filter((row) => JSON.stringify(row.rule).toLowerCase().includes(keyword)); return { total: filtered.length, offset: req.offset, limit: req.limit, items: filtered.slice(req.offset, req.offset + req.limit) }; },
  async setRuleDisabled(fingerprint: string, disabled: boolean): Promise<{ ok: boolean; disabled: boolean }> { const row = ruleRows.find((item) => item.fingerprint === fingerprint); if (row) row.disabled = disabled; return { ok: true, disabled }; },
  async addCustomRule(): Promise<{ ok: boolean; fingerprint: string }> { return { ok: true, fingerprint: "demo-custom-rule" }; },
  async updateCustomRule(fingerprint: string): Promise<{ ok: boolean; fingerprint: string }> { return { ok: true, fingerprint }; },
  async removeCustomRule(): Promise<{ ok: boolean }> { return { ok: true }; },
  async getMcpStatus(): Promise<McpStatus> { await delay(); return demoMcpStatus(); },
  async setMcpEnabled(enabled: boolean): Promise<McpStatus> {
    await delay();
    mcpEnabled = enabled;
    mcpRunning = enabled;
    return demoMcpStatus();
  },
  async restartMcp(): Promise<McpStatus> {
    await delay(180);
    mcpRunning = mcpEnabled;
    return demoMcpStatus();
  },
} satisfies ApiContract;
