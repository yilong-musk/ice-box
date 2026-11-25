// SPDX-License-Identifier: GPL-3.0-or-later

import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App, { resetLaunchWindowSizeForTest } from "./App";

/** Lets a test stand in for window chrome commands; real (no-op) otherwise. */
const windowResize = vi.hoisted(() => ({
  fn: null as null | ((delta: number, min: { width: number; height: number }) => Promise<boolean>),
}));
const windowCommand = vi.hoisted(() => ({
  fn: null as null | ((command: string) => Promise<void>),
}));

vi.mock("@platform/windowChrome", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@platform/windowChrome")>();
  return {
    ...actual,
    resizeWindowWidthBy: (delta: number, min: { width: number; height: number }) =>
      windowResize.fn ? windowResize.fn(delta, min) : actual.resizeWindowWidthBy(delta, min),
    runWindowCommand: (command: Parameters<typeof actual.runWindowCommand>[0]) =>
      windowCommand.fn ? windowCommand.fn(command) : actual.runWindowCommand(command),
  };
});
import { APP_VERSION } from "./lib/appVersion";
import { LANGUAGE_STORAGE_KEY, t, isMessageKey } from "./lib/i18n";
import { clearNodesSnapshot } from "./lib/nodes";

const phoneShell = vi.hoisted(() => ({ value: false }));
const desktopShell = vi.hoisted(() => ({ value: true }));

vi.mock("@platform/shell", () => ({
  isPhoneShell: () => phoneShell.value,
  isDesktopShell: () => desktopShell.value,
}));

const getStatus = vi.fn();
const getSettings = vi.fn();
const saveSettings = vi.fn();
const listNodes = vi.fn();
const listApps = vi.fn();
const setTrayLanguage = vi.fn();
const setTrayUpdateAvailable = vi.fn();
const listenTrayUpdateClick = vi.fn();
const restoreLaunchProxy = vi.fn();
const checkAppUpdate = vi.fn();
const installAppUpdate = vi.fn();
const getMcpStatus = vi.fn();
const setMcpEnabled = vi.fn();
const restartMcp = vi.fn();

const defaultSettings = {
  mixed_listen: "127.0.0.1",
  mixed_port: 17890,
  clash_api_listen: "127.0.0.1",
  clash_api_port: 19090,
  selected_tag: null,
  auto_set_system_proxy: false,
  proxy_service_enabled: false,
  allow_lan: false,
  ipv6: true,
  prefer_ipv6: false,
  proxy_mode: "rule",
  auto_default_rules: true,
  tun: {
    enabled: false,
    interface_name: null,
    ipv4_address: "10.0.0.1/30",
    ipv6_address: "fdfe:dcba:9876::1/126",
    mtu: 9000,
    auto_route: true,
    strict_route: true,
    stack: "gvisor",
    dns_hijack: false,
  },
  language: "system",
  check_app_updates: true,
  probe_exit_ip: true,
  tray_display_mode: "icon_and_speed",
} as const;

const tunStatus = {
  traffic_capture: "inactive",
  configured_tun: false,
  tun_status: "disabled",
  tun_interface: null,
  tun_error: null,
  capture_transition_id: null,
  tun_available: true,
  tun_unavailable_reason: null,
  tun_ui_hidden: false,
  tun_ipv6_disabled: false,
  helper_installed: false,
  helper_supported: true,
  launch_at_login_supported: true,
  helper_stale: false,
} as const;

const mcpStatus = {
  supported: true,
  enabled: true,
  running: true,
  endpoint: "127.0.0.1:4567",
  client_count: 1,
  tool_count: 34,
  bridge_executable: "/Applications/ice-box.app/Contents/MacOS/ice-box",
  bridge_args: ["--mcp-stdio", "--data-dir", "/tmp/ice-box"],
  client_config:
    '{"mcpServers":{"ice-box":{"command":"/Applications/ice-box.app/Contents/MacOS/ice-box","args":["--mcp-stdio","--data-dir","/tmp/ice-box"]}}}',
  error: null,
} as const;

vi.mock("./api/client", () => ({
  formatDiagnostic: (warning: string) => warning,
  formatUiMessage: (msg: unknown) => {
    if (!msg) return "";
    if (typeof msg === "string") return msg;
    const m = msg as { key?: string; params?: Record<string, string> };
    if (!m.key) return String(msg);
    if (m.key === "ui.raw") return m.params?.text ?? "";
    const params = m.params ?? {};
    const label = isMessageKey(m.key) ? t(m.key, params) : m.key;
    const detail = params.detail;
    if (detail && !label.includes(detail)) {
      return `${label}: ${detail}`;
    }
    return label;
  },
  api: {
    getStatus: (...args: unknown[]) => getStatus(...args),
    listNodes: (...args: unknown[]) => listNodes(...args),
    listApps: (...args: unknown[]) => listApps(...args),
    setAppProxy: vi.fn().mockResolvedValue(undefined),
    getSettings: (...args: unknown[]) => getSettings(...args),
    restoreLaunchProxy: (...args: unknown[]) => restoreLaunchProxy(...args),
    checkAppUpdate: (...args: unknown[]) => checkAppUpdate(...args),
    installAppUpdate: (...args: unknown[]) => installAppUpdate(...args),
    recordAppUpdateCheck: vi.fn().mockResolvedValue(undefined),
    copyProxyCommand: vi.fn().mockResolvedValue(undefined),
    listenAppUpdateProgress: vi.fn().mockResolvedValue(() => {}),
    listenStateChanged: vi.fn().mockResolvedValue(() => {}),
    saveSettings: (...args: unknown[]) => saveSettings(...args),
    setTrayLanguage: (...args: unknown[]) => setTrayLanguage(...args),
    setTrayUpdateAvailable: (...args: unknown[]) =>
      setTrayUpdateAvailable(...args),
    listenTrayUpdateClick: (...args: unknown[]) =>
      listenTrayUpdateClick(...args),
    getTrafficSnapshot: vi
      .fn()
      .mockResolvedValue({ points: [], latest: null, peak: null }),
    getTrafficSince: vi.fn().mockResolvedValue({
      generation: 0,
      cursor: null,
      points: [],
      latest: null,
      peak: null,
    }),
    start: vi.fn(),
    stop: vi.fn(),
    listSubscriptions: vi.fn().mockResolvedValue([]),
    getConnectionStats: vi.fn().mockResolvedValue({
      running: true,
      count: 0,
      upload_total: 0,
      download_total: 0,
    }),
    listConnections: vi.fn().mockResolvedValue({
      running: true,
      upload_total: 0,
      download_total: 0,
      connections: [],
    }),
    getMcpStatus: (...args: unknown[]) => getMcpStatus(...args),
    setMcpEnabled: (...args: unknown[]) => setMcpEnabled(...args),
    restartMcp: (...args: unknown[]) => restartMcp(...args),
  },
  formatInvokeError: (err: unknown) => String(err),
  formatInvokeErrorTitle: (err: unknown) => String(err),
}));

describe("App", () => {
  beforeEach(() => {
    phoneShell.value = false;
    desktopShell.value = true;
    windowResize.fn = null;
    windowCommand.fn = null;
    resetLaunchWindowSizeForTest();
    vi.clearAllMocks();
    clearNodesSnapshot();
    getSettings.mockResolvedValue(defaultSettings);
    restoreLaunchProxy.mockResolvedValue(undefined);
    setTrayLanguage.mockResolvedValue(undefined);
    setTrayUpdateAvailable.mockResolvedValue(undefined);
    listenTrayUpdateClick.mockResolvedValue(() => {});
    listNodes.mockResolvedValue([]);
    listApps.mockResolvedValue([]);
    checkAppUpdate.mockResolvedValue({
      available: false,
      version: null,
      notes: null,
      skipped: false,
      should_prompt: false,
    });
    installAppUpdate.mockResolvedValue(undefined);
    saveSettings.mockResolvedValue(undefined);
    getMcpStatus.mockResolvedValue(mcpStatus);
    setMcpEnabled.mockResolvedValue(mcpStatus);
    restartMcp.mockResolvedValue(mcpStatus);
    window.innerWidth = 1280;
    getStatus.mockResolvedValue({
      core: {
        status: "stopped",
        message: null,
        inbound_host: null,
        inbound_port: null,
      },
      subscription_count: 0,
      proxy_recovery_warning: null,
      system_proxy_applied: null,
      system_proxy_recorded: null,
      system_proxy_available: true,
      ...tunStatus,
    });
  });

  it("reconciles the startup cache with settings and updates the tray", async () => {
    getSettings.mockResolvedValue({ ...defaultSettings, language: "en" });

    const { container } = render(<App />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "Home" })).toBeInTheDocument();
    });
    expect(window.localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("en");
    expect(document.documentElement.lang).toBe("en");
    // The tray follows once the startup read marks the language ready.
    await waitFor(() => expect(setTrayLanguage).toHaveBeenCalledWith("en"));
    expect(restoreLaunchProxy).toHaveBeenCalled();
  });

  it("shows proxy recovery warning globally on any tab", async () => {
    getStatus.mockResolvedValue({
      core: {
        status: "error",
        message: { key: "core.exitedUnexpectedly", params: { code: "1" } },
        inbound_host: null,
        inbound_port: null,
      },
      subscription_count: 1,
      proxy_recovery_warning: [
        { key: "recover.proxyAfterExit", params: { detail: "mock" } },
      ],
      system_proxy_applied: null,
      system_proxy_recorded: null,
      system_proxy_available: true,
      ...tunStatus,
    });

    const { container } = render(<App />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("alert")).toHaveTextContent(
        t("recover.proxyAfterExit", { detail: "mock" }),
      );
    });
  });

  it("opens each page in the content pane", async () => {
    const view = within(render(<App />).container);
    fireEvent.click(view.getByRole("button", { name: t("app.nav.connections") }));
    await waitFor(() => {
      expect(view.getByTestId("connections-panel")).toBeInTheDocument();
    });

    const main = view.getByTestId("app-main");
    const panel = view.getByTestId("connections-panel");
    expect(main.contains(panel)).toBe(true);
    await waitFor(() => {
      expect(view.getByTestId("connections-empty")).toHaveTextContent(
        t("connections.empty"),
      );
    });

    fireEvent.click(view.getByRole("button", { name: t("app.nav.home") }));
    expect(view.getByTestId("home-panel")).toBeInTheDocument();

    fireEvent.click(view.getByRole("button", { name: t("app.nav.nodes") }));
    expect(view.getByTestId("nodes-panel")).toBeInTheDocument();

    fireEvent.click(view.getByRole("button", { name: t("app.nav.rules") }));
    expect(view.getByTestId("rules-panel")).toBeInTheDocument();
    const addRuleButton = screen.getByRole("button", { name: t("rules.addCustom") });
    expect(addRuleButton.closest("[data-titlebar-actions]")).not.toBeNull();
    expect(view.getByTestId("rules-panel").contains(addRuleButton)).toBe(false);

    fireEvent.click(view.getByRole("button", { name: t("app.nav.subs") }));
    expect(view.getByTestId("subs-panel")).toBeInTheDocument();
    const importButton = screen.getByRole("button", { name: t("subs.importTitle") });
    expect(importButton).toHaveAttribute("data-variant", "outline");
    expect(importButton.closest("[data-titlebar-actions]")).not.toBeNull();
    expect(view.getByTestId("subs-panel").contains(importButton)).toBe(false);

    fireEvent.click(view.getByRole("button", { name: t("app.nav.settings") }));
    expect(view.getByTestId("settings-panel")).toBeInTheDocument();
  });

  it("drops the outgoing page's title-bar actions in the switch frame", async () => {
    const { container } = render(<App />);
    const view = within(container);
    // Only visited tabs of the current shell mount a title-bar slot.
    expect(
      Array.from(container.querySelectorAll("[data-titlebar-slot]")).map((el) =>
        el.getAttribute("data-titlebar-slot"),
      ),
    ).toEqual(["home"]);

    fireEvent.click(view.getByRole("button", { name: t("app.nav.rules") }));
    const add = await screen.findByRole("button", { name: t("rules.addCustom") });
    const rulesSlot = add.closest("[data-titlebar-slot]");
    expect(rulesSlot).toHaveAttribute("data-titlebar-slot", "rules");
    expect(rulesSlot).not.toHaveAttribute("hidden");

    fireEvent.click(view.getByRole("button", { name: t("app.nav.subs") }));

    // The hidden page keeps its portal until its offscreen render commits,
    // but its slot is out of the bar at once: only Subscriptions' action shows.
    expect(rulesSlot).toHaveAttribute("hidden");
    const subsSlot = screen
      .getByRole("button", { name: t("subs.importTitle") })
      .closest("[data-titlebar-slot]");
    expect(subsSlot).toHaveAttribute("data-titlebar-slot", "subs");
    expect(screen.queryByRole("button", { name: t("rules.addCustom") })).toBeNull();

    // Returning to Rules reuses its mounted slot and restores its action.
    fireEvent.click(view.getByRole("button", { name: t("app.nav.rules") }));
    expect(subsSlot).toHaveAttribute("hidden");
    expect(rulesSlot).not.toHaveAttribute("hidden");
    expect(
      screen.getByRole("button", { name: t("rules.addCustom") }).closest("[data-titlebar-slot]"),
    ).toBe(rulesSlot);
    expect(container.querySelector('[data-titlebar-slot="apps"]')).toBeNull();
  });

  it("orders Connections, MCP, and Settings from top to bottom", async () => {
    const { container } = render(<App />);
    const view = within(container);
    const nav = container.querySelector('[data-slot="sidebar-menu"]');
    expect(nav).not.toBeNull();
    const buttons = within(nav as HTMLElement).getAllByRole("button");
    expect(buttons.every((button) => button.querySelector("svg.nav-icon"))).toBe(true);
    const labels = buttons.map((button) => button.textContent);
    expect(labels.slice(-3)).toEqual([
      t("app.nav.connections"),
      t("app.nav.mcp"),
      t("app.nav.settings"),
    ]);

    fireEvent.click(view.getByRole("button", { name: t("app.nav.mcp") }));
    await waitFor(() => {
      expect(view.getByTestId("mcp-panel")).toBeInTheDocument();
    });
    expect(getMcpStatus).toHaveBeenCalled();
  });

  it("applies appearance changes from settings to the document", async () => {
    const { container } = render(<App />);
    const view = within(container);
    fireEvent.click(view.getByRole("button", { name: t("app.nav.settings") }));

    await waitFor(() => {
      expect(view.getByLabelText(t("settings.theme"))).toBeInTheDocument();
    });
    const theme = view.getByLabelText(t("settings.theme"));

    fireEvent.change(theme, { target: { value: "dark" } });
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    fireEvent.change(theme, { target: { value: "light" } });
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    fireEvent.change(theme, { target: { value: "system" } });
    expect(theme).toHaveValue("system");
  });

  it("places the app brand at the bottom of the sidebar", () => {
    const { container } = render(<App />);
    const sidebar = container.querySelector('[data-slot="sidebar"]');
    expect(sidebar).not.toBeNull();
    const nav = sidebar!.querySelector('[data-slot="sidebar-menu"]');
    const brand = sidebar!.querySelector("h1");
    const version = sidebar!.querySelector(
      `[aria-label="${t("app.versionAria", { version: APP_VERSION })}"]`,
    );
    expect(nav).not.toBeNull();
    expect(brand).toHaveTextContent("ice-box");
    expect(container.querySelector('[data-testid="app-brand-row"]')?.contains(brand)).toBe(
      true,
    );
    expect(version).toHaveTextContent(APP_VERSION);
    expect(brand!.compareDocumentPosition(version!)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(nav!.compareDocumentPosition(brand!)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(sidebar!.querySelector('[data-slot="separator"]')).toBeNull();
  });

  it("puts Windows caption buttons at the top left in traffic-light order", () => {
    const platform = vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
    const userAgent = vi
      .spyOn(navigator, "userAgent", "get")
      .mockReturnValue("Mozilla/5.0 (Windows NT 10.0; Win64; x64)");
    try {
      const { container } = render(<App />);
      const titlebar = container.querySelector("[data-titlebar]");
      expect(titlebar).not.toBeNull();
      const controls = within(titlebar as HTMLElement).getByTestId("window-controls");
      const close = within(controls).getByRole("button", { name: t("window.close") });
      const minimize = within(controls).getByRole("button", {
        name: t("window.minimize"),
      });
      const maximize = within(controls).getByRole("button", {
        name: t("window.maximize"),
      });
      expect(close.compareDocumentPosition(minimize)).toBe(
        Node.DOCUMENT_POSITION_FOLLOWING,
      );
      expect(minimize.compareDocumentPosition(maximize)).toBe(
        Node.DOCUMENT_POSITION_FOLLOWING,
      );
      // No page title any more; the header's drag strip follows the controls.
      expect(titlebar!.querySelector("h2")).toBeNull();
      const title = titlebar!.querySelector("header [data-tauri-drag-region]");
      expect(title).not.toBeNull();
      expect(controls.compareDocumentPosition(title!)).toBe(
        Node.DOCUMENT_POSITION_FOLLOWING,
      );
      expect(titlebar!.firstElementChild).toBe(controls.parentElement);
      // The caption strip has no sidebar fill of its own; it only keeps the
      // sidebar's width and overlays the sidebar's top row. The desktop opens
      // collapsed, so the strip starts at the rail's width, which leaves room
      // for the caption buttons.
      expect(controls.parentElement).not.toHaveClass("bg-sidebar");
      expect(controls.parentElement).toHaveClass("w-(--sidebar-width-icon)");

      fireEvent.click(within(container).getByTestId("sidebar-collapse-toggle"));
      // Expanded, the strip follows the full sidebar width.
      expect(controls.parentElement).toHaveClass("w-(--sidebar-width)");
      expect(controls.parentElement).not.toHaveClass("bg-sidebar");
      expect(controls.compareDocumentPosition(title!)).toBe(
        Node.DOCUMENT_POSITION_FOLLOWING,
      );
      // The off-canvas trigger only exists in the narrow (sheet) layout.
      expect(
        within(titlebar as HTMLElement).queryByRole("button", { name: "Toggle Sidebar" }),
      ).toBeNull();
    } finally {
      platform.mockRestore();
      userAgent.mockRestore();
    }
  });

  it("opens the desktop sidebar collapsed and remembers the last choice", () => {
    const first = render(<App />);
    const sidebar = () => document.querySelector('[data-slot="sidebar"]');
    expect(sidebar()).toHaveAttribute("data-state", "collapsed");

    fireEvent.click(within(first.container).getByTestId("sidebar-collapse-toggle"));
    expect(sidebar()).toHaveAttribute("data-state", "expanded");
    expect(window.localStorage.getItem("ice-box.sidebar-open")).toBe("true");
    first.unmount();

    // Next launch: the expanded choice is restored.
    const second = render(<App />);
    expect(sidebar()).toHaveAttribute("data-state", "expanded");
    fireEvent.click(within(second.container).getByTestId("sidebar-collapse-toggle"));
    expect(window.localStorage.getItem("ice-box.sidebar-open")).toBe("false");
    second.unmount();

    render(<App />);
    expect(sidebar()).toHaveAttribute("data-state", "collapsed");
  });

  it("holds the content pane's width while the window follows a collapse", async () => {
    // The window resizes in one step while the sidebar animates, so the pane
    // is pinned to its width until the window has caught up; otherwise it
    // would narrow and widen back during the animation.
    const resolvers: Array<(resized: boolean) => void> = [];
    windowResize.fn = vi.fn(
      () => new Promise<boolean>((resolve) => resolvers.push(resolve)),
    );
    const rect = vi
      .spyOn(HTMLElement.prototype, "getBoundingClientRect")
      .mockReturnValue({ width: 652, height: 500, top: 0, left: 0, right: 652, bottom: 500, x: 0, y: 0, toJSON: () => ({}) } as DOMRect);
    window.localStorage.setItem("ice-box.sidebar-open", "true");
    try {
      const { container } = render(<App />);
      const view = within(container);
      const content = view.getByTestId("app-content");

      fireEvent.click(view.getByTestId("sidebar-collapse-toggle"));
      expect(windowResize.fn).toHaveBeenLastCalledWith(-120, { width: 600, height: 500 });
      expect(content).toHaveAttribute("data-pinned", "true");
      expect(content.style.width).toBe("652px");

      // The window reaches its new width: the pin is released once the
      // sidebar animation is over.
      window.innerWidth = 1280 - 120;
      await act(async () => resolvers[resolvers.length - 1](true));
      await waitFor(() => expect(content).not.toHaveAttribute("data-pinned"), {
        timeout: 1500,
      });
      expect(content.style.width).toBe("");

      // A maximized window keeps its size: the pane follows the sidebar.
      fireEvent.click(view.getByTestId("sidebar-collapse-toggle"));
      expect(content).toHaveAttribute("data-pinned", "true");
      await act(async () => resolvers[resolvers.length - 1](false));
      expect(content).not.toHaveAttribute("data-pinned");
    } finally {
      rect.mockRestore();
      windowResize.fn = null;
      window.localStorage.removeItem("ice-box.sidebar-open");
    }
  });

  it("keeps the icon rail when collapsing leaves the window under 768px", () => {
    // Collapsing shrinks the window by the sidebar delta; on macOS that takes
    // the default 860px window to 740px. The rail must stay, not turn into
    // an off-canvas sheet at the 768px phone breakpoint.
    window.localStorage.setItem("ice-box.sidebar-open", "false");
    window.innerWidth = 740;
    try {
      const { container } = render(<App />);
      const sidebar = container.querySelector('[data-slot="sidebar"]');
      expect(sidebar).not.toBeNull();
      expect(sidebar).toHaveAttribute("data-collapsible", "icon");
      expect(sidebar).not.toHaveAttribute("data-mobile");
      expect(sidebar).not.toHaveClass("hidden");
      expect(container.querySelector('[data-slot="sidebar-container"]')).not.toHaveClass(
        "hidden",
      );
      expect(
        within(container.querySelector("[data-titlebar]") as HTMLElement).queryByRole(
          "button",
          { name: "Toggle Sidebar" },
        ),
      ).toBeNull();
    } finally {
      window.localStorage.removeItem("ice-box.sidebar-open");
    }
  });

  it("draws the titlebar divider as one shared edge", () => {
    const { container } = render(<App />);
    const titlebar = container.querySelector("[data-titlebar]");
    expect(titlebar).not.toBeNull();
    // The page title was removed: the selected sidebar item names the page.
    expect(titlebar!.querySelector("h2")).toBeNull();
    const sidebar = container.querySelector('[data-slot="sidebar"]');
    expect(sidebar).not.toBeNull();
    expect(titlebar!.compareDocumentPosition(sidebar!)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
  });

  it("keeps overlay drag regions in the sidebar chrome and page header", () => {
    const { container } = render(<App />);
    const regions = container.querySelectorAll("[data-tauri-drag-region]");
    expect(regions.length).toBe(4);
    expect(regions[1]).toHaveAttribute("aria-label", t("app.nav.home"));
    expect(regions[1]).toBeEmptyDOMElement();
    expect(regions[2]).toHaveTextContent("ice-box");
    expect(regions[3]).toHaveTextContent(APP_VERSION);
  });

  it("keeps accessible names on collapsed desktop navigation buttons", () => {
    const { container } = render(<App />);
    const sidebar = container.querySelector('[data-slot="sidebar"]');
    expect(sidebar).toHaveAttribute("data-state", "collapsed");
    for (const label of [
      t("app.nav.home"),
      t("app.nav.nodes"),
      t("app.nav.rules"),
      t("app.nav.subs"),
      t("app.nav.connections"),
      t("app.nav.mcp"),
      t("app.nav.settings"),
    ]) {
      const button = within(container).getByRole("button", { name: label });
      expect(button).toHaveAttribute("aria-label", label);
    }
  });

  it("grows the desktop window at launch when an expanded sidebar was remembered", () => {
    const resize = vi.fn().mockResolvedValue(true);
    windowResize.fn = resize;
    window.localStorage.setItem("ice-box.sidebar-open", "true");
    try {
      render(<App />);
      expect(resize).toHaveBeenCalledWith(120, { width: 720, height: 500 });
    } finally {
      window.localStorage.removeItem("ice-box.sidebar-open");
    }
  });

  it("drags and maximizes from the content top strip, but ignores portaled modals and selectable text", async () => {
    const cmd = vi.fn().mockResolvedValue(undefined);
    windowCommand.fn = cmd;
    const { container } = render(<App />);
    const view = within(container);
    const content = view.getByTestId("app-content");

    // Plain background in the top 48px starts drag / toggles maximize.
    fireEvent.mouseDown(content, { button: 0, clientY: 16 });
    expect(cmd).toHaveBeenCalledWith("startDragging");
    fireEvent.doubleClick(content, { button: 0, clientY: 16 });
    expect(cmd).toHaveBeenCalledWith("toggleMaximize");
    cmd.mockClear();

    // Selectable text inside the top 48px must not hijack pointer drags.
    const selectable = document.createElement("span");
    selectable.setAttribute("data-selectable", "");
    content.appendChild(selectable);
    fireEvent.mouseDown(selectable, { button: 0, clientY: 16 });
    fireEvent.doubleClick(selectable, { button: 0, clientY: 16 });
    expect(cmd).not.toHaveBeenCalled();
    selectable.remove();

    // Opening a portaled dialog (LandingMapDialog) renders its overlay into
    // document.body. Synthetic events bubble through the React tree into
    // app-content, so clicks in the top 48px must not start a window drag.
    fireEvent.click(view.getByTestId("home-landing-expand"));
    const overlay = await waitFor(() => {
      const el = document.querySelector('[data-slot="dialog-overlay"]');
      expect(el).not.toBeNull();
      return el as HTMLElement;
    });
    fireEvent.mouseDown(overlay, { button: 0, clientY: 12 });
    fireEvent.doubleClick(overlay, { button: 0, clientY: 12 });
    expect(cmd).not.toHaveBeenCalled();
  });

  it("keeps the nodes list mounted when switching away and back", async () => {
    listNodes.mockResolvedValue([
      {
        tag: "proxy-1",
        outbound_type: "trojan",
        group_now: null,
        group_all: null,
      },
    ]);
    const { container } = render(<App />);
    const view = within(container);

    fireEvent.click(view.getByRole("button", { name: t("app.nav.nodes") }));
    await waitFor(() => {
      expect(listNodes).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(view.getByText("proxy-1")).toBeInTheDocument();
    });
    expect(view.queryByText(t("nodes.emptyTitle"))).toBeNull();
    // Page actions moved out of the title bar into a floating action area in
    // the content, so every page can start at the window top.
    expect(container.querySelector("[data-titlebar] [data-titlebar-actions]")).toBeNull();
    const actions = container.querySelector("[data-page-toolbar]");
    expect(actions).not.toBeNull();
    const nodesSlot = container.querySelector('[data-titlebar-slot="nodes"]');
    expect(actions!.contains(nodesSlot)).toBe(true);
    expect(nodesSlot).not.toHaveAttribute("hidden");
    const batch = within(actions as HTMLElement).getByRole("button", {
      name: t("nodes.batchTest"),
    });
    expect(batch).toHaveAttribute("data-variant", "outline");
    expect(view.getByTestId("nodes-panel").contains(batch)).toBe(false);

    fireEvent.click(view.getByRole("button", { name: t("app.nav.home") }));
    expect(
      container.querySelector("[data-titlebar-actions] button"),
    ).toBeNull();
    const panel = view.getByTestId("nodes-panel");
    expect(panel.parentElement).toHaveAttribute("data-active", "false");
    // Home and Nodes stay laid out while hidden: moved out of view and out of
    // the accessibility tree at once, out of keyboard reach after the paint.
    expect(panel.parentElement).toHaveClass("pane-kept-hidden");
    expect(panel.parentElement).toHaveAttribute("aria-hidden", "true");
    expect(view.getByText("proxy-1")).toBeInTheDocument();
    await waitFor(() => {
      expect(panel.parentElement).toHaveAttribute("inert");
    });
    const home = view.getByTestId("home-panel").parentElement;
    expect(home).not.toHaveClass("pane-kept-hidden");
    expect(home).not.toHaveAttribute("aria-hidden");
    expect(home).not.toHaveAttribute("inert");

    fireEvent.click(view.getByRole("button", { name: t("app.nav.nodes") }));
    expect(view.getByText("proxy-1")).toBeInTheDocument();
    expect(view.queryByText(t("nodes.emptyTitle"))).toBeNull();
    expect(panel.parentElement).toHaveAttribute("data-active", "true");
    expect(panel.parentElement).not.toHaveAttribute("aria-hidden");
    await waitFor(() => {
      expect(panel.parentElement).not.toHaveAttribute("inert");
    });
    expect(
      within(actions as HTMLElement).getByRole("button", { name: t("nodes.batchTest") }),
    ).toBeInTheDocument();
  });

  it("moves focus out of a kept pane in the frame it hides", async () => {
    const { container } = render(<App />);
    const view = within(container);
    const home = view.getByTestId("home-panel");
    const control = await waitFor(() => {
      const button = home.querySelector("button");
      expect(button).not.toBeNull();
      return button as HTMLButtonElement;
    });
    control.focus();
    expect(document.activeElement).toBe(control);

    // A tray switch moves no focus itself; the pane must drop it before
    // `inert` lands a frame later.
    fireEvent.click(view.getByRole("button", { name: t("app.nav.nodes") }));
    expect(home.parentElement).toHaveAttribute("aria-hidden", "true");
    expect(home.parentElement).not.toHaveAttribute("inert");
    expect(home.contains(document.activeElement)).toBe(false);
  });

  it("shows a sidebar upgrade icon when a background check finds a version", async () => {
    checkAppUpdate.mockResolvedValue({
      available: true,
      version: "0.1.6",
      notes: "fixes",
      skipped: false,
      should_prompt: false,
    });
    render(<App />);
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: t("app.updateAvailableAria", { version: "0.1.6" }) }),
      ).toBeInTheDocument();
    });
    expect(checkAppUpdate).toHaveBeenCalledWith(true, true);
    expect(screen.queryByRole("alertdialog")).toBeNull();
    // A badge on the logo's corner, out of flow, so the logo does not move.
    const badge = screen.getByTestId("app-update-badge");
    expect(badge).toHaveClass("absolute");
    expect(badge.parentElement!.querySelector(".status-logo")).not.toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: t("app.updateAvailableAria", { version: "0.1.6" }) }),
    );
    await waitFor(() => {
      expect(screen.getByRole("button", { name: t("settings.updateInstall") })).toBeEnabled();
    });
    expect(screen.getByText(t("settings.updateAvailable", { version: "0.1.6" }))).toBeInTheDocument();
  });

  it("mirrors the available version into the tray menu prompt", async () => {
    checkAppUpdate.mockResolvedValue({
      available: true,
      version: "0.1.6",
      notes: "fixes",
      skipped: false,
      should_prompt: false,
    });
    render(<App />);
    await waitFor(() => {
      expect(setTrayUpdateAvailable).toHaveBeenCalledWith("0.1.6");
    });
  });

  it("drops the tray menu prompt when no update is on offer", async () => {
    render(<App />);
    await waitFor(() => {
      expect(setTrayUpdateAvailable).toHaveBeenCalledWith(null);
    });
  });

  it("opens App Updates when the tray menu prompt is clicked", async () => {
    let trayClick: (() => void) | null = null;
    listenTrayUpdateClick.mockImplementation((handler: () => void) => {
      trayClick = handler;
      return Promise.resolve(() => {});
    });
    checkAppUpdate.mockResolvedValue({
      available: true,
      version: "0.1.6",
      notes: "fixes",
      skipped: false,
      should_prompt: false,
    });
    render(<App />);
    await waitFor(() => {
      expect(trayClick).not.toBeNull();
    });
    act(() => trayClick?.());
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: t("settings.updateInstall") }),
      ).toBeEnabled();
    });
  });

  it("hides the sidebar upgrade icon when automatic checks are turned off", async () => {
    checkAppUpdate.mockResolvedValue({
      available: true,
      version: "0.1.6",
      notes: "fixes",
      skipped: false,
      should_prompt: false,
    });
    render(<App />);
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: t("app.updateAvailableAria", { version: "0.1.6" }) }),
      ).toBeInTheDocument();
    });
    fireEvent.click(
      screen.getByRole("button", { name: t("app.updateAvailableAria", { version: "0.1.6" }) }),
    );
    await waitFor(() => {
      expect(screen.getByLabelText(t("settings.updateAutoCheck"))).toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: t("settings.updateInstall") })).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText(t("settings.updateAutoCheck")));
    await waitFor(() => {
      expect(
        screen.queryByRole("button", { name: t("app.updateAvailableAria", { version: "0.1.6" }) }),
      ).toBeNull();
    });
    expect(setTrayUpdateAvailable).toHaveBeenLastCalledWith(null);
    expect(screen.getByRole("button", { name: t("settings.updateInstall") })).toBeInTheDocument();
  });

  it("uses a bottom tab bar on the phone shell", async () => {
    phoneShell.value = true;
    desktopShell.value = false;
    window.innerWidth = 390;

    const { container } = render(<App />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByTestId("phone-tab-bar")).toBeInTheDocument();
    });
    const header = view.getByTestId("phone-header");
    expect(header).toHaveClass("phone-header", "absolute", "top-0");
    expect(header.querySelector("h2")).toBeNull();
    const tabBar = view.getByTestId("phone-tab-bar");
    expect(tabBar).toHaveClass("phone-tab-bar", "rounded-full");
    expect(tabBar.style.getPropertyValue("--tab-index")).toBe("0");
    expect(tabBar.style.getPropertyValue("--tab-count")).toBe("5");
    expect(view.getByTestId("phone-tab-indicator")).toBeInTheDocument();
    expect(view.getByTestId("app-main")).toHaveClass("pt-3", "pb-0");
    expect(view.queryByTestId("app-brand-row")).not.toBeInTheDocument();
    const home = view.getByRole("button", { name: t("app.nav.home") });
    expect(home).toHaveAttribute("aria-current", "page");
    // Top-aligned 8px padding, not justify-center: a centered glyph in this
    // row lands on a half-pixel and blurs off 2x.
    expect(home).toHaveClass("py-2", "leading-[14px]");
    expect(home).not.toHaveClass("justify-center", "leading-tight");
    const apps = view.getByRole("button", { name: t("app.nav.apps") });
    expect(apps).toBeInTheDocument();
    expect(view.queryByRole("button", { name: t("app.nav.nodes") })).not.toBeInTheDocument();
    expect(view.getByTestId("nodes-panel")).toBeInTheDocument();
    // On the phone Home screen the whole page scrolls as one surface rather
    // than trapping the embedded node list in a nested scroll viewport.
    expect(
      view
        .getByTestId("nodes-panel")
        .querySelector('[data-slot="scroll-area-viewport"]'),
    ).toBeNull();
    expect(view.queryByRole("button", { name: t("app.nav.connections") })).not.toBeInTheDocument();
    expect(view.queryByRole("button", { name: t("app.nav.mcp") })).not.toBeInTheDocument();
    expect(view.queryByTestId("connections-panel")).not.toBeInTheDocument();
    expect(view.queryByTestId("mcp-panel")).not.toBeInTheDocument();
    expect(restoreLaunchProxy).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(checkAppUpdate).toHaveBeenCalled();
    });
    expect(setTrayUpdateAvailable).not.toHaveBeenCalled();

    // Switching tabs glides the floating selection indicator to the active slot.
    fireEvent.click(apps);
    expect(tabBar.style.getPropertyValue("--tab-index")).toBe("1");
    expect(apps).toHaveAttribute("aria-current", "page");
  });

  it("opens the Apps page from the phone tab bar", async () => {
    phoneShell.value = true;
    desktopShell.value = false;
    window.innerWidth = 390;
    listApps.mockResolvedValue([
      {
        package_name: "com.example.browser",
        label: "Browser",
        system: false,
        icon: null,
      },
    ]);

    const { container } = render(<App />);
    const view = within(container);

    fireEvent.click(
      await view.findByRole("button", { name: t("app.nav.apps") }),
    );
    await waitFor(() => {
      expect(view.getByTestId("apps-panel")).toBeInTheDocument();
    });
    expect(
      await view.findByRole("button", {
        name: t("apps.selectAria", { app: "Browser" }),
      }),
    ).toBeInTheDocument();
    expect(
      view.getByTestId("nodes-panel").closest("[data-active]"),
    ).toHaveAttribute("data-active", "false");
  });

  it("uses the sidebar when the phone shell is wide", async () => {
    phoneShell.value = true;
    desktopShell.value = false;
    window.innerWidth = 1280;

    const { container } = render(<App />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByTestId("tablet-shell")).toBeInTheDocument();
    });
    expect(view.getByTestId("app-brand-row")).toBeInTheDocument();
    expect(view.queryByTestId("phone-tab-bar")).not.toBeInTheDocument();
    expect(container.querySelector("[data-titlebar]")).toBeNull();
    expect(view.getByRole("button", { name: t("app.nav.home") })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(view.getByRole("button", { name: t("app.nav.apps") })).toBeInTheDocument();
    expect(view.queryByRole("button", { name: t("app.nav.nodes") })).not.toBeInTheDocument();
    expect(view.queryByRole("button", { name: t("app.nav.connections") })).not.toBeInTheDocument();
    expect(view.queryByRole("button", { name: t("app.nav.mcp") })).not.toBeInTheDocument();
    expect(restoreLaunchProxy).not.toHaveBeenCalled();
    expect(setTrayUpdateAvailable).not.toHaveBeenCalled();
  });

  it("keeps MCP out of the website shell", async () => {
    desktopShell.value = false;

    const { container } = render(<App />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: t("app.nav.home") })).toBeInTheDocument();
    });
    expect(view.getByRole("button", { name: t("app.nav.connections") })).toBeInTheDocument();
    expect(view.queryByRole("button", { name: t("app.nav.mcp") })).not.toBeInTheDocument();
    expect(view.queryByTestId("mcp-panel")).not.toBeInTheDocument();
  });
});
