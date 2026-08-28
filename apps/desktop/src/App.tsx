import {
  useEffect,
  useState,
  type ComponentType,
  type CSSProperties,
  type ReactNode,
} from "react";
import {
  House,
  ListFilter,
  Rss,
  ScrollText,
  Settings as SettingsIcon,
  Waypoints,
} from "lucide-react";
import { api, type StatusResponse } from "./api/tauri";
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
import { useThemePreference } from "./lib/theme";
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
    >
      {children}
    </div>
  );
}

const NAV_ITEMS: { id: Tab; label: string; icon: ComponentType<{ className?: string }> }[] = [
  { id: "home", label: "主页", icon: House },
  { id: "nodes", label: "节点", icon: Waypoints },
  { id: "rules", label: "规则", icon: ListFilter },
  { id: "subs", label: "订阅", icon: Rss },
  { id: "logs", label: "日志", icon: ScrollText },
  { id: "settings", label: "设置", icon: SettingsIcon },
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
  const [tab, setTab] = useState<Tab>("home");
  const [visited, setVisited] = useState<ReadonlySet<Tab>>(
    () => new Set<Tab>(["home"]),
  );
  const [globalStatus, setGlobalStatus] = useState<StatusResponse | null>(null);
  useThemePreference();

  function selectTab(id: Tab) {
    setTab(id);
    setVisited((prev) => {
      if (prev.has(id)) return prev;
      const next = new Set(prev);
      next.add(id);
      return next;
    });
  }

  useEffect(() => {
    let cancelled = false;
    const poll = async () => {
      try {
        const status = await api.getStatus();
        if (!cancelled) setGlobalStatus(status);
      } catch {
        // Background poll; tab pages surface their own errors.
      }
    };
    void poll();
    const id = window.setInterval(() => void poll(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, []);

  const current = NAV_ITEMS.find((item) => item.id === tab);

  return (
    <TooltipProvider>
      <SidebarProvider
        defaultOpen
        className="flex h-svh w-full flex-col overflow-hidden bg-background text-foreground"
        style={SIDEBAR_PROVIDER_STYLE}
      >
        <TitleBar label={current?.label ?? ""} />

        <div className="flex min-h-0 min-w-0 flex-1">
          <Sidebar
            collapsible="offcanvas"
            side="left"
            className="top-12! h-[calc(100svh-3rem)]!"
          >
            <SidebarContent>
              <SidebarGroup>
                <SidebarMenu aria-label="主导航">
                  {NAV_ITEMS.map(({ id, label, icon: Icon }) => (
                    <SidebarMenuItem key={id}>
                      <SidebarMenuButton
                        type="button"
                        isActive={tab === id}
                        aria-current={tab === id ? "page" : undefined}
                        className="text-sm"
                        onClick={() => selectTab(id)}
                      >
                        <Icon className="size-4" />
                        <span>{label}</span>
                      </SidebarMenuButton>
                    </SidebarMenuItem>
                  ))}
                </SidebarMenu>
              </SidebarGroup>
            </SidebarContent>
            <SidebarFooter className="p-0 items-center justify-center">
              <div
                className="flex w-full select-none flex-col items-center justify-center gap-1 px-4 py-2.5"
                data-tauri-drag-region
              >
                <div className="flex items-center justify-center gap-2.5">
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
                <p
                  className="text-[11px] leading-none text-sidebar-foreground/50 tabular-nums"
                  aria-label={`版本 ${APP_VERSION}`}
                >
                  {APP_VERSION}
                </p>
              </div>
            </SidebarFooter>
          </Sidebar>

          <div className="flex min-h-0 min-w-0 flex-1 flex-col">
            {globalStatus?.proxy_recovery_warning && (
              <div className="px-4 pt-3">
                <ErrorAlert>
                  {globalStatus.proxy_recovery_warning}
                </ErrorAlert>
              </div>
            )}

            <main className="content-main content-fill min-h-0 flex-1 overflow-hidden p-4">
              {visited.has("home") && (
                <TabPane active={tab === "home"}>
                  <Home onNavigate={selectTab} active={tab === "home"} />
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
                  <Settings />
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
