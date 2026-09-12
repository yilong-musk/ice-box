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
import { THEME_STORAGE_KEY } from "../lib/theme";
import {
  LANGUAGE_STORAGE_KEY,
  persistLanguagePreference,
  t,
  isMessageKey,
} from "../lib/i18n";
import { Settings } from "./Settings";
import { RuntimeStoreProvider } from "../lib/runtimeStore";

/** The macOS-only menu bar card is gated on the host classifier; jsdom is not a
 * macOS host, so that one test flips the flag. */
const hostState = vi.hoisted(() => ({ macos: false }));
vi.mock("../lib/windowChrome", () => ({
  isMacosHost: () => hostState.macos,
}));

const getSettings = vi.fn();
const getStatus = vi.fn();
const saveSettings = vi.fn();
const installHelper = vi.fn();
const uninstallHelper = vi.fn();
const relaunchElevatedForTun = vi.fn();
const ensureTunElevation = vi.fn();
const start = vi.fn();
const checkAppUpdate = vi.fn();
const installAppUpdate = vi.fn();
const listenCoreStatusChanged = vi.fn().mockResolvedValue(() => {});
const listenWindowHidden = vi.fn().mockResolvedValue(() => {});
const listenWindowShown = vi.fn().mockResolvedValue(() => {});
const listenStateChanged = vi.fn().mockResolvedValue(() => {});

vi.mock("../api/tauri", () => ({
  api: {
    getSettings: (...args: unknown[]) => getSettings(...args),
    getStatus: (...args: unknown[]) => getStatus(...args),
    saveSettings: (...args: unknown[]) => saveSettings(...args),
    checkAppUpdate: (...args: unknown[]) => checkAppUpdate(...args),
    recordUpdatePrompt: vi.fn(),
    skipAppUpdate: vi.fn(),
    installAppUpdate: (...args: unknown[]) => installAppUpdate(...args),
    listenAppUpdateProgress: vi.fn().mockResolvedValue(() => {}),
    listenCoreStatusChanged: (...args: unknown[]) =>
      listenCoreStatusChanged(...args),
    listenWindowHidden: (...args: unknown[]) => listenWindowHidden(...args),
    listenWindowShown: (...args: unknown[]) => listenWindowShown(...args),
    listenStateChanged: (...args: unknown[]) => listenStateChanged(...args),
    installHelper: (...args: unknown[]) => installHelper(...args),
    uninstallHelper: (...args: unknown[]) => uninstallHelper(...args),
    relaunchElevatedForTun: (...args: unknown[]) =>
      relaunchElevatedForTun(...args),
    ensureTunElevation: (...args: unknown[]) => ensureTunElevation(...args),
    start: (...args: unknown[]) => start(...args),
    revealDataDir: vi.fn(),
  },
  formatInvokeError: (err: unknown) => String(err),
  formatUiMessage: (msg: unknown) => {
    if (!msg) return "";
    if (typeof msg === "string") return msg;
    const m = msg as { key?: string; params?: Record<string, string> };
    if (!m.key) return String(msg);
    if (m.key === "ui.raw") return m.params?.text ?? "";
    return isMessageKey(m.key) ? t(m.key, m.params) : m.key;
  },
}));

const tunSettings = {
  enabled: false,
  interface_name: null,
  ipv4_address: "10.0.0.1/30",
  ipv6_address: "fdfe:dcba:9876::1/126",
  mtu: 9000,
  auto_route: true,
  strict_route: true,
  stack: "gvisor",
  dns_hijack: false,
} as const;

const defaultStatus = {
  core: { status: "stopped", message: null, inbound_host: null, inbound_port: null },
  subscription_count: 0,
  proxy_recovery_warning: null,
  system_proxy_applied: null,
  system_proxy_recorded: null,
  system_proxy_available: true,
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

describe("Settings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    hostState.macos = false;
    saveSettings.mockResolvedValue(undefined);
    window.localStorage.removeItem(THEME_STORAGE_KEY);
    document.documentElement.classList.remove("dark");
    getSettings.mockResolvedValue({
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
      language: "system",
      check_app_updates: true,
      log_debug: false,
      tray_display_mode: "icon_and_speed",
      tun: tunSettings,
    });
    getStatus.mockResolvedValue({ ...defaultStatus });
    checkAppUpdate.mockResolvedValue({
      available: false,
      version: null,
      notes: null,
      skipped: false,
      should_prompt: false,
    });
    installAppUpdate.mockResolvedValue(undefined);
  });

  it("auto-saves only valid settings and blocks invalid ones", async () => {
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByDisplayValue("17890")).toBeInTheDocument();
    });

    const portInput = view.getByDisplayValue("17890");
    fireEvent.change(portInput, { target: { value: "80" } });

    await waitFor(
      () => {
        expect(container.textContent).toContain("1024");
        expect(saveSettings).not.toHaveBeenCalled();
      },
      { timeout: 2000 },
    );

    fireEvent.change(portInput, { target: { value: "18080" } });
    await waitFor(
      () => {
        expect(saveSettings).toHaveBeenCalledWith(
          expect.objectContaining({ mixed_port: 18080 }),
        );
        const patch = saveSettings.mock.calls[0][0] as {
          tun?: { enabled?: boolean };
        };
        expect(patch.tun?.enabled).toBeUndefined();
      },
      { timeout: 2000 },
    );
  });

  it("blocks invalid listen address from being saved", async () => {
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getAllByDisplayValue("127.0.0.1").length).toBeGreaterThan(0);
    });

    const listenInputs = view.getAllByDisplayValue("127.0.0.1");
    fireEvent.change(listenInputs[0], { target: { value: "0.0.0.0" } });

    await waitFor(
      () => {
        expect(container.textContent).toContain("loopback");
        expect(saveSettings).not.toHaveBeenCalled();
      },
      { timeout: 2000 },
    );
  });

  it("allows non-loopback mixed listen when allow_lan is on", async () => {
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByLabelText(t("settings.allowLan"))).toBeInTheDocument();
    });

    fireEvent.click(view.getByLabelText(t("settings.allowLan")));

    await waitFor(() => {
      const listenInputs = view.getAllByDisplayValue("127.0.0.1");
      expect(listenInputs[0]).toBeDisabled();
      expect(listenInputs[1]).not.toBeDisabled();
    });

    fireEvent.change(view.getByDisplayValue("19090"), {
      target: { value: "19190" },
    });

    await waitFor(
      () => {
        expect(saveSettings).toHaveBeenCalledWith(
          expect.objectContaining({
            allow_lan: true,
            clash_api_port: 19190,
          }),
        );
        expect(container.textContent).not.toContain("loopback");
      },
      { timeout: 2000 },
    );
  });

  it("default rules toggle defaults on and saves the change", async () => {
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(
        view.getByLabelText(t("settings.autoDefaultRules")),
      ).toBeInTheDocument();
    });

    const toggle = view.getByLabelText(t("settings.autoDefaultRules"));
    expect(toggle).toBeChecked();

    fireEvent.click(toggle);
    await waitFor(
      () => {
        expect(saveSettings).toHaveBeenCalledWith(
          expect.objectContaining({ auto_default_rules: false }),
        );
      },
      { timeout: 2000 },
    );
    expect(toggle).not.toBeChecked();
  });

  it("persists turning on debug logs", async () => {
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByLabelText(t("settings.logDebug"))).toBeInTheDocument();
    });
    const inboundCard = view
      .getByText(t("settings.inbound"))
      .closest('[data-slot="card"]');
    expect(inboundCard).toBeTruthy();
    expect(
      within(inboundCard as HTMLElement).queryByLabelText(
        t("settings.logDebug"),
      ),
    ).toBeNull();
    expect(
      within(inboundCard as HTMLElement).queryByText(t("settings.openDataDir")),
    ).toBeNull();
    const dataCard = view
      .getByText(t("settings.data"))
      .closest('[data-slot="card"]');
    expect(dataCard).toBeTruthy();
    expect(
      within(dataCard as HTMLElement).getByText(t("settings.openDataDir")),
    ).toBeInTheDocument();
    const toggle = within(dataCard as HTMLElement).getByLabelText(
      t("settings.logDebug"),
    );
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);
    await waitFor(
      () => {
        expect(saveSettings).toHaveBeenCalledWith(
          expect.objectContaining({ log_debug: true }),
        );
      },
      { timeout: 2000 },
    );
    expect(toggle).toBeChecked();
  });

  it("blocks save when mixed and clash api ports conflict", async () => {
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByDisplayValue("19090")).toBeInTheDocument();
    });

    const clashPort = view.getByDisplayValue("19090");
    fireEvent.change(clashPort, { target: { value: "17890" } });

    await waitFor(
      () => {
        expect(container.textContent).toContain(t("validation.portsConflict"));
        expect(saveSettings).not.toHaveBeenCalled();
      },
      { timeout: 2000 },
    );
  });

  it("does not save anything before settings load completes", async () => {
    let resolveSettings: (value: unknown) => void = () => {};
    getSettings.mockReturnValue(
      new Promise((resolve) => {
        resolveSettings = resolve;
      }),
    );

    const { container } = render(<Settings />);
    const view = within(container);

    const mixedInput = await view.findByLabelText(t("settings.mixedListen"));
    const language = view.getByLabelText(t("settings.language"));
    expect(mixedInput).toBeDisabled();
    expect(language).toBeDisabled();

    resolveSettings({
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
      language: "system",
      check_app_updates: true,
      log_debug: false,
      tray_display_mode: "icon_and_speed",
      tun: tunSettings,
    });

    await waitFor(() => {
      expect(mixedInput).not.toBeDisabled();
    });
    expect(language).not.toBeDisabled();
    // Opening the page never writes the just-loaded snapshot back.
    await new Promise((resolve) => setTimeout(resolve, 600));
    expect(saveSettings).not.toHaveBeenCalled();
  });

  it("stays read-only when settings load fails", async () => {
    getSettings.mockRejectedValue("load failed");

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(container.textContent).toContain("load failed");
    });

    const mixedInput = view.getByLabelText(t("settings.mixedListen"));
    expect(mixedInput).toBeDisabled();
    expect(view.getByLabelText(t("settings.language"))).toBeDisabled();
    expect(saveSettings).not.toHaveBeenCalled();
  });

  it("reloads settings when the panel becomes active again", async () => {
    const initial = {
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: null,
      auto_set_system_proxy: false,
      proxy_service_enabled: false,
      allow_lan: false,
      proxy_mode: "rule" as const,
      language: "system" as const,
      check_app_updates: true,
      log_debug: false,
      tray_display_mode: "icon_and_speed",
      tun: tunSettings,
    };
    const updated = { ...initial, mixed_port: 17900, proxy_mode: "global" as const };
    getSettings.mockResolvedValueOnce(initial).mockResolvedValueOnce(updated);

    const { container, rerender } = render(<Settings active />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByDisplayValue("17890")).toBeInTheDocument();
    });

    rerender(<Settings active={false} />);
    expect(view.getByLabelText(t("settings.mixedListen"))).toBeDisabled();
    rerender(<Settings active />);

    await waitFor(() => {
      expect(view.getByDisplayValue("17900")).toBeInTheDocument();
    });
    expect(getSettings).toHaveBeenCalledTimes(2);
  });

  it("defaults appearance to follow the system and applies immediately", async () => {
    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      const appearance = view.getByLabelText(t("settings.appearance"));
      expect(
        within(appearance).getByRole("radio", { name: t("settings.appearance.system") }),
      ).toHaveAttribute("data-state", "on");
    });
    const appearance = view.getByLabelText(t("settings.appearance"));

    fireEvent.click(within(appearance).getByRole("radio", { name: t("settings.appearance.light") }));
    expect(
      within(appearance).getByRole("radio", { name: t("settings.appearance.light") }),
    ).toHaveAttribute("data-state", "on");
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    expect(saveSettings).not.toHaveBeenCalled();

    fireEvent.click(within(appearance).getByRole("radio", { name: t("settings.appearance.dark") }));
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");
    expect(saveSettings).not.toHaveBeenCalled();

    fireEvent.click(
      within(appearance).getByRole("radio", { name: t("settings.appearance.system") }),
    );
    expect(
      within(appearance).getByRole("radio", { name: t("settings.appearance.system") }),
    ).toHaveAttribute("data-state", "on");
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("system");
    expect(saveSettings).not.toHaveBeenCalled();
  });

  it("hides the menu bar item setting away from macOS", async () => {
    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() =>
      expect(view.getByLabelText(t("settings.language"))).toBeInTheDocument(),
    );
    expect(view.queryByLabelText(t("settings.tray"))).toBeNull();
  });

  it("persists the menu bar item mode on macOS", async () => {
    hostState.macos = true;
    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() =>
      expect(view.getByLabelText(t("settings.tray"))).toBeInTheDocument(),
    );
    const tray = view.getByLabelText(t("settings.tray"));
    expect(
      within(tray).getByRole("radio", { name: t("settings.trayIconAndSpeed") }),
    ).toHaveAttribute("data-state", "on");

    fireEvent.click(
      within(tray).getByRole("radio", { name: t("settings.traySpeedOnly") }),
    );
    await waitFor(
      () =>
        expect(saveSettings).toHaveBeenCalledWith(
          expect.objectContaining({ tray_display_mode: "speed" }),
        ),
      { timeout: 2000 },
    );
  });

  it("defaults language to the system locale and persists changes", async () => {
    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByLabelText(t("settings.language"))).toBeInTheDocument();
    });
    const language = view.getByLabelText(t("settings.language"));
    expect(language).toHaveValue("system");

    fireEvent.change(language, { target: { value: "en" } });
    expect(language).toHaveValue("en");
    expect(window.localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("en");
    expect(document.documentElement.lang).toBe("en");
    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({ language: "en" }),
      );
    });

    fireEvent.change(language, { target: { value: "zh" } });
    expect(language).toHaveValue("zh");
    expect(window.localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("zh");
    expect(document.documentElement.lang).toBe("zh");
  });

  it("flushes a pending language save before the panel becomes inactive", async () => {
    const { container, rerender } = render(<Settings active />);
    const view = within(container);

    const language = await view.findByLabelText(t("settings.language"));
    await waitFor(() => expect(language).not.toBeDisabled());
    fireEvent.change(language, { target: { value: "en" } });
    rerender(<Settings active={false} />);

    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({ language: "en" }),
      );
    });
  });

  it("applies the stored settings language after load", async () => {
    getSettings.mockResolvedValue({
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
      language: "en",
      check_app_updates: true,
      log_debug: false,
      tray_display_mode: "icon_and_speed",
      tun: tunSettings,
    });
    render(<Settings />);
    await waitFor(() => {
      expect(window.localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("en");
    });
    expect(document.documentElement.lang).toBe("en");
    // Rendering stays Chinese-free: the nav/labels resolve through the
    // current language, so the settings panel shows English text.
    expect(document.body.textContent).toContain(t("settings.appearance"));
    expect(document.body.textContent).toContain(t("settings.language"));

    // Reset the module-level language so later tests in this file render zh.
    persistLanguagePreference("system");
  });

  it("lets the settings panel fill the content pane", () => {
    const { container } = render(<Settings />);
    expect(within(container).getByTestId("settings-panel")).toBeInTheDocument();
    expect(within(container).getByTestId("settings-stack")).toBeInTheDocument();
  });

  it("auto-saves the TUN switch", async () => {
    getStatus.mockResolvedValue({
      ...defaultStatus,
      helper_installed: true,
    });
    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByLabelText(t("settings.tunEnable"))).toBeInTheDocument();
    });
    expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
      "data-state",
      "unchecked",
    );

    fireEvent.click(view.getByLabelText(t("settings.tunEnable")));
    expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
      "data-state",
      "checked",
    );

    await waitFor(
      () => {
        expect(saveSettings).toHaveBeenCalledWith(
          expect.objectContaining({
            tun: expect.objectContaining({ enabled: true }),
          }),
        );
      },
      { timeout: 2000 },
    );
    expect(start).not.toHaveBeenCalled();
  });

  it("rolls the TUN switch back when saving the setting fails", async () => {
    saveSettings.mockRejectedValue("disk full");
    getStatus.mockResolvedValue({
      ...defaultStatus,
      helper_installed: true,
    });
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
        "data-state",
        "unchecked",
      );
    });
    fireEvent.click(view.getByLabelText(t("settings.tunEnable")));
    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalled();
      expect(container.textContent).toContain("disk full");
    });
    expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
      "data-state",
      "unchecked",
    );
  });

  it("refreshes shared status after persisting TUN so Home sees the new desire", async () => {
    getStatus.mockResolvedValue({
      ...defaultStatus,
      helper_installed: true,
    });
    const { container } = render(
      <RuntimeStoreProvider>
        <Settings />
      </RuntimeStoreProvider>,
    );
    const view = within(container);
    await waitFor(() => {
      expect(view.getByLabelText(t("settings.tunEnable"))).toBeInTheDocument();
    });
    const statusCallsAfterLoad = getStatus.mock.calls.length;
    fireEvent.click(view.getByLabelText(t("settings.tunEnable")));
    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          tun: expect.objectContaining({ enabled: true }),
        }),
      );
    });
    await waitFor(() => {
      expect(getStatus.mock.calls.length).toBeGreaterThan(statusCallsAfterLoad);
    });
  });

  it("turning TUN off while capture is live only persists the next-start desire", async () => {
    getStatus.mockResolvedValue({
      ...defaultStatus,
      helper_installed: true,
      configured_tun: true,
      traffic_capture: "tun",
      tun_status: "enabled",
      tun_interface: "utun42",
    });
    getSettings.mockResolvedValue({
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
      language: "system",
      check_app_updates: true,
      log_debug: false,
      tray_display_mode: "icon_and_speed",
      tun: { ...tunSettings, enabled: true },
    });

    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
        "data-state",
        "checked",
      );
    });

    fireEvent.click(view.getByLabelText(t("settings.tunEnable")));
    expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
      "data-state",
      "unchecked",
    );
    await waitFor(
      () => {
        expect(saveSettings).toHaveBeenCalledWith(
          expect.objectContaining({
            tun: expect.objectContaining({ enabled: false }),
          }),
        );
      },
      { timeout: 2000 },
    );
    expect(start).not.toHaveBeenCalled();
  });

  it("prompts helper install before enabling TUN when the helper is missing", async () => {
    installHelper.mockResolvedValue(undefined);
    // After the install action the status reports the helper as authorized.
    getStatus
      .mockResolvedValue({ ...defaultStatus, helper_installed: false })
      .mockResolvedValueOnce({ ...defaultStatus, helper_installed: false })
      .mockResolvedValueOnce({ ...defaultStatus, helper_installed: true });

    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByLabelText(t("settings.tunEnable"))).toBeInTheDocument();
    });

    // First attempt: no helper -> dialog, switch stays off, nothing saved.
    fireEvent.click(view.getByLabelText(t("settings.tunEnable")));
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
      "data-state",
      "unchecked",
    );
    expect(saveSettings).not.toHaveBeenCalled();

    // Cancel: dialog closes, switch stays off, nothing saved.
    fireEvent.click(screen.getByRole("button", { name: t("common.cancel") }));
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    });
    expect(saveSettings).not.toHaveBeenCalled();
    expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
      "data-state",
      "unchecked",
    );

    // Second attempt: confirm -> install runs, then the TUN-on setting saves.
    fireEvent.click(view.getByLabelText(t("settings.tunEnable")));
    fireEvent.click(screen.getByRole("button", { name: t("tunDialog.installAndEnable") }));
    await waitFor(() => {
      expect(installHelper).toHaveBeenCalledTimes(1);
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          tun: expect.objectContaining({ enabled: true }),
        }),
      );
    });
    await waitFor(() => {
      expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
        "data-state",
        "checked",
      );
    });
  });

  it("enables TUN via the one-time scheduled-task elevation on a platform without a helper", async () => {
    // Windows (plan B): no install dialog, no UAC relaunch; the one-time
    // elevation component is installed and the next-start desire is
    // persisted. The proxy service is not started from this switch.
    ensureTunElevation.mockResolvedValue(undefined);
    getStatus.mockResolvedValue({
      ...defaultStatus,
      helper_supported: false,
      helper_installed: false,
      tun_elevation_ready: false,
    });

    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByLabelText(t("settings.tunEnable"))).toBeInTheDocument();
    });

    fireEvent.click(view.getByLabelText(t("settings.tunEnable")));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(installHelper).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(ensureTunElevation).toHaveBeenCalledTimes(1);
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          tun: expect.objectContaining({ enabled: true }),
        }),
      );
      expect(start).not.toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
        "data-state",
        "checked",
      );
    });
  });

  it("skips the one-time elevation when the scheduled task is already ready", async () => {
    ensureTunElevation.mockResolvedValue(undefined);
    getStatus.mockResolvedValue({
      ...defaultStatus,
      helper_supported: false,
      helper_installed: false,
      tun_elevation_ready: true,
    });

    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByLabelText(t("settings.tunEnable"))).toBeInTheDocument();
    });

    fireEvent.click(view.getByLabelText(t("settings.tunEnable")));
    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          tun: expect.objectContaining({ enabled: true }),
        }),
      );
    });
    expect(ensureTunElevation).not.toHaveBeenCalled();
    expect(start).not.toHaveBeenCalled();
  });

  it("reports a cancelled one-time elevation when enabling TUN", async () => {
    // Windows, setup prompt cancelled: nothing was persisted or started, the
    // error surfaces and the switch stays off.
    ensureTunElevation.mockRejectedValue("tun.elevation_cancelled: x");
    getStatus.mockResolvedValue({
      ...defaultStatus,
      helper_supported: false,
      helper_installed: false,
      tun_elevation_ready: false,
    });

    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByLabelText(t("settings.tunEnable"))).toBeInTheDocument();
    });

    fireEvent.click(view.getByLabelText(t("settings.tunEnable")));
    await waitFor(() => {
      expect(container.textContent).toContain("tun.elevation_cancelled");
    });
    expect(saveSettings).not.toHaveBeenCalled();
    expect(start).not.toHaveBeenCalled();
    expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
      "data-state",
      "unchecked",
    );
  });

  it("keeps TUN off when the guided helper install fails", async () => {
    installHelper.mockRejectedValue("tun.helper_install_failed: x");
    getStatus.mockResolvedValue({ ...defaultStatus, helper_installed: false });

    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByLabelText(t("settings.tunEnable"))).toBeInTheDocument();
    });

    fireEvent.click(view.getByLabelText(t("settings.tunEnable")));
    fireEvent.click(screen.getByRole("button", { name: t("tunDialog.installAndEnable") }));

    await waitFor(() => {
      expect(container.textContent).toContain("tun.helper_install_failed");
    });
    expect(saveSettings).not.toHaveBeenCalled();
    expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
      "data-state",
      "unchecked",
    );
  });

  it("rejects the guided install when the form has validation errors", async () => {
    installHelper.mockResolvedValue(undefined);
    // Install converges (poll sees the helper installed); the persistence
    // step then rejects the invalid form.
    getStatus
      .mockResolvedValue({ ...defaultStatus, helper_installed: false })
      .mockResolvedValueOnce({ ...defaultStatus, helper_installed: false })
      .mockResolvedValueOnce({ ...defaultStatus, helper_installed: true });

    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByLabelText(t("settings.tunEnable"))).toBeInTheDocument();
    });

    // Make the form invalid, like the auto-save guard would block.
    fireEvent.change(view.getByDisplayValue("17890"), {
      target: { value: "80" },
    });
    await waitFor(() => {
      expect(container.textContent).toContain("1024");
      expect(saveSettings).not.toHaveBeenCalled();
    });

    fireEvent.click(view.getByLabelText(t("settings.tunEnable")));
    fireEvent.click(screen.getByRole("button", { name: t("tunDialog.installAndEnable") }));

    await waitFor(() => {
      expect(installHelper).toHaveBeenCalledTimes(1);
      expect(container.textContent).toContain(t("settings.tunNotSaved"));
    });
    // No TUN-on save, no success flash, switch stays off.
    expect(saveSettings).not.toHaveBeenCalled();
    expect(container.textContent).not.toContain(t("common.saved"));
    expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
      "data-state",
      "unchecked",
    );
  });

  it("does not claim success when the helper state never converges", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      installHelper.mockResolvedValue(undefined);
      getStatus.mockResolvedValue({ ...defaultStatus, helper_installed: false });

      const { container } = render(<Settings />);
      const view = within(container);
      await waitFor(() => {
        expect(view.getByLabelText(t("settings.tunEnable"))).toBeInTheDocument();
      });

      fireEvent.click(view.getByLabelText(t("settings.tunEnable")));
      fireEvent.click(screen.getByRole("button", { name: t("tunDialog.installAndEnable") }));
      await waitFor(() => {
        expect(installHelper).toHaveBeenCalled();
      });

      // Let the status-poll window (8 x 400ms) exhaust.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(4000);
      });

      expect(container.textContent).toContain(t("settings.helperStatusUnconfirmed"));
      expect(saveSettings).not.toHaveBeenCalled();
      expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
        "data-state",
        "unchecked",
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it("disables the TUN switch and shows the reason when unavailable", async () => {
    getStatus.mockResolvedValue({
      ...defaultStatus,
      tun_available: false,
      tun_unavailable_reason: { key: "tun.unsupportedPlatform" },
    });

    const { container } = render(<Settings />);
    const view = within(container);

    // Wait for the status payload to render: the switch is also disabled
    // before the first status arrives (`!loaded`), so asserting the disabled
    // state alone would race with the mocked status resolution.
    await waitFor(() => {
      expect(container.textContent).toContain(t("tun.unsupportedPlatform"));
    });
    expect(view.getByLabelText(t("settings.tunEnable"))).toBeDisabled();
  });

  it("hides the TUN card entirely when the platform hides TUN UI", async () => {
    getStatus.mockResolvedValue({
      ...defaultStatus,
      tun_available: false,
      tun_unavailable_reason: { key: "tun.unsupportedPlatform" },
      tun_ui_hidden: true,
    });

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByLabelText(t("settings.appearance"))).toBeInTheDocument();
    });
    expect(view.queryByText(t("settings.tun"))).not.toBeInTheDocument();
    expect(
      view.queryByLabelText(t("settings.tunEnable")),
    ).not.toBeInTheDocument();
    expect(
      view.queryByRole("button", { name: t("settings.installHelper") }),
    ).not.toBeInTheDocument();
    expect(container.textContent).not.toContain(t("tun.unsupportedPlatform"));
  });

  it("disables the TUN switch while a TUN transition is in progress", async () => {
    getStatus.mockResolvedValue({
      ...defaultStatus,
      configured_tun: true,
      tun_status: "preparing",
    });

    const { container } = render(<Settings />);
    const view = within(container);

    // The switch is disabled before the first status arrives (`!loaded`);
    // wait for the transition hint to render first.
    await waitFor(() => {
      expect(container.textContent).toContain(t("settings.tunTransition.preparing"));
    });
    expect(view.getByLabelText(t("settings.tunEnable"))).toBeDisabled();
  });

  it("follows runtime-store TUN transitions after the initial load", async () => {
    let onCore = () => {};
    listenCoreStatusChanged.mockImplementation((handler: () => void) => {
      onCore = handler;
      return Promise.resolve(() => {});
    });
    getStatus.mockResolvedValue({
      ...defaultStatus,
      configured_tun: true,
      tun_status: "disabled",
    });

    const { container } = render(
      <RuntimeStoreProvider>
        <Settings />
      </RuntimeStoreProvider>,
    );
    const view = within(container);

    await waitFor(() => {
      expect(view.getByLabelText(t("settings.tunEnable"))).toBeEnabled();
    });
    await waitFor(() => {
      expect(listenCoreStatusChanged).toHaveBeenCalled();
    });

    getStatus.mockResolvedValue({
      ...defaultStatus,
      configured_tun: true,
      tun_status: "preparing",
    });
    await act(async () => {
      onCore();
    });
    await waitFor(() => {
      expect(container.textContent).toContain(t("settings.tunTransition.preparing"));
    });
    expect(view.getByLabelText(t("settings.tunEnable"))).toBeDisabled();
  });

  it("shows the active TUN interface and transition hint when capture is live", async () => {
    getStatus.mockResolvedValue({
      ...defaultStatus,
      configured_tun: true,
      traffic_capture: "tun",
      tun_status: "enabled",
      tun_interface: "utun42",
    });
    getSettings.mockResolvedValue({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: null,
      auto_set_system_proxy: false,
      proxy_service_enabled: false,
      allow_lan: false,
      proxy_mode: "rule",
      tun: { ...tunSettings, enabled: true },
    });

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(container.textContent).toContain(t("common.withIfaceLabel", { iface: "utun42" }));
    });
    expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
      "data-state",
      "checked",
    );
  });

  it("disables install and enables uninstall when the helper is installed", async () => {
    getStatus.mockResolvedValue({ ...defaultStatus, helper_installed: true });

    const { container } = render(<Settings />);
    const view = within(container);

    // The install button is also disabled before the first status arrives
    // (`!loaded`); wait for the installed-state text first.
    await waitFor(() => {
      expect(container.textContent).toContain(t("settings.helperReady"));
    });
    expect(view.getByRole("button", { name: t("settings.installHelper") })).toBeDisabled();
    expect(view.getByRole("button", { name: t("settings.uninstallHelper") })).not.toBeDisabled();
  });

  it("enables install and disables uninstall when the helper is missing", async () => {
    getStatus.mockResolvedValue({ ...defaultStatus, helper_installed: false });

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: t("settings.installHelper") })).not.toBeDisabled();
    });
    expect(view.getByRole("button", { name: t("settings.uninstallHelper") })).toBeDisabled();
  });

  it("blocks TUN and offers an update when the helper core is stale", async () => {
    installHelper.mockResolvedValue(undefined);
    getStatus.mockResolvedValue({
      ...defaultStatus,
      helper_installed: true,
      helper_stale: true,
    });

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: t("settings.updateHelper") })).toBeInTheDocument();
    });
    expect(container.textContent).toContain(t("settings.helperStale"));
    expect(view.getByLabelText(t("settings.tunEnable"))).toBeDisabled();

    fireEvent.click(view.getByRole("button", { name: t("settings.updateHelper") }));
    await waitFor(() => {
      expect(installHelper).toHaveBeenCalled();
    });
  });

  it("disables helper update/uninstall while TUN capture is active", async () => {
    getStatus.mockResolvedValue({
      ...defaultStatus,
      helper_installed: true,
      helper_stale: true,
      configured_tun: true,
      traffic_capture: "tun",
      tun_status: "enabled",
    });

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: t("settings.updateHelper") })).toBeDisabled();
    });
    expect(view.getByRole("button", { name: t("settings.uninstallHelper") })).toBeDisabled();
    expect(container.textContent).toContain(
      t("settings.tunActiveWithIface", { interface: "" }),
    );
  });

  it("installs and uninstalls the helper through the authorization dialog", async () => {
    installHelper.mockResolvedValue(undefined);
    uninstallHelper.mockResolvedValue(undefined);
    // Call order: initial load (false) -> after install action (true) ->
    // after uninstall action (false, base). The Once queue is consumed
    // first-in-first-out, so both states are queued explicitly.
    getStatus
      .mockResolvedValue({ ...defaultStatus, helper_installed: false })
      .mockResolvedValueOnce({ ...defaultStatus, helper_installed: false })
      .mockResolvedValueOnce({ ...defaultStatus, helper_installed: true });

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: t("settings.installHelper") })).toBeInTheDocument();
    });

    fireEvent.click(view.getByRole("button", { name: t("settings.installHelper") }));
    await waitFor(() => {
      expect(installHelper).toHaveBeenCalled();
      expect(view.getByRole("button", { name: t("settings.uninstallHelper") })).not.toBeDisabled();
    });

    fireEvent.click(view.getByRole("button", { name: t("settings.uninstallHelper") }));
    await waitFor(() => {
      expect(uninstallHelper).toHaveBeenCalled();
    });
    expect(container.textContent).not.toContain(t("error.tun.helper_install_failed"));
  });

  it("turns the TUN setting off when the helper is uninstalled while TUN is enabled", async () => {
    uninstallHelper.mockResolvedValue(undefined);
    getSettings.mockResolvedValue({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: null,
      auto_set_system_proxy: false,
      proxy_service_enabled: false,
      allow_lan: false,
      proxy_mode: "rule",
      tun: { ...tunSettings, enabled: true },
    });
    // Initial load reports the helper installed; after the uninstall action
    // it reports the helper gone.
    getStatus
      .mockResolvedValue({ ...defaultStatus, helper_installed: true })
      .mockResolvedValueOnce({ ...defaultStatus, helper_installed: true })
      .mockResolvedValueOnce({ ...defaultStatus, helper_installed: false });

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
        "data-state",
        "checked",
      );
    });

    fireEvent.click(view.getByRole("button", { name: t("settings.uninstallHelper") }));
    await waitFor(() => {
      expect(uninstallHelper).toHaveBeenCalled();
    });
    await waitFor(
      () => {
        expect(saveSettings).toHaveBeenCalledWith(
          expect.objectContaining({
            tun: expect.objectContaining({ enabled: false }),
          }),
        );
      },
      { timeout: 2000 },
    );
    expect(view.getByLabelText(t("settings.tunEnable"))).toHaveAttribute(
      "data-state",
      "unchecked",
    );
  });

  it("surfaces a helper install failure without claiming success", async () => {
    installHelper.mockRejectedValue("tun.helper_install_failed: x");

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: t("settings.installHelper") })).toBeInTheDocument();
    });
    fireEvent.click(view.getByRole("button", { name: t("settings.installHelper") }));

    await waitFor(() => {
      expect(container.textContent).toContain("tun.helper_install_failed");
    });
  });

  it("checks for app updates from the settings card", async () => {
    checkAppUpdate.mockResolvedValue({
      available: true,
      version: "0.1.6",
      notes: "fixes",
      skipped: false,
      should_prompt: false,
    });
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByRole("button", { name: t("settings.updateCheck") })).toBeEnabled();
    });
    fireEvent.click(view.getByRole("button", { name: t("settings.updateCheck") }));
    await waitFor(() => {
      expect(checkAppUpdate).toHaveBeenCalledWith(false);
      expect(container.textContent).toContain(t("settings.updateAvailable", { version: "0.1.6" }));
    });
    expect(view.getByRole("button", { name: t("settings.updateInstall") })).toBeInTheDocument();
    fireEvent.click(view.getByRole("button", { name: t("settings.updateInstall") }));
    await waitFor(() => {
      expect(installAppUpdate).toHaveBeenCalled();
    });
  });

  it("still offers install after a check that marks the version skipped", async () => {
    checkAppUpdate.mockResolvedValue({
      available: true,
      version: "0.1.6",
      notes: null,
      skipped: true,
      should_prompt: false,
    });
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByRole("button", { name: t("settings.updateCheck") })).toBeEnabled();
    });
    fireEvent.click(view.getByRole("button", { name: t("settings.updateCheck") }));
    await waitFor(() => {
      expect(container.textContent).toContain(t("settings.updateAvailable", { version: "0.1.6" }));
    });
    expect(view.getByRole("button", { name: t("settings.updateInstall") })).toBeInTheDocument();
  });

  it("shows install next to check when a parent reported an available update", async () => {
    const { container } = render(
      <Settings
        availableUpdate={{
          available: true,
          version: "0.1.6",
          notes: "fixes",
          skipped: false,
          should_prompt: false,
        }}
      />,
    );
    const view = within(container);
    await waitFor(() => {
      expect(view.getByRole("button", { name: t("settings.updateCheck") })).toBeEnabled();
    });
    expect(container.textContent).toContain(t("settings.updateAvailable", { version: "0.1.6" }));
    expect(view.getByRole("button", { name: t("settings.updateInstall") })).toBeInTheDocument();
  });

  it("explains a missing GitHub update catalog from a structured IPC payload", async () => {
    checkAppUpdate.mockRejectedValue({
      code: "update.feed_unavailable",
      message: "Could not fetch a valid release JSON from the remote",
    });
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByRole("button", { name: t("settings.updateCheck") })).toBeEnabled();
    });
    fireEvent.click(view.getByRole("button", { name: t("settings.updateCheck") }));
    await waitFor(() => {
      expect(container.textContent).toContain(t("settings.updateFeedUnavailable"));
    });
    expect(container.textContent).not.toContain(t("settings.updateCheckFailed"));
    expect(container.textContent).not.toContain("[object Object]");
  });

  it("explains a missing GitHub update catalog instead of blaming the proxy", async () => {
    checkAppUpdate.mockRejectedValue(
      "update.feed_unavailable: Could not fetch a valid release JSON from the remote",
    );
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByRole("button", { name: t("settings.updateCheck") })).toBeEnabled();
    });
    fireEvent.click(view.getByRole("button", { name: t("settings.updateCheck") }));
    await waitFor(() => {
      expect(container.textContent).toContain(t("settings.updateFeedUnavailable"));
    });
    expect(container.textContent).not.toContain(t("settings.updateCheckFailed"));
  });

  it("does not treat a mention of an update code as the error code", async () => {
    checkAppUpdate.mockRejectedValue(
      "network error (docs mention update.feed_unavailable)",
    );
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByRole("button", { name: t("settings.updateCheck") })).toBeEnabled();
    });
    fireEvent.click(view.getByRole("button", { name: t("settings.updateCheck") }));
    await waitFor(() => {
      expect(container.textContent).toContain("network error");
    });
    expect(container.textContent).not.toContain(t("settings.updateFeedUnavailable"));
  });

  it("keeps the proxy hint for a real update check network failure", async () => {
    checkAppUpdate.mockRejectedValue(
      "update.check_failed: error sending request for url",
    );
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByRole("button", { name: t("settings.updateCheck") })).toBeEnabled();
    });
    fireEvent.click(view.getByRole("button", { name: t("settings.updateCheck") }));
    await waitFor(() => {
      expect(container.textContent).toContain(t("settings.updateCheckFailed"));
    });
  });

  it("persists turning off automatic update checks", async () => {
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByLabelText(t("settings.updateAutoCheck"))).toBeInTheDocument();
    });
    fireEvent.click(view.getByLabelText(t("settings.updateAutoCheck")));
    await waitFor(
      () => {
        expect(saveSettings).toHaveBeenCalledWith(
          expect.objectContaining({ check_app_updates: false }),
        );
      },
      { timeout: 2000 },
    );
  });

  it("clears the sidebar upgrade when automatic checks are turned off", async () => {
    const onAvailableUpdate = vi.fn();
    const available = {
      available: true,
      version: "0.1.6",
      notes: "fixes",
      skipped: false,
      should_prompt: false,
    };
    const { container } = render(
      <Settings
        availableUpdate={available}
        onAvailableUpdate={onAvailableUpdate}
      />,
    );
    const view = within(container);
    await waitFor(() => {
      expect(view.getByLabelText(t("settings.updateAutoCheck"))).toBeInTheDocument();
    });
    expect(view.getByRole("button", { name: t("settings.updateInstall") })).toBeInTheDocument();
    fireEvent.click(view.getByLabelText(t("settings.updateAutoCheck")));
    expect(onAvailableUpdate).toHaveBeenCalledWith(null);
    expect(view.getByRole("button", { name: t("settings.updateInstall") })).toBeInTheDocument();
    expect(container.textContent).toContain(t("settings.updateAvailable", { version: "0.1.6" }));
    fireEvent.click(view.getByLabelText(t("settings.updateAutoCheck")));
    expect(onAvailableUpdate).toHaveBeenLastCalledWith(available);
  });

  it("does not restore the sidebar after a manual check while auto-check is off", async () => {
    getSettings.mockResolvedValue({
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
      language: "system",
      check_app_updates: false,
      log_debug: false,
      tray_display_mode: "icon_and_speed",
      tun: tunSettings,
    });
    checkAppUpdate.mockResolvedValue({
      available: true,
      version: "0.1.6",
      notes: "fixes",
      skipped: false,
      should_prompt: false,
    });
    const onAvailableUpdate = vi.fn();
    const { container } = render(
      <Settings onAvailableUpdate={onAvailableUpdate} />,
    );
    const view = within(container);
    await waitFor(() => {
      expect(view.getByRole("button", { name: t("settings.updateCheck") })).toBeEnabled();
    });
    fireEvent.click(view.getByRole("button", { name: t("settings.updateCheck") }));
    await waitFor(() => {
      expect(container.textContent).toContain(t("settings.updateAvailable", { version: "0.1.6" }));
    });
    expect(onAvailableUpdate).toHaveBeenCalledWith(null);
    expect(view.getByRole("button", { name: t("settings.updateInstall") })).toBeInTheDocument();
  });
});
