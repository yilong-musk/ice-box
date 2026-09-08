// SPDX-License-Identifier: GPL-3.0-or-later

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { APP_VERSION } from "./lib/appVersion";
import { LANGUAGE_STORAGE_KEY, t, isMessageKey } from "./lib/i18n";
import { clearNodesSnapshot } from "./lib/nodes";

const getStatus = vi.fn();
const getSettings = vi.fn();
const saveSettings = vi.fn();
const listNodes = vi.fn();
const setTrayLanguage = vi.fn();
const restoreLaunchProxy = vi.fn();
const checkAppUpdate = vi.fn();
const installAppUpdate = vi.fn();

const defaultSettings = {
  mixed_listen: "127.0.0.1",
  mixed_port: 17890,
  clash_api_listen: "127.0.0.1",
  clash_api_port: 19090,
  selected_tag: null,
  auto_set_system_proxy: false,
  proxy_service_enabled: false,
  allow_lan: false,
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
  log_debug: false,
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
  helper_installed: false,
  helper_supported: true,
  helper_stale: false,
} as const;

vi.mock("./api/tauri", () => ({
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
    getSettings: (...args: unknown[]) => getSettings(...args),
    restoreLaunchProxy: (...args: unknown[]) => restoreLaunchProxy(...args),
    checkAppUpdate: (...args: unknown[]) => checkAppUpdate(...args),
    installAppUpdate: (...args: unknown[]) => installAppUpdate(...args),
    listenAppUpdateProgress: vi.fn().mockResolvedValue(() => {}),
    saveSettings: (...args: unknown[]) => saveSettings(...args),
    setTrayLanguage: (...args: unknown[]) => setTrayLanguage(...args),
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
    getLogView: vi.fn().mockResolvedValue([]),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

describe("App", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    clearNodesSnapshot();
    getSettings.mockResolvedValue(defaultSettings);
    restoreLaunchProxy.mockResolvedValue(undefined);
    setTrayLanguage.mockResolvedValue(undefined);
    listNodes.mockResolvedValue([]);
    checkAppUpdate.mockResolvedValue({
      available: false,
      version: null,
      notes: null,
      skipped: false,
      should_prompt: false,
    });
    installAppUpdate.mockResolvedValue(undefined);
    saveSettings.mockResolvedValue(undefined);
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
    expect(setTrayLanguage).toHaveBeenCalledWith("en");
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

  it("lets the logs panel fill the content pane", async () => {
    const view = within(render(<App />).container);
    fireEvent.click(view.getByRole("button", { name: t("app.nav.logs") }));
    await waitFor(() => {
      expect(view.getByTestId("logs-panel")).toBeInTheDocument();
    });

    const main = view.getByTestId("app-main");
    const panel = view.getByTestId("logs-panel");
    const logView = view.getByTestId("log-view");
    expect(main.contains(panel)).toBe(true);
    expect(panel.contains(logView)).toBe(true);
    expect(logView.parentElement).toBe(panel);
    expect(view.queryByRole("button", { name: t("common.refresh") })).toBeNull();
    expect(panel.querySelector("[data-slot='card']")).toBeNull();

    fireEvent.click(view.getByRole("button", { name: t("app.nav.home") }));
    expect(view.getByTestId("home-panel")).toBeInTheDocument();

    fireEvent.click(view.getByRole("button", { name: t("app.nav.nodes") }));
    expect(view.getByTestId("nodes-panel")).toBeInTheDocument();

    fireEvent.click(view.getByRole("button", { name: t("app.nav.rules") }));
    expect(view.getByTestId("rules-panel")).toBeInTheDocument();

    fireEvent.click(view.getByRole("button", { name: t("app.nav.subs") }));
    expect(view.getByTestId("subs-panel")).toBeInTheDocument();

    fireEvent.click(view.getByRole("button", { name: t("app.nav.settings") }));
    expect(view.getByTestId("settings-panel")).toBeInTheDocument();
  });

  it("applies appearance changes from settings to the document", async () => {
    const { container } = render(<App />);
    const view = within(container);
    fireEvent.click(view.getByRole("button", { name: t("app.nav.settings") }));

    await waitFor(() => {
      expect(view.getByLabelText(t("settings.appearance"))).toBeInTheDocument();
    });
    const appearance = view.getByLabelText(t("settings.appearance"));

    fireEvent.click(within(appearance).getByRole("radio", { name: t("settings.appearance.dark") }));
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    fireEvent.click(within(appearance).getByRole("radio", { name: t("settings.appearance.light") }));
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    fireEvent.click(
      within(appearance).getByRole("radio", { name: t("settings.appearance.system") }),
    );
    expect(
      within(appearance).getByRole("radio", { name: t("settings.appearance.system") }),
    ).toHaveAttribute("data-state", "on");
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

  it("draws the titlebar divider as one shared edge", () => {
    const { container } = render(<App />);
    const titlebar = container.querySelector("[data-titlebar]");
    expect(titlebar).not.toBeNull();
    expect(titlebar!.querySelector("h2")).toHaveTextContent(t("app.nav.home"));
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
    expect(regions[1]).toHaveTextContent(t("app.nav.home"));
    expect(regions[2]).toHaveTextContent("ice-box");
    expect(regions[3]).toHaveTextContent(APP_VERSION);
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

    await waitFor(() => {
      expect(listNodes).toHaveBeenCalled();
    });
    fireEvent.click(view.getByRole("button", { name: t("app.nav.nodes") }));
    await waitFor(() => {
      expect(view.getByText("proxy-1")).toBeInTheDocument();
    });
    expect(view.queryByText(t("nodes.emptyTitle"))).toBeNull();

    fireEvent.click(view.getByRole("button", { name: t("app.nav.home") }));
    const panel = view.getByTestId("nodes-panel");
    expect(panel.parentElement).toHaveAttribute("data-active", "false");

    fireEvent.click(view.getByRole("button", { name: t("app.nav.nodes") }));
    expect(view.getByText("proxy-1")).toBeInTheDocument();
    expect(view.queryByText(t("nodes.emptyTitle"))).toBeNull();
    expect(panel.parentElement).toHaveAttribute("data-active", "true");
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
    expect(checkAppUpdate).toHaveBeenCalledWith(true);
    expect(screen.queryByRole("alertdialog")).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: t("app.updateAvailableAria", { version: "0.1.6" }) }),
    );
    await waitFor(() => {
      expect(screen.getByRole("button", { name: t("settings.updateInstall") })).toBeEnabled();
    });
    expect(screen.getByText(t("settings.updateAvailable", { version: "0.1.6" }))).toBeInTheDocument();
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
    expect(screen.getByRole("button", { name: t("settings.updateInstall") })).toBeInTheDocument();
  });
});
