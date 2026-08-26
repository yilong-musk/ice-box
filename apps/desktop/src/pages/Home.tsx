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
import { ErrorAlert, WarnAlert } from "../components/StatusAlert";
import { useGenerationGuard } from "../lib/generationGuard";
import { resolveSelectedTag } from "../lib/nodes";
import { TrafficChart } from "../components/TrafficChart";
import { ProxyPowerButton } from "../components/ProxyPowerButton";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";

type Props = {
  onBusyChange?: (busy: boolean) => void;
  onNavigate?: (tab: "subs") => void;
};

const GROUP_TYPES = ["selector", "urltest", "fallback", "loadbalance"];

function formatOutbound(tag: string, nodes: NodeInfo[]): string {
  const node = nodes.find((n) => n.tag === tag);
  if (!node) return tag;
  if (GROUP_TYPES.includes(node.outbound_type)) {
    return node.group_now
      ? `${node.tag} → ${node.group_now}`
      : `${node.tag}（${node.outbound_type}）`;
  }
  return `${node.tag}（${node.outbound_type}）`;
}

export function Home({ onBusyChange, onNavigate }: Props) {
  const { nextGeneration, isStale } = useGenerationGuard();
  const pollGenRef = useRef(0);
  const [status, setStatus] = useState<StatusResponse | null>(null);
  const [nodes, setNodes] = useState<NodeInfo[]>([]);
  const [selectedTag, setSelectedTag] = useState<string>("");
  const [proxyMode, setProxyMode] = useState<ProxyMode>("rule");
  const [connCount, setConnCount] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [modeBusy, setModeBusy] = useState(false);
  const modeBusyRef = useRef(false);
  const pendingRef = useRef(false);

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
      setSelectedTag(resolveSelectedTag(settings.selected_tag, n));
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
      }
    } catch (e) {
      // Mode switch / power toggle reloads the core; ignore poll failures mid-flight.
      if (
        gen === pollGenRef.current &&
        !modeBusyRef.current &&
        !pendingRef.current
      ) {
        setError(formatInvokeError(e));
      }
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
    // Invalidate in-flight poll so mid-start API misses cannot flash a red error.
    pollGenRef.current += 1;
    pendingRef.current = true;
    setPending(true);
    setError(null);
    try {
      await action();
      pollGenRef.current += 1;
      await refresh();
    } catch (e) {
      const message = formatInvokeError(e);
      pollGenRef.current += 1;
      await refresh();
      setError(message);
    } finally {
      pendingRef.current = false;
      setPending(false);
    }
  }

  async function onSetMode(mode: ProxyMode) {
    if (mode === proxyMode) return;
    const gen = nextGeneration();
    // Invalidate in-flight poll so a mid-reload sample cannot flash a red error.
    pollGenRef.current += 1;
    modeBusyRef.current = true;
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
      if (!isStale(gen)) {
        modeBusyRef.current = false;
        setModeBusy(false);
      }
    }
  }

  // Core follows the app; this control only toggles OS system proxy.
  const running = core?.status === "running";
  const proxyAvailable = status?.system_proxy_available !== false;
  const proxyLive = status?.system_proxy_applied === true;
  const proxyRecorded = status?.system_proxy_recorded === true;
  const canEnableProxy = proxyAvailable && !busy && !proxyLive;
  const canDisableProxy = proxyAvailable && !busy && running && proxyRecorded;
  // Treat live or on-disk recorded as "on" so out-of-sync can still restore.
  const proxyOn = proxyLive || (proxyRecorded && running);
  const canToggleProxy = proxyOn ? canDisableProxy : canEnableProxy;
  const outboundLabel =
    nodes.length === 0
      ? running
        ? "直连"
        : "—"
      : selectedTag
        ? formatOutbound(selectedTag, nodes)
        : "—";

  function onToggleProxy() {
    if (proxyOn) {
      void run(() => api.stopSystemProxy());
    } else {
      void run(() => api.start());
    }
  }

  return (
    <div className="flex flex-col gap-4">
      {proxyAvailable &&
        running &&
        proxyRecorded &&
        status?.system_proxy_applied === false && (
          <WarnAlert>系统代理未接管或已不同步</WarnAlert>
        )}
      {!proxyAvailable && running && (
        <p className="muted text-sm">当前平台不支持系统代理接管</p>
      )}
      {error && <ErrorAlert>{error}</ErrorAlert>}

      {proxyAvailable ? (
        <div className="flex justify-center">
          <ProxyPowerButton
            proxyOn={proxyOn}
            busy={busy}
            disabled={!canToggleProxy}
            ariaLabel={proxyOn ? "停止代理服务" : "启动代理服务"}
            title={proxyOn ? "停止代理服务" : "启动代理服务"}
            subtitle={
              busy
                ? "处理中…"
                : proxyLive
                  ? "系统代理已接管"
                  : proxyOn
                    ? "已记录，可恢复系统代理"
                    : "点击接管系统代理"
            }
            onClick={onToggleProxy}
          />
        </div>
      ) : null}

      <Card size="sm">
        <CardContent className="flex items-stretch gap-4">
          <dl className="grid min-w-0 flex-1 grid-cols-[5.5rem_1fr] gap-x-4 gap-y-2 text-sm">
            <dt className="text-muted-foreground">内核</dt>
            <dd className={`m-0 font-medium status status-${core?.status ?? "unknown"}`}>
              {core?.status ?? "—"}
            </dd>
            <dt className="text-muted-foreground">系统代理</dt>
            <dd className="m-0 font-medium">
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
            <dt className="text-muted-foreground">当前出站</dt>
            <dd className="m-0 truncate font-medium" title={outboundLabel}>
              {outboundLabel}
            </dd>
            <dt className="text-muted-foreground">订阅数</dt>
            <dd className="m-0 font-medium">{status?.subscription_count ?? "—"}</dd>
            <dt className="text-muted-foreground">入站</dt>
            <dd className="m-0 font-medium">
              {core?.inbound_host && core.inbound_port
                ? `${core.inbound_host}:${core.inbound_port}`
                : "—"}
            </dd>
            {running && connCount !== null && (
              <>
                <dt className="text-muted-foreground">连接</dt>
                <dd className="m-0 font-medium">{connCount}</dd>
              </>
            )}
            {core?.message && (
              <>
                <dt className="text-muted-foreground">消息</dt>
                <dd className="m-0 font-medium">{core.message}</dd>
              </>
            )}
          </dl>

          {nodes.length > 0 ? (
            <div
              className="flex w-20 shrink-0 flex-col gap-1"
              role="group"
              aria-label="模式"
            >
              {(
                [
                  ["rule", "规则"],
                  ["global", "全局"],
                  ["direct", "直连"],
                ] as const
              ).map(([value, label]) => (
                <Button
                  key={value}
                  type="button"
                  size="sm"
                  variant={proxyMode === value ? "default" : "outline"}
                  className="h-7"
                  aria-pressed={proxyMode === value}
                  disabled={modeBusy || busy}
                  onClick={() => void onSetMode(value)}
                >
                  {label}
                </Button>
              ))}
            </div>
          ) : null}
        </CardContent>
      </Card>

      {nodes.length === 0 ? (
        <EmptyState
          title={running ? "仅直连模式运行中" : "还没有可用节点"}
          description={
            running
              ? "当前没有订阅节点，所有流量直接连接。导入订阅后会自动切换到节点分流。需要时用上方大按钮接管系统代理。"
              : "未导入任何订阅。打开软件会自动启动内核（仅直连）；用上方大按钮接管系统代理，或先导入订阅。"
          }
          actionLabel="前往订阅页导入"
          onAction={() => onNavigate?.("subs")}
        />
      ) : null}

      <Card size="sm">
        <CardContent>
          <TrafficChart running={running} paused={modeBusy || busy} />
        </CardContent>
      </Card>
    </div>
  );
}
