import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  formatInvokeError,
  type CoreState,
  type NodeInfo,
  type ProxyMode,
  type StatusResponse,
} from "../api/tauri";
import { EmptyState } from "../components/EmptyState";
import { useGenerationGuard } from "../lib/generationGuard";
import { resolveSelectedTag } from "../lib/nodes";
import { TrafficChart } from "../components/TrafficChart";

type Props = {
  onBusyChange?: (busy: boolean) => void;
  onNavigate?: (tab: "subs") => void;
};

const GROUP_TYPES = ["selector", "urltest", "fallback", "loadbalance"];

export function Home({ onBusyChange, onNavigate }: Props) {
  const { nextGeneration, isStale } = useGenerationGuard();
  const pollGenRef = useRef(0);
  const [status, setStatus] = useState<StatusResponse | null>(null);
  const [nodes, setNodes] = useState<NodeInfo[]>([]);
  const [selectedTag, setSelectedTag] = useState<string>("");
  const [proxyMode, setProxyMode] = useState<ProxyMode>("rule");
  const [delayMs, setDelayMs] = useState<number | null>(null);
  const [connCount, setConnCount] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [nodeBusy, setNodeBusy] = useState(false);
  const [modeBusy, setModeBusy] = useState(false);

  const refresh = useCallback(async (pollGen?: number) => {
    const gen = pollGen ?? pollGenRef.current;
    try {
      const [s, n, settings] = await Promise.all([
        api.getStatus(),
        api.listNodes(),
        api.getSettings(),
      ]);
      if (gen !== pollGenRef.current) return;

      setStatus(s);
      setNodes(n);
      setProxyMode(settings.proxy_mode);
      const nextTag = resolveSelectedTag(settings.selected_tag, n);
      setSelectedTag((prev) => {
        if (prev !== nextTag) setDelayMs(null);
        return nextTag;
      });
      setError(null);

      if (s.core.status === "running") {
        try {
          const stats = await api.getConnectionStats();
          if (gen !== pollGenRef.current) return;
          setConnCount(stats.connection_count);
        } catch {
          if (gen === pollGenRef.current) setConnCount(null);
        }
      } else {
        setConnCount(null);
        setDelayMs(null);
      }
    } catch (e) {
      if (gen === pollGenRef.current) setError(formatInvokeError(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const id = window.setInterval(() => {
      pollGenRef.current += 1;
      void refresh(pollGenRef.current);
    }, 2000);
    return () => window.clearInterval(id);
  }, [refresh]);

  const core: CoreState | undefined = status?.core;
  const busy =
    pending ||
    core?.status === "starting" ||
    core?.status === "stopping";

  useEffect(() => {
    onBusyChange?.(busy);
  }, [busy, onBusyChange]);

  async function run(action: () => Promise<void>) {
    pollGenRef.current += 1;
    setPending(true);
    setError(null);
    try {
      await action();
      await refresh();
    } catch (e) {
      setError(formatInvokeError(e));
      await refresh();
    } finally {
      setPending(false);
    }
  }

  async function onSelectNode(tag: string) {
    nextGeneration();
    setSelectedTag(tag);
    setDelayMs(null);
    setNodeBusy(true);
    setError(null);
    try {
      await api.setSelectedNode(tag);
      pollGenRef.current += 1;
      await refresh();
    } catch (e) {
      const message = formatInvokeError(e);
      pollGenRef.current += 1;
      await refresh();
      setError(message);
    } finally {
      setNodeBusy(false);
    }
  }

  async function onSetMode(mode: ProxyMode) {
    if (mode === proxyMode) return;
    const gen = nextGeneration();
    setModeBusy(true);
    setError(null);
    setProxyMode(mode);
    try {
      await api.setProxyMode(mode);
      pollGenRef.current += 1;
      await refresh();
    } catch (e) {
      if (isStale(gen)) return;
      const message = formatInvokeError(e);
      pollGenRef.current += 1;
      await refresh();
      setError(message);
    } finally {
      if (!isStale(gen)) setModeBusy(false);
    }
  }

  async function onTestDelay() {
    if (!selectedTag) return;
    const tag = selectedTag;
    const gen = nextGeneration();
    setNodeBusy(true);
    setError(null);
    try {
      const r = await api.testNodeDelay(tag);
      if (isStale(gen)) return;
      setDelayMs(r.delay_ms);
    } catch (e) {
      if (isStale(gen)) return;
      setError(formatInvokeError(e));
      setDelayMs(null);
    } finally {
      setNodeBusy(false);
    }
  }

  // Core follows the app; these buttons only toggle OS system proxy.
  const running = core?.status === "running";
  const proxyAvailable = status?.system_proxy_available !== false;
  const proxyLive = status?.system_proxy_applied === true;
  const proxyRecorded = status?.system_proxy_recorded === true;
  const canEnableProxy = proxyAvailable && !busy && !proxyLive;
  const canDisableProxy = proxyAvailable && !busy && running && proxyRecorded;

  function nodeLabel(n: NodeInfo): string {
    if (GROUP_TYPES.includes(n.outbound_type)) {
      const exit = n.group_now ? ` → ${n.group_now}` : "";
      return `${n.tag} (${n.outbound_type}${exit})`;
    }
    return `${n.tag} (${n.outbound_type})`;
  }

  return (
    <section className="panel">
      <h2>主页</h2>
      {proxyAvailable && running && status?.system_proxy_applied === false && (
        <p className="warn">系统代理未接管或已不同步</p>
      )}
      {!proxyAvailable && running && (
        <p className="muted">当前平台不支持系统代理接管</p>
      )}
      {error && <p className="error">{error}</p>}
      <dl className="kv">
        <dt>内核</dt>
        <dd className={`status status-${core?.status ?? "unknown"}`}>
          {core?.status ?? "—"}
        </dd>
        <dt>系统代理</dt>
        <dd>
          {!proxyAvailable
            ? "不支持"
            : proxyLive
              ? "已接管"
              : running
                ? proxyRecorded
                  ? "已不同步"
                  : "未接管"
                : "—"}
        </dd>
        <dt>订阅数</dt>
        <dd>{status?.subscription_count ?? "—"}</dd>
        <dt>入站</dt>
        <dd>
          {core?.inbound_host && core.inbound_port
            ? `${core.inbound_host}:${core.inbound_port}`
            : "—"}
        </dd>
        {running && connCount !== null && (
          <>
            <dt>连接</dt>
            <dd>{connCount}</dd>
          </>
        )}
        {core?.message && (
          <>
            <dt>消息</dt>
            <dd>{core.message}</dd>
          </>
        )}
      </dl>

      {nodes.length > 0 ? (
        <>
          <div className="node-row">
            <div className="mode-buttons" role="group" aria-label="模式">
              {(
                [
                  ["rule", "规则"],
                  ["global", "全局"],
                  ["direct", "直连"],
                ] as const
              ).map(([value, label]) => (
                <button
                  key={value}
                  type="button"
                  className={`mode-button${proxyMode === value ? " active" : ""}`}
                  disabled={modeBusy || busy}
                  onClick={() => void onSetMode(value)}
                >
                  {label}
                </button>
              ))}
            </div>
          </div>
          <div className="node-row">
            <label className="node-label">
              节点
              <select
                value={selectedTag}
                disabled={nodeBusy || busy}
                onChange={(e) => void onSelectNode(e.target.value)}
              >
                {nodes.map((n) => (
                  <option key={n.tag} value={n.tag}>
                    {nodeLabel(n)}
                  </option>
                ))}
              </select>
            </label>
            <button
              type="button"
              disabled={!running || !selectedTag || nodeBusy}
              onClick={() => void onTestDelay()}
            >
              测延迟
            </button>
            {delayMs !== null && (
              <span className="delay-badge">{delayMs} ms</span>
            )}
          </div>
        </>
      ) : (
        <EmptyState
          title={running ? "仅直连模式运行中" : "还没有可用节点"}
          description={
            running
              ? "当前没有订阅节点，所有流量直接连接。导入订阅后会自动切换到节点分流。需要时点击「启动代理服务」接管系统代理。"
              : "未导入任何订阅。打开软件会自动启动内核（仅直连）；点击「启动代理服务」接管系统代理，或先导入订阅。"
          }
          actionLabel="前往订阅页导入"
          onAction={() => onNavigate?.("subs")}
        />
      )}

      {proxyAvailable ? (
        <div className="actions">
          <button
            type="button"
            disabled={!canEnableProxy}
            onClick={() => void run(() => api.start())}
          >
            启动代理服务
          </button>
          <button
            type="button"
            disabled={!canDisableProxy}
            onClick={() => void run(() => api.stopSystemProxy())}
          >
            停止代理服务
          </button>
        </div>
      ) : null}

      <TrafficChart running={running} />
    </section>
  );
}
