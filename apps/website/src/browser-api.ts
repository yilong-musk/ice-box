import type {
  AppSettings,
  DelayTestResponse,
  ListRulesRequest,
  ListRulesResponse,
  NodeInfo,
  ProxyMode,
  RuleOverview,
  StatusResponse,
  SubscriptionAutoUpdateInterval,
  SubscriptionMeta,
  TrafficSnapshot,
} from "../../../apps/desktop/src/api/tauri";

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
  TrafficPoint,
  TrafficSample,
  TrafficSnapshot,
} from "../../../apps/desktop/src/api/tauri";

/** README / CI screenshot mode (`demo.html?capture=1`) freezes traffic and skips mock latency. */
function isCaptureMode(): boolean {
  return typeof window !== "undefined" && new URLSearchParams(window.location.search).get("capture") === "1";
}

const CAPTURE_NOW = Date.UTC(2026, 0, 15, 12, 0, 0);

const settings: AppSettings = {
  mixed_listen: "127.0.0.1",
  mixed_port: 17890,
  clash_api_listen: "127.0.0.1",
  clash_api_port: 19090,
  selected_tag: "Tokyo / edge-01",
  auto_set_system_proxy: true,
  allow_lan: false,
  proxy_mode: "rule",
  auto_default_rules: true,
  language: "en",
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

const nodes: NodeInfo[] = [
  { tag: "Tokyo / edge-01", outbound_type: "vmess", group_now: null, group_all: null },
  { tag: "Singapore / edge-02", outbound_type: "trojan", group_now: null, group_all: null },
  { tag: "Los Angeles / edge-03", outbound_type: "shadowsocks", group_now: null, group_all: null },
  { tag: "Frankfurt / edge-04", outbound_type: "hysteria2", group_now: null, group_all: null },
];

const subscriptions: SubscriptionMeta[] = [
  {
    id: "demo-profile",
    name: "Personal profile",
    url: "https://demo.ice-box.dev/profile.yaml",
    active: true,
    format: "Clash",
    node_count: 18,
    group_count: 3,
    rule_count: 42,
    has_dns: true,
    parse_warnings: [],
    last_updated: new Date((isCaptureMode() ? CAPTURE_NOW : Date.now()) - 4 * 60_000).toISOString(),
    last_error: null,
    etag: null,
    last_modified: null,
    auto_update: true,
    auto_update_interval: "one_hour",
  },
];

const ruleRows = [
  ["domain", "github.com", "Tokyo / edge-01"],
  ["domain_suffix", "googleapis.com", "Singapore / edge-02"],
  ["geoip", "cn", "direct"],
  ["final", "match all remaining traffic", "Tokyo / edge-01"],
].map(([rule_type, value, outbound], index) => ({
  index,
  fingerprint: `demo-rule-${index}`,
  rule: { [rule_type]: value, outbound },
  custom: false,
  disabled: false,
  rule_type,
}));

let running = true;
let trafficTick = 0;

const delay = (ms = 80) =>
  isCaptureMode() ? Promise.resolve() : new Promise<void>((resolve) => window.setTimeout(resolve, ms));

export function formatInvokeError(err: unknown): string {
  if (err && typeof err === "object" && "message" in err) return String(err.message);
  return String(err);
}

export const api = {
  async getStatus(): Promise<StatusResponse> {
    await delay();
    return {
      core: { status: running ? "running" : "stopped", message: null, inbound_host: "127.0.0.1", inbound_port: 17890 },
      subscription_count: subscriptions.length,
      proxy_recovery_warning: null,
      system_proxy_applied: running,
      system_proxy_recorded: running,
      system_proxy_available: true,
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
      helper_stale: false,
      tun_elevation_ready: true,
    };
  },
  async getSettings(): Promise<AppSettings> { await delay(); return structuredClone(settings); },
  async saveSettings(next: AppSettings): Promise<void> { await delay(); Object.assign(settings, structuredClone(next)); },
  async setTrayLanguage(): Promise<void> {},
  async listNodes(): Promise<NodeInfo[]> { await delay(); return [...nodes]; },
  async setSelectedNode(tag: string): Promise<void> { settings.selected_tag = tag; await delay(); },
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
  async start(): Promise<void> { await delay(220); running = true; },
  async stop(): Promise<void> { await delay(180); running = false; },
  async stopSystemProxy(): Promise<void> { await delay(180); running = false; },
  async recoverTun(): Promise<null> { await delay(); return null; },
  async installHelper(): Promise<void> { await delay(); },
  async uninstallHelper(): Promise<void> { await delay(); },
  async ensureTunElevation(): Promise<void> { await delay(); },
  async removeTunElevation(): Promise<void> { await delay(); },
  async getLogView(): Promise<string[]> { await delay(); return ["12:04:31 route match api.github.com → Tokyo / edge-01", "12:04:28 health check Tokyo / edge-01 responded in 42 ms", "12:04:16 subscription Profile refreshed · 18 nodes", "12:03:52 dns Using fallback resolver 1.1.1.1"]; },
  async getRuntimeConfig(): Promise<string> { return "{\n  \"route\": { \"final\": \"Tokyo / edge-01\" }\n}"; },
  async revealDataDir(): Promise<void> {},
  async setProxyMode(mode: ProxyMode): Promise<void> { settings.proxy_mode = mode; await delay(120); },
  async listSubscriptions(): Promise<SubscriptionMeta[]> { await delay(); return [...subscriptions]; },
  async addSubscription(url: string, name?: string, autoUpdate = false, interval: SubscriptionAutoUpdateInterval = "one_hour"): Promise<SubscriptionMeta> { await delay(180); const sub = { ...subscriptions[0], id: `demo-${subscriptions.length + 1}`, url, name: name || "Imported profile", auto_update: autoUpdate, auto_update_interval: autoUpdate ? interval : null }; subscriptions.push(sub); return sub; },
  async removeSubscription(id: string): Promise<{ ok: boolean }> { await delay(); const index = subscriptions.findIndex((sub) => sub.id === id); if (index >= 0) subscriptions.splice(index, 1); return { ok: true }; },
  async updateSubscription(): Promise<SubscriptionMeta> { await delay(220); return subscriptions[0]; },
  async updateAllSubscriptions(): Promise<void> { await delay(220); },
  async setSubscriptionActive(id: string, active: boolean): Promise<SubscriptionMeta> { await delay(); const sub = subscriptions.find((item) => item.id === id) ?? subscriptions[0]; sub.active = active; return sub; },
  async setSubscriptionAutoUpdate(id: string, autoUpdate: boolean, interval: SubscriptionAutoUpdateInterval): Promise<SubscriptionMeta> { await delay(); const sub = subscriptions.find((item) => item.id === id) ?? subscriptions[0]; sub.auto_update = autoUpdate; sub.auto_update_interval = autoUpdate ? interval : null; return sub; },
  async getRuleOverview(): Promise<RuleOverview> { await delay(); return { total: ruleRows.length, disabled: ruleRows.filter((row) => row.disabled).length, custom: 0, rule_sets: 2, types: [{ rule_type: "domain", count: 2 }, { rule_type: "geoip", count: 1 }] }; },
  async listRules(req: ListRulesRequest): Promise<ListRulesResponse> { await delay(); const keyword = req.keyword?.toLowerCase() ?? ""; const filtered = ruleRows.filter((row) => JSON.stringify(row.rule).toLowerCase().includes(keyword)); return { total: filtered.length, offset: req.offset, limit: req.limit, items: filtered.slice(req.offset, req.offset + req.limit) }; },
  async setRuleDisabled(fingerprint: string, disabled: boolean): Promise<{ ok: boolean; disabled: boolean }> { const row = ruleRows.find((item) => item.fingerprint === fingerprint); if (row) row.disabled = disabled; return { ok: true, disabled }; },
  async addCustomRule(): Promise<{ ok: boolean; fingerprint: string }> { return { ok: true, fingerprint: "demo-custom-rule" }; },
  async removeCustomRule(): Promise<{ ok: boolean }> { return { ok: true }; },
};
