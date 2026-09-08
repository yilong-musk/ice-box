// SPDX-License-Identifier: GPL-3.0-or-later

import {
  useEffect,
  useRef,
  useState,
  type ComponentType,
  type CSSProperties,
  type ReactNode,
} from "react";
import {
  ArrowUpCircle,
  House,
  ListFilter,
  Rss,
  ScrollText,
  Settings as SettingsIcon,
  Waypoints,
} from "lucide-react";
import { api, formatDiagnostic, type CheckAppUpdateResponse, type StatusResponse } from "./api/tauri";
import { Home } from "./pages/Home";
import { Nodes } from "./pages/Nodes";
import { Subscriptions } from "./pages/Subscriptions";
import { Rules } from "./pages/Rules";
import { Logs } from "./pages/Logs";
import { Settings } from "./pages/Settings";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarTrigger,
  useSidebar,
} from "@/components/ui/sidebar";
import { ErrorAlert } from "@/components/StatusAlert";
import { WindowControls } from "@/components/WindowControls";
import { cn } from "@/lib/utils";
import { APP_VERSION } from "./lib/appVersion";
import {
  readLanguagePreference,
  t,
  useLanguagePreference,
  type MessageKey,
} from "./lib/i18n";
import { useThemePreference } from "./lib/theme";
import { RuntimeStoreProvider, useRuntimeStore } from "./lib/runtimeStore";
import logo from "./assets/logo.png";

type Tab = "home" | "nodes" | "subs" | "rules" | "logs" | "settings";

const SIDEBAR_WIDTH = "11rem";

const SIDEBAR_PROVIDER_STYLE = {
  "--sidebar-width": SIDEBAR_WIDTH,
} as CSSProperties;

function TabPane({
  active,
  children,
}: {
  active: boolean;
  children: ReactNode;
}) {
  return (
    <div
      className={
        active ? "flex min-h-0 min-w-0 flex-1 flex-col" : "hidden"
      }
      aria-hidden={!active}
      data-active={active ? "true" : "false"}
    >
      {children}
    </div>
  );
}

const NAV_ITEMS: {
  id: Tab;
  labelKey: MessageKey;
  icon: ComponentType<{ className?: string }>;
}[] = [
  { id: "home", labelKey: "app.nav.home", icon: House },
  { id: "nodes", labelKey: "app.nav.nodes", icon: Waypoints },
  { id: "rules", labelKey: "app.nav.rules", icon: ListFilter },
  { id: "subs", labelKey: "app.nav.subs", icon: Rss },
  { id: "logs", labelKey: "app.nav.logs", icon: ScrollText },
  { id: "settings", labelKey: "app.nav.settings", icon: SettingsIcon },
];

function TitleBar({ label }: { label: string }) {
  const { open, isMobile } = useSidebar();

  return (
    <div className="flex h-12 shrink-0 border-b" data-titlebar>
      <div
        className={cn(
          "shrink-0 border-r border-sidebar-border bg-sidebar transition-[width] duration-200 ease-linear",
          open && !isMobile ? "w-(--sidebar-width)" : "w-0",
        )}
        data-tauri-drag-region
        aria-hidden="true"
      />
      <header className="flex min-w-0 flex-1 items-center">
        <div className="flex h-full shrink-0 items-center px-2 md:hidden">
          <SidebarTrigger />
        </div>
        <div
          className="flex h-full min-w-0 flex-1 select-none items-center px-4"
          data-tauri-drag-region
        >
          <h2 className="text-sm font-medium">{label}</h2>
        </div>
        <WindowControls />
      </header>
    </div>
  );
}

function App() {
  return (
    <RuntimeStoreProvider>
      <AppShell />
    </RuntimeStoreProvider>
  );
}

function AppShell() {
  const [tab, setTab] = useState<Tab>("home");
  const [visited, setVisited] = useState<ReadonlySet<Tab>>(
    () => new Set<Tab>(["home"]),
  );
  const runtime = useRuntimeStore();
  const [globalStatus, setGlobalStatus] = useState<StatusResponse | null>(null);
  const status = runtime?.status ?? globalStatus;
  const [languageReady, setLanguageReady] = useState(false);
  const [availableUpdate, setAvailableUpdate] =
    useState<CheckAppUpdateResponse | null>(null);
  const [focusUpdateNonce, setFocusUpdateNonce] = useState(0);
  useThemePreference();
  const { preference, resolved, setPreference } = useLanguagePreference();
  const bootLanguageRef = useRef(preference);

  useEffect(() => {
    void api.restoreLaunchProxy().catch(() => {
      // Home / status poll surfaces proxy_recovery_warning.
    });
  }, []);

  useEffect(() => {
    let cancelled = false;
    void api
      .checkAppUpdate(true)
      .then((result) => {
        if (!cancelled && result.available && result.version) {
          setAvailableUpdate(result);
        }
      })
      .catch(() => {
        // Background checks stay silent.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    void api
      .getSettings()
      .then((settings) => {
        // localStorage paints the first frame quickly. Reconcile it with the
        // authoritative settings file unless the user changed language while
        // this startup read was in flight.
        if (
          !cancelled &&
          readLanguagePreference() === bootLanguageRef.current &&
          settings.language !== readLanguagePreference()
        ) {
          setPreference(settings.language);
        }
      })
      .catch(() => {
        // Settings surfaces load errors when the user opens that page.
      })
      .finally(() => {
        if (!cancelled) setLanguageReady(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!languageReady) return;
    void api.setTrayLanguage(resolved).catch(() => {
      // The web UI remains usable if the native tray is unavailable.
    });
  }, [languageReady, resolved]);

  function selectTab(id: Tab) {
    setTab(id);
    setVisited((prev) => {
      if (prev.has(id)) return prev;
      const next = new Set(prev);
      next.add(id);
      return next;
    });
  }

  const current = NAV_ITEMS.find((item) => item.id === tab);

  return (
    <TooltipProvider>
      <SidebarProvider
        defaultOpen
        className="flex h-svh w-full flex-col overflow-hidden bg-background text-foreground"
        style={SIDEBAR_PROVIDER_STYLE}
      >
        <TitleBar label={current ? t(current.labelKey) : ""} />

        <div className="flex min-h-0 min-w-0 flex-1">
          <Sidebar
            collapsible="offcanvas"
            side="left"
            className="top-12! h-[calc(100svh-3rem)]!"
          >
            <SidebarContent>
              <SidebarGroup>
                <SidebarMenu aria-label={t("app.nav.aria")}>
                  {NAV_ITEMS.map(({ id, labelKey, icon: Icon }) => (
                    <SidebarMenuItem key={id}>
                      <SidebarMenuButton
                        type="button"
                        isActive={tab === id}
                        aria-current={tab === id ? "page" : undefined}
                        className="text-sm"
                        onClick={() => selectTab(id)}
                      >
                        <Icon className="size-4" />
                        <span>{t(labelKey)}</span>
                      </SidebarMenuButton>
                    </SidebarMenuItem>
                  ))}
                </SidebarMenu>
              </SidebarGroup>
            </SidebarContent>
            <SidebarFooter className="p-0 items-center justify-center">
              <div className="flex w-full select-none flex-col items-center justify-center gap-1 px-4 py-2.5">
                <div
                  className="flex w-full items-center justify-center gap-2.5"
                  data-tauri-drag-region
                >
                  <img
                    src={logo}
                    alt=""
                    className="size-7 shrink-0 object-contain"
                    aria-hidden="true"
                  />
                  <h1 className="font-heading text-sm font-medium tracking-tight">
                    ice-box
                  </h1>
                </div>
                <div className="flex items-center justify-center gap-1">
                  <p
                    className="text-[11px] leading-none text-sidebar-foreground/50 tabular-nums"
                    aria-label={t("app.versionAria", { version: APP_VERSION })}
                    data-tauri-drag-region
                  >
                    {APP_VERSION}
                  </p>
                  {availableUpdate?.available && availableUpdate.version ? (
                    <button
                      type="button"
                      className="inline-flex size-4 shrink-0 items-center justify-center text-green-500 hover:text-green-400"
                      aria-label={t("app.updateAvailableAria", {
                        version: availableUpdate.version,
                      })}
                      onClick={() => {
                        selectTab("settings");
                        setFocusUpdateNonce((n) => n + 1);
                      }}
                    >
                      <ArrowUpCircle className="size-3.5" aria-hidden="true" />
                    </button>
                  ) : null}
                </div>
              </div>
            </SidebarFooter>
          </Sidebar>

          <div className="flex min-h-0 min-w-0 flex-1 flex-col">
            {status?.proxy_recovery_warning && (
              <div className="px-4 pt-3">
                <ErrorAlert>
                  {formatDiagnostic(status.proxy_recovery_warning)}
                </ErrorAlert>
              </div>
            )}

            <main
              className="content-main content-fill min-h-0 flex-1 overflow-hidden p-4"
              data-testid="app-main"
            >
              {visited.has("home") && (
                <TabPane active={tab === "home"}>
                  <Home
                    onNavigate={selectTab}
                    active={tab === "home"}
                    onStatus={setGlobalStatus}
                  />
                </TabPane>
              )}
              {visited.has("nodes") && (
                <TabPane active={tab === "nodes"}>
                  <Nodes onNavigate={selectTab} active={tab === "nodes"} />
                </TabPane>
              )}
              {visited.has("subs") && (
                <TabPane active={tab === "subs"}>
                  <Subscriptions />
                </TabPane>
              )}
              {visited.has("rules") && (
                <TabPane active={tab === "rules"}>
                  <Rules onNavigate={selectTab} active={tab === "rules"} />
                </TabPane>
              )}
              {visited.has("logs") && (
                <TabPane active={tab === "logs"}>
                  <Logs active={tab === "logs"} />
                </TabPane>
              )}
              {visited.has("settings") && (
                <TabPane active={tab === "settings"}>
                  <Settings
                    active={tab === "settings"}
                    availableUpdate={availableUpdate}
                    onAvailableUpdate={setAvailableUpdate}
                    focusUpdateNonce={focusUpdateNonce}
                  />
                </TabPane>
              )}
            </main>
          </div>
        </div>
      </SidebarProvider>
    </TooltipProvider>
  );
}

export default App;
