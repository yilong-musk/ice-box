// SPDX-License-Identifier: GPL-3.0-or-later

import {
  Activity,
  memo,
  useCallback,
  useDeferredValue,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { ArrowUpCircle, ChevronsLeft, ChevronsRight } from "lucide-react";
import {
  api,
  formatUiMessage,
  type CheckAppUpdateResponse,
  type StatusResponse,
} from "./api/client";
import { Home } from "./pages/Home";
import { Nodes } from "./pages/Nodes";
import { Apps } from "./pages/Apps";
import { Subscriptions } from "./pages/Subscriptions";
import { Rules } from "./pages/Rules";
import { Connections } from "./pages/Connections";
import { Mcp } from "./pages/Mcp";
import { Settings } from "./pages/Settings";
import { TooltipProvider } from "@/components/ui/tooltip";
import { NavIcon } from "@/components/NavIcon";
import { StatusLogo } from "@/components/StatusLogo";
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
import { isDesktopShell, isPhoneShell } from "@platform/shell";
import {
  detectWindowChrome,
  resizeWindowWidthBy,
  runWindowCommand,
} from "@platform/windowChrome";
import { useIsMobile } from "@/hooks/use-mobile";
import {
  readLanguagePreference,
  t,
  useLanguagePreference,
  type MessageKey,
} from "./lib/i18n";
import { useThemePreference } from "./lib/theme";
import { startBackgroundAppUpdateCheck } from "./lib/appUpdateCheck";
import { RuntimeStoreProvider, useRuntimeStore } from "./lib/runtimeStore";
import { PaneTabContext, PaneVisibleContext } from "./lib/paneVisibility";

type Tab =
  | "home"
  | "nodes"
  | "apps"
  | "subs"
  | "rules"
  | "settings"
  | "mcp"
  | "connections";

const SIDEBAR_WIDTH = "13rem";

/** Expanded (labels) or collapsed (icon rail) sidebar, kept across launches. */
const SIDEBAR_OPEN_STORAGE_KEY = "ice-box.sidebar-open";

/**
 * The visitor's last choice, else the default: the desktop starts collapsed
 * to the icon rail (its configured window size in tauri.conf.json is the
 * collapsed width), a wide phone-shell window starts expanded.
 */
function readSidebarOpen(): boolean {
  const fallback = isPhoneShell();
  try {
    const stored = window.localStorage.getItem(SIDEBAR_OPEN_STORAGE_KEY);
    return stored == null ? fallback : stored === "true";
  } catch {
    return fallback;
  }
}

function writeSidebarOpen(open: boolean) {
  try {
    window.localStorage.setItem(SIDEBAR_OPEN_STORAGE_KEY, String(open));
  } catch {
    // Storage unavailable: the choice lasts for this session only.
  }
}

// Every visited page stays mounted. Memoized pages with stable props skip the
// shell re-render on a tab switch. Each page subscribes to language changes on
// its own.
const HomePage = memo(Home);
const NodesPage = memo(Nodes);
const AppsPage = memo(Apps);
const SubscriptionsPage = memo(Subscriptions);
const RulesPage = memo(Rules);
const ConnectionsPage = memo(Connections);
const SettingsPage = memo(Settings);
const McpPage = memo(Mcp);

/**
 * Collapsed rail width, the same on every desktop. Its top strip holds the
 * window buttons: the native macOS traffic lights (inset 14px, ~52px wide) or
 * the Windows buttons drawn as the same lights (`WindowControls`), with the
 * same margin on the right. A 48px nav button centres in it with room to spare.
 */
const SIDEBAR_WIDTH_ICON = "5.5rem";

const SIDEBAR_PROVIDER_STYLE = {
  "--sidebar-width": SIDEBAR_WIDTH,
  "--sidebar-width-icon": SIDEBAR_WIDTH_ICON,
} as CSSProperties;

/** Expanded minus collapsed sidebar width, in CSS px (1rem = 16px). */
const SIDEBAR_WIDTH_DELTA_PX =
  (Number.parseFloat(SIDEBAR_WIDTH) - Number.parseFloat(SIDEBAR_WIDTH_ICON)) * 16;

/** The sidebar, its gap and the caption strip animate width over this long
 * (`duration-200` in sidebar.tsx and TitleBar). */
const SIDEBAR_TRANSITION_MS = 200;

/**
 * Window minimum size. tauri.conf.json configures the collapsed window
 * (740 x 620, minimum 600 x 500); expanded, the minimum grows by the sidebar
 * delta to 720 so the content pane keeps the same minimum width.
 */
/** The launch-time window size adjustment runs once per app start. */
let launchWindowSizeApplied = false;

function windowMinSize(open: boolean): { width: number; height: number } {
  return { width: open ? 720 : 720 - SIDEBAR_WIDTH_DELTA_PX, height: 500 };
}

/**
 * Below this window width the desktop sidebar turns into an off-canvas sheet.
 * It is the collapsed minimum window width, so a desktop window never gets
 * there by collapsing or by resizing within its minimum. Not the 768px
 * default: collapsing shrinks the window by the sidebar delta, which on macOS
 * (120px) takes an expanded 860px window to 740px and would hide the rail.
 */
const DESKTOP_SIDEBAR_BREAKPOINT_PX = windowMinSize(false).width;

function isDesktopNarrow(): boolean {
  return window.innerWidth < DESKTOP_SIDEBAR_BREAKPOINT_PX;
}

/**
 * Menu button classes. Collapsed to the icon rail, each button becomes a
 * 48px square centred in the rail, with a 24px icon and no label.
 */
const NAV_BUTTON_CLASS =
  "h-10 gap-3 rounded-lg px-3 text-[15px] [&_svg:not(.nav-icon)]:size-5 group-data-[collapsible=icon]:mx-auto group-data-[collapsible=icon]:size-12! group-data-[collapsible=icon]:p-0! group-data-[collapsible=icon]:justify-center group-data-[collapsible=icon]:rounded-xl group-data-[collapsible=icon]:[&_.nav-icon]:size-6 group-data-[collapsible=icon]:[&_svg:not(.nav-icon)]:size-5 group-data-[collapsible=icon]:[&>span]:hidden";

/** All panes share one grid cell, so a hidden pane that keeps its layout has
 * exactly the size it will have when shown again. */
const PANE_CLASS = "col-start-1 row-start-1 flex min-h-0 min-w-0 flex-col";

/**
 * A page that stays mounted while its tab is not shown. Hidden, <Activity>
 * sets `display: none` on the page (title-bar portals included), tears down
 * its effects, and renders its updates only when nothing else is pending;
 * state and DOM are kept for the next visit.
 */
function TabPane({
  tab,
  active,
  children,
}: {
  tab: Tab;
  active: boolean;
  children: ReactNode;
}) {
  return (
    <Activity mode={active ? "visible" : "hidden"}>
      <div className={PANE_CLASS} data-active={active ? "true" : "false"}>
        <PaneTabContext value={tab}>
          <PaneVisibleContext value={active}>{children}</PaneVisibleContext>
        </PaneTabContext>
      </div>
    </Activity>
  );
}

/**
 * A pane for the pages people switch to most (Home, Nodes). Hidden, it stays
 * mounted, rendered and laid out, just moved out of view, and its effects keep
 * running: the page stops its own polling through `active`. <Activity> set
 * `display: none` instead and re-ran every effect on the way back, so each
 * return rebuilt and laid out the whole page (the traffic chart re-registered
 * with Recharts and re-measured its ticks), which WebKit takes tens of
 * milliseconds for.
 */
function KeptPane({
  tab,
  active,
  children,
}: {
  tab: Tab;
  active: boolean;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  // Until `inert` lands below, a control focused before the switch would
  // keep focus (and take Space/Enter) inside the hidden pane. Drop it in the
  // switch frame; this touches only the focused element.
  useLayoutEffect(() => {
    if (active) return;
    const focused = document.activeElement;
    if (focused instanceof HTMLElement && ref.current?.contains(focused)) {
      focused.blur();
    }
  }, [active]);
  // `inert` keeps keyboard focus out of the hidden pane. It is inherited, so
  // WebKit restyles the whole pane when it changes; apply it after the switch
  // has been painted instead of in that frame.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let timer = 0;
    const frame = requestAnimationFrame(() => {
      timer = window.setTimeout(() => el.toggleAttribute("inert", !active), 0);
    });
    return () => {
      cancelAnimationFrame(frame);
      window.clearTimeout(timer);
    };
  }, [active]);
  return (
    <div
      ref={ref}
      className={cn(PANE_CLASS, !active && "pane-kept-hidden")}
      data-active={active ? "true" : "false"}
      aria-hidden={active ? undefined : true}
    >
      <PaneTabContext value={tab}>
        <PaneVisibleContext value={active}>{children}</PaneVisibleContext>
      </PaneTabContext>
    </div>
  );
}

const NAV_ITEMS: {
  id: Tab;
  labelKey: MessageKey;
}[] = [
  { id: "home", labelKey: "app.nav.home" },
  { id: "nodes", labelKey: "app.nav.nodes" },
  { id: "apps", labelKey: "app.nav.apps" },
  { id: "rules", labelKey: "app.nav.rules" },
  { id: "subs", labelKey: "app.nav.subs" },
  { id: "connections", labelKey: "app.nav.connections" },
  { id: "mcp", labelKey: "app.nav.mcp" },
  { id: "settings", labelKey: "app.nav.settings" },
];

/** One title-bar container per mounted tab: a page's actions never share a slot
 * with the page before it, and the shell shows only the current tab's
 * container. A hidden page whose deferred render has not dropped its portal yet
 * is already out of the bar, so switching between two pages that both put
 * buttons there cannot show both at once. `padding` insets the actions like the
 * page content below and applies only while the slot has buttons. */
function TitlebarActionSlots({
  items,
  tab,
  padding,
}: {
  items: readonly { id: Tab }[];
  tab: Tab;
  padding: string;
}) {
  return items.map(({ id }) => (
    <div
      key={id}
      data-titlebar-slot={id}
      hidden={id !== tab}
      className={cn("flex h-full items-center gap-2", id === tab && padding)}
    />
  ));
}

/** Height of the title bar strip the content slides up under (h-12). */
const TITLEBAR_PX = 48;
const INTERACTIVE_SELECTOR =
  'button, a, input, select, textarea, label, summary, [role="button"], [role="link"], [role="switch"], [role="checkbox"], [role="tab"], [role="menuitem"], [role="slider"], [contenteditable=""], [contenteditable="true"], [data-no-window-drag]';

/**
 * The content column sits on top of the title bar (-mt-12), so the header's
 * drag region underneath never gets the pointer on the right-hand side.
 * Treat a press in the top strip of the content as a title bar press:
 * drag the window, double-click to maximize, unless it lands on a control.
 */
function isTitlebarPress(e: React.MouseEvent<HTMLElement>): boolean {
  if (e.button !== 0) return false;
  const top = e.currentTarget.getBoundingClientRect().top;
  if (e.clientY - top >= TITLEBAR_PX) return false;
  const target = e.target as Element | null;
  return !target?.closest?.(INTERACTIVE_SELECTOR);
}

function onContentTitlebarMouseDown(e: React.MouseEvent<HTMLElement>) {
  if (!isTitlebarPress(e) || e.detail > 1) return;
  void runWindowCommand("startDragging");
}

function onContentTitlebarDoubleClick(e: React.MouseEvent<HTMLElement>) {
  if (!isTitlebarPress(e)) return;
  void runWindowCommand("toggleMaximize");
}

function TitleBar({ label }: { label: string }) {
  const { open, isMobile } = useSidebar();
  const sidebarOpen = open && !isMobile;
  const windows = detectWindowChrome() === "windows-custom";

  return (
    <div className="flex h-12 shrink-0" data-titlebar>
      {/* Window buttons sit in this sidebar-width strip so the divider stays
          aligned. In the narrow (sheet) layout the strip shrinks to the
          Windows lights (4.5rem), a concrete width so the transition runs. */}
      <div
        className={cn(
          "relative z-20 flex h-full shrink-0 overflow-hidden transition-[width] duration-200 ease-linear",
          sidebarOpen
            ? "w-(--sidebar-width)"
            : isMobile
              ? windows
                ? "w-[4.5rem]"
                : "w-0"
              : "w-(--sidebar-width-icon)",
        )}
      >
        <WindowControls />
        <div
          className={cn(
            "h-full",
            (sidebarOpen || !isMobile) && "min-w-0 flex-1",
          )}
          data-tauri-drag-region
          aria-hidden="true"
        />
      </div>
      <header className="flex min-w-0 flex-1 items-center">
        {isMobile ? (
          <div className="flex h-full shrink-0 items-center px-2">
            <SidebarTrigger />
          </div>
        ) : null}
        {/* No page title: the selected sidebar item already names the page.
            The strip stays as the window drag region. */}
        <div
          className="flex h-full min-w-0 flex-1 select-none items-center px-4"
          data-tauri-drag-region
          aria-label={label}
        />
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

/** Collapse the sidebar to its icon rail, or expand it back to labels. */
function SidebarCollapseToggle({ className }: { className?: string }) {
  const { open, toggleSidebar } = useSidebar();
  const label = t(open ? "app.nav.collapse" : "app.nav.expand");
  // Not a SidebarMenu: the toggle is window chrome, not a navigation entry,
  // so the main nav stays the first (and only) menu in the sidebar. No
  // wrapper either: on the brand row the button is positioned on its own,
  // and a wrapper left in the flow would add a flex gap and push the logo
  // and name off centre.
  return (
    <SidebarMenuButton
      type="button"
      tooltip={label}
      aria-label={label}
      aria-expanded={open}
      data-testid="sidebar-collapse-toggle"
      // Icon only, in the sidebar footer above the brand.
      className={cn(
        NAV_BUTTON_CLASS,
        // Quiet chrome: faint, small icon, no fill; it only firms up on
        // hover, like the sidebar toggle in macOS apps.
        "size-10 justify-center px-0! text-sidebar-foreground/40 hover:bg-sidebar-hover! hover:text-sidebar-foreground/80 [&_svg]:size-4! [&_svg]:stroke-[1.75]",
        className,
      )}
      onClick={toggleSidebar}
    >
      {open ? (
        <ChevronsLeft aria-hidden="true" />
      ) : (
        <ChevronsRight aria-hidden="true" />
      )}
    </SidebarMenuButton>
  );
}

function AppShell() {
  const [sidebarOpen, setSidebarOpen] = useState(readSidebarOpen);
  const sidebarOpenRef = useRef(sidebarOpen);
  const contentRef = useRef<HTMLDivElement>(null);
  // Content pane width held fixed while the sidebar animates (see below).
  const [pinnedContentWidth, setPinnedContentWidth] = useState<number | null>(null);
  const releasePinRef = useRef<() => void>(() => {});
  useEffect(() => () => releasePinRef.current(), []);
  const onSidebarOpenChange = useCallback((next: boolean) => {
    // Collapsing or expanding changes the window width by the sidebar's
    // width difference, so the content pane on the right keeps its width.
    // (Not inside the state updater: StrictMode runs updaters twice.)
    if (sidebarOpenRef.current !== next && !isDesktopNarrow()) {
      const delta = next ? SIDEBAR_WIDTH_DELTA_PX : -SIDEBAR_WIDTH_DELTA_PX;
      const min = windowMinSize(next);
      // The window changes width in one step while the sidebar animates its
      // width over SIDEBAR_TRANSITION_MS, so a flexible content pane would
      // stretch and shrink back (wider first on expand, narrower first on
      // collapse). Hold the pane at its width until the sidebar has finished
      // and the window has reached its new width: the pane slides instead.
      releasePinRef.current();
      let released = false;
      let frame = 0;
      let fallback = 0;
      const release = () => {
        if (released) return;
        released = true;
        cancelAnimationFrame(frame);
        window.clearTimeout(fallback);
        setPinnedContentWidth(null);
      };
      releasePinRef.current = release;
      const width = contentRef.current?.getBoundingClientRect().width ?? 0;
      if (width > 0) setPinnedContentWidth(width);
      const started = performance.now();
      const target = Math.max(min.width, window.innerWidth + delta);
      void resizeWindowWidthBy(delta, min).then((resized) => {
        // Maximized, fullscreen, or no window to resize: the pane absorbs
        // the sidebar change after all, so let it follow the animation.
        if (!resized) {
          release();
          return;
        }
        const settle = () => {
          if (released) return;
          const animated = performance.now() - started >= SIDEBAR_TRANSITION_MS;
          if (animated && Math.abs(window.innerWidth - target) <= 1) release();
          else frame = requestAnimationFrame(settle);
        };
        frame = requestAnimationFrame(settle);
        // A window the system would not resize must not stay pinned.
        fallback = window.setTimeout(release, 1000);
      });
    }
    sidebarOpenRef.current = next;
    setSidebarOpen(next);
    writeSidebarOpen(next);
  }, []);
  // At launch the window has its configured size, which is the collapsed
  // width (the default state). If the saved state is expanded, grow it by the
  // sidebar delta so the content pane has the same width either way. Guarded
  // at module level: StrictMode runs effects twice in dev.
  useEffect(() => {
    if (launchWindowSizeApplied) return;
    launchWindowSizeApplied = true;
    const open = readSidebarOpen();
    const grow = open && !isDesktopNarrow();
    void resizeWindowWidthBy(grow ? SIDEBAR_WIDTH_DELTA_PX : 0, windowMinSize(open));
  }, []);
  const [tab, setTab] = useState<Tab>("home");
  const [visited, setVisited] = useState<ReadonlySet<Tab>>(
    () => new Set<Tab>(["home"]),
  );
  // Kept panes switch on `tab` at once; their pages see the change one render
  // later, so the click only flips two panes and the page work (stopping or
  // starting polls) runs after that frame.
  const pageTab = useDeferredValue(tab);
  const runtime = useRuntimeStore();
  const [globalStatus, setGlobalStatus] = useState<StatusResponse | null>(null);
  const status = runtime?.status ?? globalStatus;
  const [languageReady, setLanguageReady] = useState(false);
  const [availableUpdate, setAvailableUpdate] =
    useState<CheckAppUpdateResponse | null>(null);
  const [focusUpdateNonce, setFocusUpdateNonce] = useState(0);
  useThemePreference();
  const narrow = useIsMobile();
  const { preference, resolved, setPreference } = useLanguagePreference();
  const bootLanguageRef = useRef(preference);

  useEffect(() => {
    // Desktop restores the last proxy session after the first frame. The
    // phone does not record that state and does not reconnect on open.
    if (isPhoneShell()) return;
    void api.restoreLaunchProxy().catch(() => {
      // Home / status poll surfaces proxy_recovery_warning.
    });
  }, []);

  const updateCheckerRef = useRef<ReturnType<
    typeof startBackgroundAppUpdateCheck
  > | null>(null);

  useEffect(() => {
    const checker = startBackgroundAppUpdateCheck({
      check: ({ startup }) => api.checkAppUpdate(true, startup),
      recordCooldown: () => api.recordAppUpdateCheck(),
      onResult: (result) => {
        if (result.available && result.version) {
          setAvailableUpdate(result);
        }
      },
    });
    updateCheckerRef.current = checker;
    return () => {
      checker.stop();
      updateCheckerRef.current = null;
    };
  }, []);

  useEffect(() => {
    if (status?.core.status === "running") {
      updateCheckerRef.current?.notifyCoreRunning();
    }
  }, [status?.core.status]);

  // Tray prompt mirrors the sidebar arrow: the same version while automatic
  // checks are on, and nothing once the switch turns them off (Settings clears
  // `availableUpdate` through `publishSidebarUpdate`).
  useEffect(() => {
    if (isPhoneShell()) return;
    if (typeof api.setTrayUpdateAvailable !== "function") return;
    const version =
      availableUpdate?.available && availableUpdate.version
        ? availableUpdate.version
        : null;
    void api.setTrayUpdateAvailable(version).catch(() => {
      // The tray is best-effort: the sidebar arrow still shows the update.
    });
  }, [availableUpdate]);

  // Tray prompt → Settings → App Updates, the same landing spot as the arrow.
  useEffect(() => {
    if (isPhoneShell()) return;
    if (typeof api.listenTrayUpdateClick !== "function") return;
    let cancelled = false;
    let unlisten = () => {};
    void api
      .listenTrayUpdateClick(() => {
        selectTab("settings");
        setFocusUpdateNonce((n) => n + 1);
      })
      .then((off) => {
        if (cancelled) off();
        else unlisten = off;
      })
      .catch(() => {
        // Without the listener the tray item still opens the window.
      });
    return () => {
      cancelled = true;
      unlisten();
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

  const selectTab = useCallback((id: Tab) => {
    setTab(id);
    setVisited((prev) => {
      if (prev.has(id)) return prev;
      const next = new Set(prev);
      next.add(id);
      return next;
    });
  }, []);

  const phone = isPhoneShell();
  const desktop = isDesktopShell();
  const navItems = NAV_ITEMS.filter((item) => {
    if (item.id === "mcp") return desktop;
    if (item.id === "connections") return !phone;
    // Phones move the node list into Home and reuse the tab slot for Apps.
    if (item.id === "apps") return phone;
    if (item.id === "nodes") return !phone;
    return true;
  });
  const slotItems = navItems.filter((item) => visited.has(item.id));
  const current = navItems.find((item) => item.id === tab);

  const pages = (
    <div className="grid min-h-0 min-w-0 flex-1 grid-cols-[minmax(0,1fr)] grid-rows-[minmax(0,1fr)]">
      {visited.has("home") && (
        <KeptPane tab="home" active={tab === "home"}>
          <HomePage
            onNavigate={selectTab}
            active={pageTab === "home"}
            onStatus={setGlobalStatus}
          />
        </KeptPane>
      )}
      {!phone && visited.has("nodes") && (
        <KeptPane tab="nodes" active={tab === "nodes"}>
          <NodesPage onNavigate={selectTab} active={pageTab === "nodes"} />
        </KeptPane>
      )}
      {phone && visited.has("apps") && (
        <TabPane tab="apps" active={tab === "apps"}>
          <AppsPage active={tab === "apps"} />
        </TabPane>
      )}
      {visited.has("subs") && (
        <TabPane tab="subs" active={tab === "subs"}>
          <SubscriptionsPage active={tab === "subs"} />
        </TabPane>
      )}
      {visited.has("rules") && (
        <TabPane tab="rules" active={tab === "rules"}>
          <RulesPage onNavigate={selectTab} active={tab === "rules"} />
        </TabPane>
      )}
      {!phone && visited.has("connections") && (
        <TabPane tab="connections" active={tab === "connections"}>
          <ConnectionsPage active={tab === "connections"} />
        </TabPane>
      )}
      {visited.has("settings") && (
        <TabPane tab="settings" active={tab === "settings"}>
          <SettingsPage
            active={tab === "settings"}
            availableUpdate={availableUpdate}
            onAvailableUpdate={setAvailableUpdate}
            focusUpdateNonce={focusUpdateNonce}
          />
        </TabPane>
      )}
      {desktop && visited.has("mcp") && (
        <TabPane tab="mcp" active={tab === "mcp"}>
          <McpPage active={tab === "mcp"} />
        </TabPane>
      )}
    </div>
  );

  const recovery = status?.proxy_recovery_warning?.length ? (
    <div className={cn("px-4 pt-3", phone && narrow && "phone-recovery")}>
      <ErrorAlert>
        {status.proxy_recovery_warning
          .map(formatUiMessage)
          .filter(Boolean)
          .join("；")}
      </ErrorAlert>
    </div>
  ) : null;

  if (phone && narrow) {
    const activeIndex = Math.max(
      0,
      navItems.findIndex((item) => item.id === tab),
    );
    return (
      <TooltipProvider>
        <div
          className="phone-shell relative flex h-svh w-full flex-col overflow-hidden bg-background text-foreground"
          data-testid="phone-shell"
        >
          <header
            data-testid="phone-header"
            className="phone-header absolute inset-x-0 top-0 z-30 flex items-center gap-2.5 px-4 pt-[max(0.75rem,var(--phone-status-inset))] pb-3"
          >
            <StatusLogo status={status} size={32} />
            <div className="min-w-0 flex-1">
              <h1 className="font-heading text-sm font-medium tracking-tight">
                ice-box
              </h1>
              <p className="mt-0.5 text-[11px] leading-none text-foreground/50 tabular-nums">
                {APP_VERSION}
              </p>
            </div>
            <div data-titlebar-actions className="flex shrink-0 items-center">
              <TitlebarActionSlots
                items={slotItems}
                tab={tab}
                padding=""
              />
            </div>
          </header>
          {recovery}
          <main
            className="content-main content-fill min-h-0 flex-1 overflow-hidden px-4 pt-3 pb-0"
            data-testid="app-main"
          >
            {pages}
          </main>
          <div className="phone-tab-dock">
            <nav
              data-testid="phone-tab-bar"
              aria-label={t("app.nav.aria")}
              className="phone-tab-bar relative grid w-full max-w-[24rem] grid-cols-5 rounded-full p-1"
              style={
                {
                  "--tab-count": navItems.length,
                  "--tab-index": activeIndex,
                } as CSSProperties
              }
            >
              <span
                className="phone-tab-indicator"
                data-testid="phone-tab-indicator"
                aria-hidden="true"
              />
              {navItems.map(({ id, labelKey }) => (
                <button
                  key={id}
                  type="button"
                  aria-label={t(labelKey)}
                  aria-current={tab === id ? "page" : undefined}
                  className={cn(
                    // Top-aligned integer metrics: 8px padding, 20px glyph, 2px gap, 14px line (52px total).
                    "phone-tab-item relative z-10 flex min-h-13 min-w-0 flex-col items-center gap-0.5 rounded-full px-1 py-2 text-[11px] leading-[14px] outline-none focus-visible:ring-2 focus-visible:ring-ring",
                    tab === id
                      ? "font-medium text-primary"
                      : "text-foreground/65 hover:text-foreground/90",
                  )}
                  onClick={() => selectTab(id)}
                >
                  <NavIcon name={id} />
                  <span className="w-full truncate text-center">
                    {t(id === "subs" ? "app.nav.subsTab" : labelKey)}
                  </span>
                </button>
              ))}
            </nav>
          </div>
        </div>
      </TooltipProvider>
    );
  }

  return (
    <TooltipProvider>
      <SidebarProvider
        open={sidebarOpen}
        onOpenChange={onSidebarOpenChange}
        mobileBreakpoint={phone ? undefined : DESKTOP_SIDEBAR_BREAKPOINT_PX}
        className={cn(
          "flex h-svh w-full flex-col overflow-hidden bg-background text-foreground",
          phone && "pt-[max(env(safe-area-inset-top),var(--ice-status-inset,0px))]",
        )}
        style={SIDEBAR_PROVIDER_STYLE}
        data-testid={phone ? "tablet-shell" : undefined}
      >
        {phone ? null : <TitleBar label={current ? t(current.labelKey) : ""} />}

        <div className="flex min-h-0 min-w-0 flex-1">
          <Sidebar
            collapsible="icon"
            side="left"
            variant="sidebar"
            className={phone ? undefined : "border-sidebar-border"}
          >
            {/* Starts right below the title bar strip. Nothing may tuck up
                under it: the strip (z-20, caption buttons and drag region)
                sits above the sidebar and would swallow the clicks. */}
            <SidebarContent className={phone ? undefined : "pt-12"}>
              <SidebarGroup>
                <SidebarMenu aria-label={t("app.nav.aria")}>
                  {navItems.map(({ id, labelKey }) => (
                    <SidebarMenuItem key={id}>
                      <SidebarMenuButton
                        type="button"
                        isActive={tab === id}
                        aria-current={tab === id ? "page" : undefined}
                        tooltip={t(labelKey)}
                        className={NAV_BUTTON_CLASS}
                        onClick={() => selectTab(id)}
                      >
                        <NavIcon name={id} />
                        <span>{t(labelKey)}</span>
                      </SidebarMenuButton>
                    </SidebarMenuItem>
                  ))}
                </SidebarMenu>
              </SidebarGroup>
            </SidebarContent>
            <SidebarFooter className="p-0 items-center justify-center">
              {/* Collapsed to the icon rail, only the status logo (full
                  size, the rail is wide enough) and its update badge stay. */}
              <div className="flex w-full select-none flex-col items-center justify-center gap-1 px-4 py-2.5 group-data-[collapsible=icon]:px-0">
                <div
                  className="relative flex w-full items-center justify-center gap-2.5 group-data-[collapsible=icon]:flex-col group-data-[collapsible=icon]:gap-1"
                  data-tauri-drag-region
                  data-testid="app-brand-row"
                >
                  {/* Collapse toggle on the brand row: at its right edge when
                      expanded (the logo and name stay centred), stacked above
                      the logo on the narrow collapsed rail. */}
                  {phone ? null : (
                    // Placed with top-1/2 and a negative margin (no
                    // translate the button's own transitions could override).
                    // -mt-[14px] instead of the exact -mt-4 (half of 32px):
                    // the row's optical middle, the "ice-box" text, sits a
                    // little below the logo's box centre.
                    <SidebarCollapseToggle className="absolute top-1/2 -right-3 -mt-3 size-7! rounded-md group-data-[collapsible=icon]:static group-data-[collapsible=icon]:mt-0 group-data-[collapsible=icon]:size-7! group-data-[collapsible=icon]:rounded-md group-data-[collapsible=icon]:[&_svg:not(.nav-icon)]:size-4!" />
                  )}
                  {/* The update arrow is a badge on the logo's bottom-right
                      corner, positioned out of flow, so the logo stays put
                      when an update turns up (expanded or collapsed). */}
                  <div className="relative shrink-0">
                    <StatusLogo status={status} />
                    {availableUpdate?.available && availableUpdate.version ? (
                      <button
                        type="button"
                        // The switch art fills only the middle of its 48px
                        // box; these offsets land the badge on the art's
                        // bottom-right corner rather than below it.
                        className="absolute -right-1.5 bottom-0.5 z-10 flex size-[18px] items-center justify-center rounded-full bg-sidebar text-ok outline-none hover:text-ok/80 focus-visible:ring-2 focus-visible:ring-ring"
                        aria-label={t("app.updateAvailableAria", {
                          version: availableUpdate.version,
                        })}
                        data-testid="app-update-badge"
                        onClick={() => {
                          selectTab("settings");
                          setFocusUpdateNonce((n) => n + 1);
                        }}
                      >
                        <ArrowUpCircle className="size-4" aria-hidden="true" />
                      </button>
                    ) : null}
                  </div>
                  <h1 className="font-heading text-sm font-medium tracking-tight group-data-[collapsible=icon]:hidden">
                    ice-box
                  </h1>
                </div>
                <p
                  className="text-[11px] leading-none text-sidebar-foreground/50 tabular-nums group-data-[collapsible=icon]:hidden"
                  aria-label={t("app.versionAria", { version: APP_VERSION })}
                  data-tauri-drag-region
                >
                  {APP_VERSION}
                </p>
              </div>
            </SidebarFooter>
          </Sidebar>

          <div
            className={cn(
              "flex min-h-0 min-w-0 flex-1 flex-col",
              // Pages slide up under the empty title bar and use that height;
              // page actions sit in a toolbar row inside the content instead.
              // The caption strip (z-20) stays on top; the sidebar strip
              // remains the drag region.
              !phone && "relative z-10 -mt-12",
            )}
            onMouseDown={phone ? undefined : onContentTitlebarMouseDown}
            onDoubleClick={phone ? undefined : onContentTitlebarDoubleClick}
            data-testid="app-content"
            ref={contentRef}
            data-pinned={pinnedContentWidth == null ? undefined : "true"}
            style={
              pinnedContentWidth == null
                ? undefined
                : { flex: "none", width: pinnedContentWidth }
            }
          >
            {phone ? (
              <header className="flex shrink-0 items-center px-4 pt-3">
                <h2 className="text-sm font-medium">
                  {current ? t(current.labelKey) : ""}
                </h2>
                <div
                  data-titlebar-actions
                  className="ml-auto flex shrink-0 items-center"
                >
                  <TitlebarActionSlots
                    items={slotItems}
                    tab={tab}
                    padding="not-empty:pr-3"
                  />
                </div>
              </header>
            ) : null}
            {recovery}

            <main
              className={cn(
                "content-main content-fill relative min-h-0 flex-1 overflow-hidden p-4",
                phone && "pb-[max(1rem,env(safe-area-inset-bottom))]",
              )}
              data-testid="app-main"
            >
              {phone ? null : (
                // Page actions (batch test, import, add rule) float at the
                // bottom right of the content. Hidden while the current page
                // puts nothing in its slot.
                <div
                  data-titlebar-actions
                  data-page-toolbar
                  className="absolute right-6 bottom-6 z-30 flex items-center gap-2"
                >
                  <TitlebarActionSlots items={slotItems} tab={tab} padding="" />
                </div>
              )}
              {pages}
            </main>
          </div>
        </div>
      </SidebarProvider>
    </TooltipProvider>
  );
}

export default App;
