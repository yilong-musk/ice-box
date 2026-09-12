// SPDX-License-Identifier: GPL-3.0-or-later

import { useCallback, useEffect, useRef, useState } from "react";
import { Power } from "lucide-react";
import {
  api,
  formatInvokeError,
  formatUiMessage,
  type AppSettings,
  type CoreStatus,
  type CoreState,
  type NodeInfo,
  type ProxyMode,
  type StatusResponse,
} from "../api/tauri";
import { EmptyState } from "../components/EmptyState";
import { ErrorAlert, WarnAlert } from "../components/StatusAlert";
import { useGenerationGuard } from "../lib/generationGuard";
import { RUNTIME_STATUS_FALLBACK_MS, useRuntimeStore } from "../lib/runtimeStore";
import {
  nodesEqual,
  nodesSnapshotRevision,
  readNodesSnapshot,
  resolveSelectedTag,
  subscribeNodesSnapshot,
  writeNodesSnapshot,
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
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Toggle } from "@/components/ui/toggle";
import { TunInstallDialog, useTunInstallDialog } from "../components/TunInstallDialog";
import { cn } from "@/lib/utils";
import { t, useLanguagePreference, type MessageKey } from "../lib/i18n";

type Props = {
  onBusyChange?: (busy: boolean) => void;
  onNavigate?: (tab: "subs") => void;
  /** When false the page stays mounted but stops polling. */
  active?: boolean;
  /** Reports fresh status up to App so the global poll can be skipped while
   * this page is the active tab. */
  onStatus?: (status: StatusResponse) => void;
};

const GROUP_TYPES = ["selector", "urltest", "fallback", "loadbalance"];

const PROXY_MODE_KEYS = [
  ["rule", "home.mode.rule"],
  ["global", "home.mode.global"],
  ["direct", "home.mode.direct"],
] as const satisfies ReadonlyArray<readonly [ProxyMode, MessageKey]>;

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

export function Home({ onBusyChange, onNavigate, active = true, onStatus }: Props) {
  useLanguagePreference();
  const { nextGeneration, isStale } = useGenerationGuard();
  const runtime = useRuntimeStore();
  const runtimeRef = useRef(runtime);
  runtimeRef.current = runtime;
  const pollGenRef = useRef(0);
  const [status, setStatus] = useState<StatusResponse | null>(null);
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
  const statusRef = useRef<StatusResponse | null>(null);

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
        const nodesPromise = api.listNodes();
        const settingsPromise = wantSettings
          ? api.getSettings()
          : Promise.resolve(null);

        const s = wantStatus ? await statusPromise : null;
        if (gen !== pollGenRef.current || !activeRef.current) return;
        if (s) {
          statusRef.current = s;
          setStatus(s);
          onStatus?.(s);
        }

        const [n, nextSettings] = await Promise.all([
          nodesPromise,
          settingsPromise,
        ]);
        if (gen !== pollGenRef.current || !activeRef.current) return;
        const settings = nextSettings ?? settingsRef.current;
        if (!settings) return;

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
          if (!tunSaveRef.current) {
            setTunOverride(null);
          }
          setError(null);
          return;
        }

        const selected = resolveSelectedTag(settings.selected_tag, n);
        setNodes(n);
        if (nextSettings) {
          settingsRef.current = nextSettings;
          setSettings(nextSettings);
          setProxyMode(nextSettings.proxy_mode);
        }
        if (!tunSaveRef.current) {
          setTunOverride(null);
        }
        setSelectedTag(selected);
        const coreStatus =
          s?.core.status ?? statusRef.current?.core.status ?? "stopped";
        writeNodesSnapshot({
          nodes: n,
          selectedTag: selected,
          running: coreStatus === "running",
        });
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
    [],
  );

  useEffect(() => {
    return subscribeNodesSnapshot((snap) => {
      if (!snap) return;
      setSelectedTag((prev) => (prev === snap.selectedTag ? prev : snap.selectedTag));
      setNodes((prev) => (nodesEqual(prev, snap.nodes) ? prev : snap.nodes));
      const current = settingsRef.current;
      if (current && current.selected_tag !== snap.selectedTag) {
        settingsRef.current = { ...current, selected_tag: snap.selectedTag };
      }
    });
  }, []);

  useEffect(() => {
    if (!runtime?.status) return;
    statusRef.current = runtime.status;
    setStatus(runtime.status);
    onStatus?.(runtime.status);
    if (!tunSaveRef.current) {
      setTunOverride(null);
    }
  }, [runtime?.status, onStatus]);

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
        void refresh(pollGenRef.current, { settings: true });
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
    activeRef.current = active;
    pollGenRef.current += 1;
    if (!active) return;
    const gen = pollGenRef.current;
    const shareStatus = runtime != null;
    void refresh(gen, { settings: true, status: !shareStatus });
    const id = window.setInterval(() => {
      if (pendingRef.current || modeBusyRef.current || tunSaveRef.current) {
        return;
      }
      if (document.visibilityState === "hidden") return;
      const rt = runtimeRef.current;
      if (rt && !rt.visible) return;
      pollGenRef.current += 1;
      void refresh(pollGenRef.current, { status: !shareStatus });
    }, RUNTIME_STATUS_FALLBACK_MS);
    return () => {
      activeRef.current = false;
      pollGenRef.current += 1;
      window.clearInterval(id);
    };
  }, [active, refresh, runtime != null]);

  const core: CoreState | undefined = status?.core;
  const tunTransitioning =
    status?.tun_status === "preparing" || status?.tun_status === "stopping";
  const busy =
    pending ||
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
  const outboundLabel =
    nodes.length === 0
      ? running
        ? t("home.outboundDirect")
        : t("common.dash")
      : selectedTag
        ? formatOutbound(selectedTag, nodes)
        : t("common.dash");

  function onToggleProxy() {
    if (proxyOn) {
      void run(() => api.stopSystemProxy());
    } else {
      void run(() => api.start());
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
  const emptyTitle = running
    ? t("home.empty.runningTitle")
    : t("home.empty.idleTitle");
  const emptyDescription = running
    ? t("home.empty.runningDesc")
    : t("home.empty.idleDesc");
  const captureLabel =
    status?.traffic_capture === "tun"
      ? t("home.capture.tun", {
          iface: status.tun_interface
            ? t("common.withIface", { iface: status.tun_interface })
            : "",
        })
      : status?.traffic_capture === "system_proxy"
        ? t("home.capture.systemProxy")
        : t("home.capture.none");
  const infoRows: { label: string; value: string; valueClassName?: string }[] = [
    {
      label: t("home.info.core"),
      value: formatCoreStatus(core?.status),
      valueClassName: `status status-${core?.status ?? "unknown"}`,
    },
    { label: t("home.info.capture"), value: captureLabel },
    { label: t("home.info.outbound"), value: outboundLabel },
    {
      label: t("home.info.inbound"),
      value: inboundLabel,
      valueClassName: "font-mono tabular-nums",
    },
  ];
  if (core?.message) {
    infoRows.push({
      label: t("home.info.message"),
      value: formatUiMessage(core.message),
    });
  }

  const powerTitle = proxyOn ? t("home.power.stop") : t("home.power.start");
  const powerSubtitle = busy
    ? t("home.power.busy")
    : tunActive
      ? t("home.power.tunActive", {
          iface: status?.tun_interface
            ? t("common.withIface", { iface: status.tun_interface })
            : "",
        })
      : proxyLive
        ? t("home.power.proxyLive")
        : proxyOn
          ? t("home.power.recorded")
          : configuredTun
            ? t("home.power.tunReady")
            : t("home.power.clickToCapture");

  const permissionRequired = status?.tun_status === "permission_required";
  const recoveryRequired = status?.tun_status === "recovery_required";

  return (
    <div className="home-panel flex min-h-0 flex-1 flex-col gap-3" data-testid="home-panel">
      {proxyAvailable &&
        running &&
        proxyRecorded &&
        status?.system_proxy_applied === false && (
          <WarnAlert className="shrink-0">
            {t("home.warn.proxyOutOfSync")}
          </WarnAlert>
        )}
      {permissionRequired && (
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
      {recoveryRequired && (
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

      <div className="grid shrink-0 grid-cols-2 items-stretch gap-3">
        <Card size="sm" className="min-w-0 data-[size=sm]:[--card-spacing:--spacing(2)]">
          <CardHeader>
            <CardTitle>{t("home.proxyStatus")}</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-1 flex-col">
            {proxyAvailable || configuredTun || tunActive ? (
              <>
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
                {configuredTun && !tunActive && !tunAvailable && (
                  <p className="mt-2 text-xs text-muted-foreground">
                    {formatUiMessage(status?.tun_unavailable_reason) ||
                      t("home.tunUnavailable")}
                  </p>
                )}
              </>
            ) : (
              <p className="muted text-sm">
                {t("home.unsupported")}
              </p>
            )}
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
              className="mt-3 w-full"
              aria-label={t("home.modeAria")}
            >
              {PROXY_MODE_KEYS.map(([mode, labelKey]) => (
                <ToggleGroupItem key={mode} value={mode} className="flex-1">
                  {t(labelKey)}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
            {!tunUiHidden && (
              <Toggle
                variant="outline"
                size="sm"
                pressed={tunOverride ?? configuredTun}
                onPressedChange={(pressed) => {
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
                className="mt-3 w-full"
                aria-label={t("home.tunMode")}
              >
                {t("home.tunMode")}
              </Toggle>
            )}
          </CardContent>
        </Card>

        <Card size="sm" className="min-w-0 data-[size=sm]:[--card-spacing:--spacing(2)]">
          <CardHeader>
            <CardTitle>{t("home.infoTitle")}</CardTitle>
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
      </div>

      <Card size="sm" className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <CardHeader className="shrink-0">
          <CardTitle>{t("home.trafficTitle")}</CardTitle>
          <CardDescription>{t("home.trafficDesc")}</CardDescription>
        </CardHeader>
        <CardContent className="flex min-h-0 flex-1 flex-col">
          {nodes.length === 0 && !running ? (
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
              {nodes.length === 0 ? (
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
                className="min-h-0 flex-1"
                running={running}
                paused={modeBusy || busy || !active}
              />
            </>
          )}
        </CardContent>
      </Card>

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
