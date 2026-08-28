import { useCallback, useEffect, useRef, useState } from "react";
import { Power } from "lucide-react";
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
import { resolveSelectedTag, writeNodesSnapshot } from "../lib/nodes";
import { TrafficChart } from "../components/TrafficChart";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Item,
  ItemDescription,
  ItemGroup,
  ItemSeparator,
  ItemTitle,
} from "@/components/ui/item";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";

type Props = {
  onBusyChange?: (busy: boolean) => void;
  onNavigate?: (tab: "subs") => void;
  /** When false the page stays mounted but stops polling. */
  active?: boolean;
};

const GROUP_TYPES = ["selector", "urltest", "fallback", "loadbalance"];

const PROXY_MODES = [
  ["rule", "规则"],
  ["global", "全局"],
  ["direct", "直连"],
] as const;

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

export function Home({ onBusyChange, onNavigate, active = true }: Props) {
  const { nextGeneration, isStale } = useGenerationGuard();
  const pollGenRef = useRef(0);
  const [status, setStatus] = useState<StatusResponse | null>(null);
  const [nodes, setNodes] = useState<NodeInfo[]>([]);
  const [selectedTag, setSelectedTag] = useState<string>("");
  const [proxyMode, setProxyMode] = useState<ProxyMode>("rule");
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [modeBusy, setModeBusy] = useState(false);
  const modeBusyRef = useRef(false);
  const pendingRef = useRef(false);
  const activeRef = useRef(active);

  const refresh = useCallback(async (pollGen?: number) => {
    const gen = pollGen ?? pollGenRef.current;
    try {
      const [s, n, settings] = await Promise.all([
        api.getStatus(),
        api.listNodes(),
        api.getSettings(),
      ]);
      if (gen !== pollGenRef.current || !activeRef.current) return;

      const selected = resolveSelectedTag(settings.selected_tag, n);
      setStatus(s);
      setNodes(n);
      setProxyMode(settings.proxy_mode);
      setSelectedTag(selected);
      writeNodesSnapshot({
        nodes: n,
        selectedTag: selected,
        running: s.core.status === "running",
      });
      setError(null);
    } catch (e) {
      // Mode switch / power toggle reloads the core; ignore poll failures mid-flight.
      if (
        gen === pollGenRef.current &&
        activeRef.current &&
        !modeBusyRef.current &&
        !pendingRef.current
      ) {
        setError(formatInvokeError(e));
      }
    }
  }, []);

  useEffect(() => {
    activeRef.current = active;
    pollGenRef.current += 1;
    if (!active) return;
    const gen = pollGenRef.current;
    void refresh(gen);
    const id = window.setInterval(() => {
      if (pendingRef.current || modeBusyRef.current) return;
      pollGenRef.current += 1;
      void refresh(pollGenRef.current);
    }, 2000);
    return () => {
      activeRef.current = false;
      pollGenRef.current += 1;
      window.clearInterval(id);
    };
  }, [active, refresh]);

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

  const inboundLabel =
    core?.inbound_host && core.inbound_port
      ? `${core.inbound_host}:${core.inbound_port}`
      : "—";
  const emptyTitle = running ? "仅直连模式运行中" : "还没有可用节点";
  const emptyDescription = running
    ? "当前没有订阅节点，所有流量直接连接。导入订阅后会自动切换到节点分流。需要时用上方大按钮接管系统代理。"
    : "未导入任何订阅。打开软件会自动启动内核（仅直连）；用上方大按钮接管系统代理，或先导入订阅。";
  const infoRows: { label: string; value: string; valueClassName?: string }[] = [
    {
      label: "内核",
      value: core?.status ?? "—",
      valueClassName: `status status-${core?.status ?? "unknown"}`,
    },
    { label: "当前出站", value: outboundLabel },
    {
      label: "入站",
      value: inboundLabel,
      valueClassName: "font-mono tabular-nums",
    },
  ];
  if (core?.message) {
    infoRows.push({ label: "消息", value: core.message });
  }

  const powerTitle = proxyOn ? "停止代理服务" : "启动代理服务";
  const powerSubtitle = busy
    ? "处理中…"
    : proxyLive
      ? "系统代理已接管"
      : proxyOn
        ? "已记录，可恢复系统代理"
        : "点击接管系统代理";

  return (
    <div className="home-panel flex min-h-0 flex-1 flex-col gap-3">
      {proxyAvailable &&
        running &&
        proxyRecorded &&
        status?.system_proxy_applied === false && (
          <WarnAlert className="shrink-0">系统代理未接管或已不同步</WarnAlert>
        )}
      {error && <ErrorAlert className="shrink-0">{error}</ErrorAlert>}

      <div className="grid shrink-0 grid-cols-3 items-stretch gap-3">
        <Card size="sm" className="min-w-0 data-[size=sm]:[--card-spacing:--spacing(2)]">
          <CardHeader>
            <CardTitle>代理状态</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-1 flex-col">
            {proxyAvailable ? (
              <Button
                type="button"
                size="lg"
                variant={proxyOn ? "default" : "outline"}
                className="h-auto w-full justify-start gap-2 py-2"
                disabled={!canToggleProxy}
                aria-pressed={proxyOn}
                aria-label={powerTitle}
                onClick={onToggleProxy}
              >
                <Power />
                <span className="min-w-0 text-left">
                  <span className="block text-sm font-medium">{powerTitle}</span>
                  <span
                    className={cn(
                      "block text-xs font-normal",
                      proxyOn
                        ? "text-primary-foreground/80"
                        : "text-muted-foreground",
                    )}
                  >
                    {powerSubtitle}
                  </span>
                </span>
              </Button>
            ) : (
              <p className="muted text-sm">当前平台不支持系统代理接管</p>
            )}
          </CardContent>
        </Card>

        <Card size="sm" className="min-w-0 data-[size=sm]:[--card-spacing:--spacing(2)]">
          <CardHeader>
            <CardTitle>信息</CardTitle>
          </CardHeader>
          <CardContent>
            <ItemGroup className="gap-0">
              {infoRows.map((row, index) => (
                <div key={row.label}>
                  {index > 0 ? <ItemSeparator className="my-0" /> : null}
                  <Item size="xs" className="justify-between px-0">
                    <ItemDescription>{row.label}</ItemDescription>
                    <ItemTitle className={row.valueClassName} title={row.value}>
                      {row.value}
                    </ItemTitle>
                  </Item>
                </div>
              ))}
            </ItemGroup>
          </CardContent>
        </Card>

        <Card size="sm" className="min-w-0 data-[size=sm]:[--card-spacing:--spacing(2)]">
          <CardHeader>
            <CardTitle>代理模式</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-1 flex-col">
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              orientation="horizontal"
              spacing={2}
              value={proxyMode}
              onValueChange={(value) => {
                if (value === "rule" || value === "global" || value === "direct") {
                  void onSetMode(value);
                }
              }}
              disabled={modeBusy || busy}
              className="w-full"
              aria-label="模式"
            >
              {PROXY_MODES.map(([mode, label]) => (
                <ToggleGroupItem key={mode} value={mode} className="flex-1">
                  {label}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </CardContent>
        </Card>
      </div>

      <Card size="sm" className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <CardHeader className="shrink-0">
          <CardTitle>流量</CardTitle>
          <CardDescription>最近 60 秒上下行</CardDescription>
        </CardHeader>
        <CardContent className="flex min-h-0 flex-1 flex-col">
          {nodes.length === 0 && !running ? (
            <EmptyState
              framed={false}
              className="my-auto"
              title={emptyTitle}
              description={emptyDescription}
              actionLabel="前往订阅页导入"
              onAction={() => onNavigate?.("subs")}
            />
          ) : (
            <>
              {nodes.length === 0 ? (
                <div className="mb-3 flex shrink-0 flex-wrap items-center justify-between gap-3">
                  <div className="min-w-0">
                    <p className="text-sm font-medium">{emptyTitle}</p>
                    <p className="text-xs text-muted-foreground">
                      导入订阅后会自动切换到节点分流。
                    </p>
                  </div>
                  <Button
                    type="button"
                    size="sm"
                    onClick={() => onNavigate?.("subs")}
                  >
                    前往订阅页导入
                  </Button>
                </div>
              ) : null}
              <TrafficChart
                className="min-h-0 flex-1"
                running={running}
                paused={modeBusy || busy || !active}
              />
            </>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
