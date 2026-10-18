// SPDX-License-Identifier: GPL-3.0-or-later

import { useCallback, useEffect, useRef, useState } from "react";
import { Copy, Power, Terminal } from "lucide-react";
import {
  api,
  formatInvokeError,
  formatUiMessage,
  type AppSettings,
  type CoreStatus,
  type CoreState,
  type McpStatus,
  type NodeInfo,
  type ProxyMode,
  type StatusResponse,
} from "../api/client";
import { isPhoneShell } from "@platform/shell";
import { EmptyState } from "../components/EmptyState";
import { ErrorAlert, WarnAlert } from "../components/StatusAlert";
import { useGenerationGuard } from "../lib/generationGuard";
import {
  formatMemory,
  formatMemoryPart,
  memoryLabel,
  memoryToneFor,
} from "../lib/memory";
import { RUNTIME_STATUS_FALLBACK_MS, useRuntimeStore } from "../lib/runtimeStore";
import {
  nodesEqual,
  nodesSnapshotRevision,
  readNodesSnapshot,
  subscribeNodesSnapshot,
} from "../lib/nodes";
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
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { TunInstallDialog, useTunInstallDialog } from "../components/TunInstallDialog";
import { cn } from "@/lib/utils";
import { t, useLanguagePreference, type MessageKey } from "../lib/i18n";
import { formatUptime } from "../lib/uptime";
import { resolveShellProxyEndpoint } from "../lib/shellProxy";

type Props = {
  onBusyChange?: (busy: boolean) => void;
  onNavigate?: (tab: "subs") => void;
  /** When false the page stays mounted but stops polling. */
  active?: boolean;
  /** Reports fresh status up to App so the global poll can be skipped while
   * this page is the active tab. */
  onStatus?: (status: StatusResponse) => void;
};

const PROXY_MODE_KEYS = [
  ["rule", "home.mode.rule"],
  ["global", "home.mode.global"],
  ["direct", "home.mode.direct"],
] as const satisfies ReadonlyArray<readonly [ProxyMode, MessageKey]>;

function formatCoreStatus(status: CoreStatus | undefined): string {
  switch (status) {
    case "running":
      return t("home.coreStatus.running");
    case "stopped":
      return t("home.coreStatus.stopped");
    case "starting":
      return t("home.coreStatus.starting");
    case "stopping":
      return t("home.coreStatus.stopping");
    case "error":
      return t("home.coreStatus.error");
    default:
      return t("common.dash");
  }
}

const GROUP_TYPES = ["selector", "urltest", "fallback", "loadbalance"];

function formatOutbound(tag: string, nodes: NodeInfo[]): string {
  const node = nodes.find((n) => n.tag === tag);
  if (!node) return tag;
  if (GROUP_TYPES.includes(node.outbound_type) && node.group_now) {
    return t("home.outboundGroupNow", { tag: node.tag, now: node.group_now });
  }
  return t("home.outboundTyped", {
    tag: node.tag,
    type: node.outbound_type,
  });
}

function formatSelectedOutbound(outbound: {
  tag: string;
  outbound_type: string;
  group_now: string | null;
}): string {
  if (GROUP_TYPES.includes(outbound.outbound_type) && outbound.group_now) {
    return t("home.outboundGroupNow", { tag: outbound.tag, now: outbound.group_now });
  }
  return t("home.outboundTyped", {
    tag: outbound.tag,
    type: outbound.outbound_type,
  });
}

/** Elapsed time since `since` (epoch ms), re-rendered once a second on its own
 * timer so the parent page does not re-render every tick. */
function RunUptime({ since }: { since: number }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);
  return <>{formatUptime(now - since)}</>;
}

/** OS network link as reported by the mobile webview (`online` / `offline`). */
function useNetworkOnline(): boolean {
  const [online, setOnline] = useState(
    () => typeof navigator !== "undefined" && navigator.onLine,
  );
  useEffect(() => {
    const sync = () => setOnline(navigator.onLine);
    window.addEventListener("online", sync);
    window.addEventListener("offline", sync);
    return () => {
      window.removeEventListener("online", sync);
      window.removeEventListener("offline", sync);
    };
  }, []);
  return online;
}

export function Home({ onBusyChange, onNavigate, active = true, onStatus }: Props) {
  useLanguagePreference();
  const phone = isPhoneShell();
  const networkOnline = useNetworkOnline();
  const { nextGeneration, isStale } = useGenerationGuard();
  const runtime = useRuntimeStore();
  const runtimeRef = useRef(runtime);
  runtimeRef.current = runtime;
  const pollGenRef = useRef(0);
  const [status, setStatus] = useState<StatusResponse | null>(null);
  const [mcpStatus, setMcpStatus] = useState<McpStatus | null>(null);
  const [nodes, setNodes] = useState<NodeInfo[]>([]);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [selectedTag, setSelectedTag] = useState<string>("");
  const [proxyMode, setProxyMode] = useState<ProxyMode>("rule");
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [modeBusy, setModeBusy] = useState(false);
  /** Optimistic TUN-toggle state while a settings save is in flight. Cleared
   * when fresh status arrives so the control snaps back on failure. */
  const [tunOverride, setTunOverride] = useState<boolean | null>(null);
  const tunInstall = useTunInstallDialog(onInstallHelperThenEnableTun);
  const modeBusyRef = useRef(false);
  const pendingRef = useRef(false);
  const tunSaveRef = useRef(false);
  const [tunSaving, setTunSaving] = useState(false);
  const activeRef = useRef(active);
  const settingsRef = useRef<AppSettings | null>(null);
  const [copiedCliProxy, setCopiedCliProxy] = useState(false);
  const copiedCliProxyTimerRef = useRef<number | null>(null);
  const statusRef = useRef<StatusResponse | null>(null);
  /** Wall time of the latest node-list snapshot. Compared with `sampled_at_ms`. */
  const snapWallRef = useRef(0);
  const homeActiveChainRef = useRef(Promise.resolve());

  const noteStatus = useCallback((next: StatusResponse) => {
    statusRef.current = next;
    setStatus(next);
    onStatus?.(next);
  }, [onStatus]);

  const refresh = useCallback(
    async (
      pollGen?: number,
      opts?: { settings?: boolean; status?: boolean },
    ) => {
      const gen = pollGen ?? pollGenRef.current;
      const startedRevision = nodesSnapshotRevision();
      const wantSettings = opts?.settings === true || settingsRef.current === null;
      const wantStatus = opts?.status !== false;
      try {
        const statusPromise = wantStatus
          ? api.getStatus()
          : Promise.resolve(null);
        const settingsPromise = wantSettings
          ? api.getSettings()
          : Promise.resolve(null);
        const mcpPromise = phone
          ? Promise.resolve(null)
          : api.getMcpStatus().catch(() => null);

        const [s, nextSettings, nextMcpStatus] = await Promise.all([
          statusPromise,
          settingsPromise,
          mcpPromise,
        ]);
        if (gen !== pollGenRef.current || !activeRef.current) return;
        if (nextMcpStatus) setMcpStatus(nextMcpStatus);

        // A node switch may land while this fetch is in flight. Keep the
        // snapshot selection instead of regressing to stale settings.
        const snap = readNodesSnapshot();
        if (nodesSnapshotRevision() !== startedRevision && snap) {
          if (nextSettings) {
            settingsRef.current = {
              ...nextSettings,
              selected_tag: snap.selectedTag,
            };
            setSettings(settingsRef.current);
            setProxyMode(nextSettings.proxy_mode);
          }
          if (s) noteStatus(s);
          if (!tunSaveRef.current) {
            setTunOverride(null);
          }
          setError(null);
          return;
        }

        if (s) noteStatus(s);
        if (!nextSettings && !settingsRef.current) return;
        if (nextSettings) {
          settingsRef.current = nextSettings;
          setSettings(nextSettings);
          setProxyMode(nextSettings.proxy_mode);
        }
        if (!tunSaveRef.current) {
          setTunOverride(null);
        }
        setError(null);
      } catch (e) {
        // Mode switch / power toggle reloads the core; ignore poll failures mid-flight.
        if (
          gen === pollGenRef.current &&
          activeRef.current &&
          !modeBusyRef.current &&
          !pendingRef.current &&
          !tunSaveRef.current
        ) {
          setError(formatInvokeError(e));
        }
      }
    },
    [noteStatus, phone],
  );

  useEffect(() => {
    return subscribeNodesSnapshot((snap) => {
      if (!snap) return;
      snapWallRef.current = Date.now();
      setSelectedTag((prev) => (prev === snap.selectedTag ? prev : snap.selectedTag));
      // The snapshot is the full node list. Home only needs the selected row,
      // without its member list, to label an in-flight switch.
      setNodes((prev) => {
        const node = snap.nodes.find((item) => item.tag === snap.selectedTag);
        const next = node
          ? [
              {
                tag: node.tag,
                outbound_type: node.outbound_type,
                group_now: node.group_now ?? null,
                group_all: null,
              },
            ]
          : [];
        return nodesEqual(prev, next) ? prev : next;
      });
      const current = settingsRef.current;
      if (current && current.selected_tag !== snap.selectedTag) {
        settingsRef.current = { ...current, selected_tag: snap.selectedTag };
      }
    });
  }, []);

  useEffect(() => {
    if (!runtime?.status) return;
    noteStatus(runtime.status);
    if (!tunSaveRef.current) {
      setTunOverride(null);
    }
  }, [runtime?.status, noteStatus]);

  // The tray menu can start/stop the service and switch the routing mode while
  // this page is on screen. The settings (mode) are otherwise read only on
  // mount, so re-read them here instead of waiting for the fallback poll; the
  // shared store covers status. A local action in flight owns the controls, and
  // its own trailing refresh converges.
  useEffect(() => {
    if (typeof api.listenStateChanged !== "function") return;
    let cancelled = false;
    let unlisten = () => {};
    void api
      .listenStateChanged(() => {
        if (pendingRef.current || modeBusyRef.current || tunSaveRef.current) {
          return;
        }
        pollGenRef.current += 1;
        void refresh(pollGenRef.current, {
          settings: true,
          status: runtimeRef.current == null,
        });
      })
      .then((off) => {
        if (cancelled) off();
        else unlisten = off;
      });
    return () => {
      cancelled = true;
      unlisten();
    };
  }, [refresh]);

  useEffect(() => {
    return () => {
      if (copiedCliProxyTimerRef.current !== null) {
        window.clearTimeout(copiedCliProxyTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    activeRef.current = active;
    let cancelled = false;
    const markHome = (visible: boolean) => {
      if (typeof api.setHomeActive !== "function") return;
      homeActiveChainRef.current = homeActiveChainRef.current.then(async () => {
        try {
          await api.setHomeActive(visible);
        } catch {
          // The website demo has no live group cache. Status still carries the exit.
        }
      });
    };
    markHome(active);
    void (async () => {
      await homeActiveChainRef.current;
      if (cancelled) return;
      pollGenRef.current += 1;
      if (!active) return;
      const gen = pollGenRef.current;
      const shareStatus = runtimeRef.current != null;
      if (shareStatus) {
        await runtimeRef.current?.refreshStatus();
        if (cancelled || gen !== pollGenRef.current) return;
      }
      void refresh(gen, { settings: true, status: !shareStatus });
    })();
    const id = window.setInterval(() => {
      if (pendingRef.current || modeBusyRef.current || tunSaveRef.current) {
        return;
      }
      if (document.visibilityState === "hidden") return;
      const rt = runtimeRef.current;
      if (rt && !rt.visible) return;
      pollGenRef.current += 1;
      void refresh(pollGenRef.current, { status: rt == null });
    }, RUNTIME_STATUS_FALLBACK_MS);
    return () => {
      cancelled = true;
      activeRef.current = false;
      pollGenRef.current += 1;
      window.clearInterval(id);
      markHome(false);
    };
  }, [active, refresh]);

  const core: CoreState | undefined = status?.core;
  const tunTransitioning =
    status?.tun_status === "preparing" || status?.tun_status === "stopping";
  const busy =
    pending ||
    status?.refresh_pending === true ||
    core?.status === "starting" ||
    core?.status === "stopping" ||
    tunTransitioning;

  useEffect(() => {
    onBusyChange?.(busy);
  }, [busy, onBusyChange]);

  async function run(action: () => Promise<void>) {
    // Invalidate in-flight poll so mid-start API misses cannot flash a red error.
    pollGenRef.current += 1;
    runtime?.bumpGeneration();
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

  /** Persist next-start TUN desire (and optional elevation / helper install)
   * without treating it as a live capture start/stop. The power button stays
   * on the current service state. */
  async function persistTunDesire(action: () => Promise<void>) {
    pollGenRef.current += 1;
    runtime?.bumpGeneration();
    tunSaveRef.current = true;
    setTunSaving(true);
    setError(null);
    try {
      await action();
      pollGenRef.current += 1;
      await refresh(undefined, { settings: true });
    } catch (e) {
      const message = formatInvokeError(e);
      pollGenRef.current += 1;
      await refresh(undefined, { settings: true });
      setError(message);
    } finally {
      tunSaveRef.current = false;
      setTunSaving(false);
      setTunOverride(null);
    }
  }

  async function onSetMode(mode: ProxyMode) {
    if (mode === proxyMode) return;
    const gen = nextGeneration();
    // Invalidate in-flight poll so a mid-reload sample cannot flash a red error.
    pollGenRef.current += 1;
    runtime?.bumpGeneration();
    modeBusyRef.current = true;
    setModeBusy(true);
    setError(null);
    setProxyMode(mode);
    try {
      await api.setProxyMode(mode);
      pollGenRef.current += 1;
      await refresh(undefined, { settings: true });
    } catch (e) {
      if (isStale(gen)) return;
      const message = formatInvokeError(e);
      pollGenRef.current += 1;
      await refresh(undefined, { settings: true });
      setError(message);
    } finally {
      if (!isStale(gen)) {
        modeBusyRef.current = false;
        setModeBusy(false);
      }
    }
  }

  // Core follows the app; this control toggles whichever capture backend is
  // active (system proxy or TUN, `docs/tun.md`) — the frontend never chooses.
  const running = core?.status === "running";
  const proxyAvailable = status?.system_proxy_available !== false;
  const proxyLive = status?.system_proxy_applied === true;
  const proxyRecorded = status?.system_proxy_recorded === true;
  const tunActive = status?.traffic_capture === "tun";
  // Windows hides the TUN controls entirely (gate blocked upstream): the
  // configured/active TUN states can never drive the power control there.
  const tunUiHidden = status?.tun_ui_hidden === true;
  const configuredTun =
    status?.configured_tun === true && !tunUiHidden;
  const tunAvailable = status?.tun_available === true && !tunUiHidden;
  // When TUN is the configured backend but the platform gate is pending /
  // failed, the button stays disabled and the unavailable reason is shown
  // (`docs/tun.md`: the setting remains a desired value, never a misleading state).
  const canEnableProxy =
    !busy &&
    !proxyLive &&
    !tunActive &&
    status?.tun_status !== "recovery_required" &&
    (configuredTun ? tunAvailable : proxyAvailable);
  const canDisableProxy =
    !busy && running && (proxyRecorded || (tunActive && tunAvailable));
  // Treat live or on-disk recorded as "on" so out-of-sync can still restore.
  const proxyOn = proxyLive || (proxyRecorded && running) || tunActive;
  const canToggleProxy = proxyOn ? canDisableProxy : canEnableProxy;
  const statusOutbound = status?.selected_outbound ?? null;
  // A selection that lands while a sample is in flight keeps the line. A later
  // sample (the tray, or the next poll) replaces it once its timestamp is newer.
  const snapshotWins =
    nodes.length > 0 &&
    selectedTag !== "" &&
    snapWallRef.current > (status?.sampled_at_ms ?? 0) &&
    selectedTag !== (statusOutbound?.tag ?? "");
  const hasNodes = snapshotWins
    ? true
    : status?.has_nodes === true || statusOutbound != null;
  const currentNodeLabel = !hasNodes
    ? running
      ? t("home.outboundDirect")
      : t("common.dash")
    : snapshotWins && selectedTag
      ? formatOutbound(selectedTag, nodes)
      : statusOutbound
        ? formatSelectedOutbound(statusOutbound)
        : t("common.dash");
  // A snapshot that just changed the selection still carries the previous
  // sample's landing address. Hide it until status catches up.
  const landingIp = snapshotWins ? null : statusOutbound?.landing_ip;
  const outboundRows: {
    label: string;
    value: string;
    valueClassName?: string;
    testId?: string;
  }[] = [{ label: t("home.outbound.node"), value: currentNodeLabel }];
  // Mobile status does not carry a landing IP and the compact layout cannot
  // afford a permanent "Exit IP —" row.
  if (!phone || landingIp) {
    outboundRows.push({
      label: t("home.outbound.ip"),
      value: landingIp || t("common.dash"),
      valueClassName: "font-mono tabular-nums",
      testId: "home-outbound-ip",
    });
  }

  const tunCardVisible =
    !tunUiHidden &&
    (phone
      ? configuredTun && !tunActive && !tunAvailable
      : proxyAvailable || configuredTun || tunActive);
  const tunnel = status?.tunnel_status ?? "stopped";
  const vpnActive =
    tunnel === "connected" ||
    tunnel === "connecting" ||
    tunnel === "disconnecting";

  function onToggleProxy() {
    if (phone) {
      void run(() => (vpnActive ? api.stop() : api.start()));
      return;
    }
    if (proxyOn) {
      void run(() => api.stopSystemProxy());
    } else {
      void run(() => api.start());
    }
  }

  async function onCopyCliProxy() {
    const endpoint = resolveShellProxyEndpoint(settings, {
      host: core?.inbound_host ?? null,
      port: core?.inbound_port ?? null,
    });
    if (!endpoint) return;
    setError(null);
    try {
      // Rust builds the one-liner and owns the clipboard, so the text cannot
      // drift from the env an opened terminal gets.
      await api.copyProxyCommand();
    } catch (err) {
      setError(formatInvokeError(err));
      return;
    }
    setCopiedCliProxy(true);
    if (copiedCliProxyTimerRef.current !== null) {
      window.clearTimeout(copiedCliProxyTimerRef.current);
    }
    copiedCliProxyTimerRef.current = window.setTimeout(() => {
      copiedCliProxyTimerRef.current = null;
      setCopiedCliProxy(false);
    }, 2000);
  }

  async function onOpenCliProxy() {
    setError(null);
    try {
      await api.openProxyTerminal();
    } catch (err) {
      setError(formatInvokeError(err));
    }
  }

  /** TUN setting switch on the home page: persists `tun.enabled` as the
   * desired backend for the *next* service start. It never starts or stops
   * the live proxy service. Enabling without an authorized helper guides
   * the user through install first (same flow as the Settings page). */
  function onToggleTunSetting(enabled: boolean) {
    if (!settings) return;
    setTunOverride(enabled);
    void persistTunDesire(async () => {
      await api.saveSettings({
        tun: { enabled },
      });
    });
  }

  function onInstallHelperThenEnableTun() {
    if (!settings) return;
    void persistTunDesire(async () => {
      await api.installHelper();
      await api.saveSettings({
        tun: { enabled: true },
      });
    });
  }

  /** Fallback offered after a TUN failure (`docs/tun.md`): disable the TUN
   * setting, then start the system proxy. Only offered when no TUN
   * resource is active and cleanup is not uncertain. */
  function onFallbackToSystemProxy() {
    if (!settings) return;
    void run(async () => {
      await api.saveSettings({
        tun: { enabled: false },
      });
      await api.start();
    });
  }

  function onRecoverTun() {
    void run(async () => {
      await api.recoverTun();
    });
  }

  /** In-app elevation (unsigned release): prompt the system authorization
   * dialog to install the privileged helper, then retry the TUN enable. */
  function onInstallHelper() {
    void run(async () => {
      await api.installHelper();
      await api.start();
    });
  }

  /** Windows (plan B): create the scheduled task (one UAC), then retry start. */
  function onEnsureTunElevation() {
    void run(async () => {
      await api.ensureTunElevation();
      await api.start();
    });
  }

  const inboundLabel =
    core?.inbound_host && core.inbound_port
      ? `${core.inbound_host}:${core.inbound_port}`
      : t("common.dash");
  const cliProxyEndpoint = resolveShellProxyEndpoint(settings, {
    host: core?.inbound_host ?? null,
    port: core?.inbound_port ?? null,
  });
  const emptyTitle = running
    ? t("home.empty.runningTitle")
    : t("home.empty.idleTitle");
  const emptyDescription = running
    ? t("home.empty.runningDesc")
    : t("home.empty.idleDesc");
  // Memory is core + app main-process memory — the physical footprint on
  // macOS, the private working set on Windows.
  const memory = status?.memory;
  const memoryToneValue = memoryToneFor(memory);
  const memoryTip = memory
    ? memory.core_bytes != null
      ? t("home.memory.tooltip", {
          core: formatMemory(memory.core_bytes),
          app: formatMemoryPart(memory.app_bytes),
        })
      : running
        ? t("home.memory.tooltipUnreadable", {
            app: formatMemoryPart(memory.app_bytes),
          })
        : t("home.memory.tooltipAppOnly", {
            app: formatMemoryPart(memory.app_bytes),
          })
    : undefined;
  const mcpSupported = mcpStatus?.supported === true;
  const mcpRunning = mcpStatus?.supported === true && mcpStatus.running;
  const mcpValue = mcpRunning
    ? t("mcp.running")
    : mcpSupported
      ? t("mcp.stopped")
      : t("common.dash");
  const platformStatusRow = phone
    ? {
        label: t("home.health.network"),
        value: networkOnline
          ? t("home.network.online")
          : t("home.network.offline"),
        valueClassName: networkOnline ? "text-ok" : "text-destructive",
        testId: "home-health-network",
      }
    : {
        label: t("home.health.mcp"),
        value: mcpValue,
        title: mcpStatus?.error ?? undefined,
        valueClassName: mcpRunning
          ? "text-ok"
          : mcpStatus?.error
            ? "text-destructive"
            : mcpSupported
              ? "text-muted-foreground"
              : undefined,
        testId: "home-health-mcp",
      };
  const infoRows: {
    label: string;
    value: string;
    valueClassName?: string;
    title?: string;
    testId?: string;
  }[] = [
    {
      label: t("home.health.core"),
      value: formatCoreStatus(core?.status),
      valueClassName: `status status-${core?.status ?? "unknown"}`,
      testId: "home-health-core",
    },
    {
      label: t("home.health.memory"),
      value: memoryLabel(memory),
      title: memoryTip,
      testId: "home-health-memory",
      valueClassName: cn(
        memoryToneValue === "ok" && "text-ok",
        memoryToneValue === "warn" && "text-warn",
        memoryToneValue === "bad" && "text-destructive",
      ),
    },
    platformStatusRow,
  ];

  const powerOn = phone ? vpnActive : proxyOn;
  const canToggle = phone ? !busy : canToggleProxy;
  const powerTitle = phone
    ? vpnActive
      ? t("home.power.vpnStop")
      : t("home.power.vpnStart")
    : proxyOn
      ? t("home.power.stop")
      : t("home.power.start");
  const permissionRequired = status?.tun_status === "permission_required";
  const recoveryRequired = status?.tun_status === "recovery_required";

  return (
    <div
      className={cn(
        "home-panel flex min-h-0 flex-1 flex-col gap-3",
        phone && "overflow-y-auto overscroll-contain",
      )}
      data-testid="home-panel"
    >
      {phone && status?.vpn_permission === "denied" && (
        <WarnAlert className="shrink-0">{t("home.warn.vpnPermission")}</WarnAlert>
      )}
      {!phone &&
        proxyAvailable &&
        running &&
        proxyRecorded &&
        status?.system_proxy_applied === false && (
          <WarnAlert className="shrink-0">
            {t("home.warn.proxyOutOfSync")}
          </WarnAlert>
        )}
      {!phone && permissionRequired && (
        <WarnAlert className="shrink-0">
          {status?.helper_supported
            ? t("home.warn.permissionRequired")
            : t("home.warn.permissionRequiredNoHelper")}
          <span className="mt-2 flex flex-wrap gap-2">
            {status?.helper_supported && (
              <Button
                type="button"
                size="sm"
                onClick={onInstallHelper}
                disabled={busy || status?.helper_installed === true}
              >
                {t("home.installHelper")}
              </Button>
            )}
            {status?.helper_supported === false && (
              <Button
                type="button"
                size="sm"
                onClick={onEnsureTunElevation}
                disabled={busy}
              >
                {t("home.ensureTunElevation")}
              </Button>
            )}
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={onFallbackToSystemProxy}
              disabled={busy}
            >
              {t("home.fallbackToSystemProxy")}
            </Button>
          </span>
        </WarnAlert>
      )}
      {!phone && recoveryRequired && (
        <ErrorAlert className="shrink-0">
          {t("home.warn.recoveryRequired")}
          <span className="mt-2 flex flex-wrap gap-2">
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={onRecoverTun}
              disabled={busy}
            >
              {t("home.retryRecovery")}
            </Button>
          </span>
        </ErrorAlert>
      )}
      {error && <ErrorAlert className="shrink-0">{error}</ErrorAlert>}

      <div
        className={cn(
          "grid gap-3",
          phone
            ? "grid-cols-1"
            : "min-h-0 flex-1 grid-cols-3 grid-rows-[auto_minmax(0,1fr)_minmax(0,1fr)]",
        )}
      >
        <Card size="sm" className="w-full min-w-0">
          <CardHeader>
            <CardTitle className="text-xs font-medium text-muted-foreground">
              {t("home.proxyStatus")}
            </CardTitle>
          </CardHeader>
          <CardContent className="flex flex-1 flex-col">
            {phone || proxyAvailable || configuredTun || tunActive ? (
              <div className="flex w-full flex-1 flex-col justify-between gap-3">
                <Button
                  type="button"
                  size="lg"
                  variant={powerOn ? "default" : "outline"}
                  className="h-auto min-h-11 w-full justify-center gap-2 py-2"
                  disabled={!canToggle}
                  aria-pressed={powerOn}
                  aria-label={powerTitle}
                  onClick={onToggleProxy}
                >
                  <Power />
                  <span className="text-sm font-medium">{powerTitle}</span>
                </Button>
                {tunCardVisible && (
                  <div className="flex items-center gap-3 whitespace-nowrap">
                    <span className="text-xs font-medium text-muted-foreground">
                      {t("home.tunMode")}
                    </span>
                    {configuredTun && !tunActive && !tunAvailable && (
                      <span
                        className="min-w-0 truncate text-xs text-muted-foreground"
                        title={formatUiMessage(status?.tun_unavailable_reason) || t("home.tunUnavailable")}
                      >
                        {formatUiMessage(status?.tun_unavailable_reason) || t("home.tunUnavailable")}
                      </span>
                    )}
                    {!phone && (
                      <div className="ml-auto flex shrink-0 items-center">
                        <Switch
                          checked={tunOverride ?? configuredTun}
                          onCheckedChange={(pressed) => {
                            const s = settings;
                            if (pressed) {
                              if (!s) return;
                              if (
                                status?.helper_supported === true &&
                                status?.helper_installed !== true
                              ) {
                                // No authorized helper (macOS): guide the user to
                                // install it first; the TUN-on setting is persisted only
                                // after a successful install (cancel leaves it off).
                                tunInstall.setOpen(true);
                                return;
                              }
                              if (status?.helper_supported === false) {
                                // Windows (plan B): a one-time elevation component (a
                                // scheduled task) makes TUN work without any further
                                // prompt. The first enable triggers a single UAC to
                                // install it, then persists the next-start desire.
                                // Never start/stop the proxy service from this switch.
                                setTunOverride(true);
                                void persistTunDesire(async () => {
                                  if (status?.tun_elevation_ready !== true) {
                                    await api.ensureTunElevation();
                                  }
                                  await api.saveSettings({
                                    tun: { enabled: true },
                                  });
                                });
                                return;
                              }
                              onToggleTunSetting(true);
                            } else {
                              onToggleTunSetting(false);
                            }
                          }}
                          disabled={
                            busy ||
                            tunSaving ||
                            !settings ||
                            status?.tun_available === false ||
                            status?.helper_stale === true
                          }
                          size="lg"
                          aria-label={t("home.tunMode")}
                        />
                      </div>
                    )}
                  </div>
                )}
                {running && status?.running_since_ms != null && (
                  <div className="flex items-center justify-between gap-3">
                    <span className="whitespace-nowrap text-xs font-medium text-muted-foreground">
                      {t("home.runUptime")}
                    </span>
                    <span
                      className="font-mono text-sm tabular-nums"
                      data-testid="home-run-uptime"
                    >
                      <RunUptime since={status.running_since_ms} />
                    </span>
                  </div>
                )}
              </div>
            ) : (
              <p className="muted text-sm">
                {t("home.unsupported")}
              </p>
            )}
          </CardContent>
        </Card>

        <Card size="sm" className="w-full min-w-0">
          <CardHeader className="[container-type:normal]">
            <CardTitle className="whitespace-nowrap text-xs font-medium text-muted-foreground">
              {t("home.modeAria")}
            </CardTitle>
          </CardHeader>
          <CardContent className="flex flex-1 flex-col">
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              orientation="vertical"
              spacing={2}
              value={proxyMode}
              onValueChange={(value) => {
                if (value === "rule" || value === "global" || value === "direct") {
                  void onSetMode(value);
                }
              }}
              disabled={modeBusy || busy}
              className="flex w-full flex-1 flex-col"
              aria-label={t("home.modeAria")}
            >
              {PROXY_MODE_KEYS.map(([mode, labelKey]) => (
                <ToggleGroupItem key={mode} value={mode} className="h-auto w-full flex-1">
                  {t(labelKey)}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </CardContent>
        </Card>

        <Card size="sm" className="w-full min-w-0">
          <CardHeader>
            <CardTitle className="text-xs font-medium text-muted-foreground">
              {t("home.healthTitle")}
            </CardTitle>
          </CardHeader>
          <CardContent>
            <ItemGroup className="w-full gap-0">
              {infoRows.map((row, index) => (
                <div key={row.label}>
                  {index > 0 ? <ItemSeparator className="my-0" /> : null}
                  <Item size="xs" className="justify-between rounded-none px-0">
                    <ItemDescription>{row.label}</ItemDescription>
                    <ItemTitle
                      className={row.valueClassName}
                      title={row.title ?? row.value}
                      data-testid={row.testId}
                    >
                      {row.value}
                    </ItemTitle>
                  </Item>
                </div>
              ))}
            </ItemGroup>
          </CardContent>
        </Card>

        <div
          className={cn(
            "flex min-w-0 flex-col gap-3",
            !phone &&
              "col-start-1 row-span-2 row-start-2 min-h-0 overflow-y-auto",
          )}
        >
          <Card
            size="sm"
            className="flex w-full min-h-fit min-w-0 flex-1 flex-col"
          >
            <CardHeader>
              <CardTitle className="text-xs font-medium text-muted-foreground">
                {t("home.outboundTitle")}
              </CardTitle>
            </CardHeader>
            <CardContent className="flex flex-1 flex-col">
              <ItemGroup className="w-full flex-1 gap-0">
                {outboundRows.map((row, index) => (
                  <div key={row.label} className="flex flex-1 flex-col">
                    {index > 0 ? <ItemSeparator className="my-0" /> : null}
                    <Item size="xs" className="flex-1 justify-between rounded-none px-0">
                      <ItemDescription className="whitespace-nowrap">{row.label}</ItemDescription>
                      <ItemTitle
                        className={cn("min-w-0 flex-1 justify-end", row.valueClassName)}
                        title={row.value}
                        data-testid={row.testId}
                      >
                        <span className="min-w-0 truncate">{row.value}</span>
                      </ItemTitle>
                    </Item>
                  </div>
                ))}
              </ItemGroup>
            </CardContent>
          </Card>

          <Card size="sm" className="flex w-full min-h-fit min-w-0 flex-1 flex-col">
            <CardHeader>
              <CardTitle className="text-xs font-medium text-muted-foreground">
                {t("home.inboundTitle")}
              </CardTitle>
            </CardHeader>
            <CardContent className="flex min-h-0 flex-1 flex-col justify-between gap-3">
              <p className="text-center font-mono text-sm tabular-nums" data-testid="home-inbound-address">
                {inboundLabel}
              </p>
              {!phone && cliProxyEndpoint ? (
                <div className="flex flex-col gap-2">
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    className="w-full justify-center gap-2"
                    aria-label={t("home.copyCliProxy")}
                    title={t("home.copyCliProxyHint")}
                    onClick={() => void onCopyCliProxy()}
                  >
                    <Copy />
                    <span className="truncate">
                      {copiedCliProxy
                        ? t("home.copyCliProxyCopied")
                        : t("home.copyCliProxy")}
                    </span>
                  </Button>
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    className="w-full justify-center gap-2"
                    aria-label={t("home.openCliProxy")}
                    title={t("home.openCliProxyHint")}
                    onClick={() => void onOpenCliProxy()}
                  >
                    <Terminal />
                    <span className="truncate">{t("home.openCliProxy")}</span>
                  </Button>
                </div>
              ) : null}
            </CardContent>
          </Card>
        </div>

        <Card
          size="sm"
          className={cn(
            "flex w-full min-w-0 flex-col",
            !phone && "col-span-2 col-start-2 row-span-2 row-start-2 min-h-0",
          )}
        >
          <CardHeader className="shrink-0">
            <CardTitle className="text-xs font-medium text-muted-foreground">
              {t("home.trafficTitle")}
            </CardTitle>
            <CardDescription>{t("home.trafficDesc")}</CardDescription>
          </CardHeader>
          <CardContent className="flex min-h-0 flex-1 flex-col">
            {!hasNodes && !running ? (
              <EmptyState
                framed={false}
                className="my-auto"
                title={emptyTitle}
                description={emptyDescription}
                actionLabel={t("home.goToSubs")}
                onAction={() => onNavigate?.("subs")}
              />
            ) : (
              <>
                {!hasNodes ? (
                  <div className="mb-3 flex shrink-0 flex-wrap items-center justify-between gap-3">
                    <div className="min-w-0">
                      <p className="text-sm font-medium">{emptyTitle}</p>
                      <p className="text-xs text-muted-foreground">
                        {t("home.autoSwitchHint")}
                      </p>
                    </div>
                    <Button
                      type="button"
                      size="sm"
                      onClick={() => onNavigate?.("subs")}
                    >
                      {t("home.goToSubs")}
                    </Button>
                  </div>
                ) : null}
                <TrafficChart
                  className="min-h-0"
                  running={running}
                  paused={modeBusy || busy || !active}
                />
              </>
            )}
          </CardContent>
        </Card>

      </div>

      {!tunUiHidden && (
        <TunInstallDialog
          open={tunInstall.open}
          onOpenChange={tunInstall.setOpen}
          onConfirm={tunInstall.confirm}
          busy={tunSaving}
        />
      )}
    </div>
  );
}
