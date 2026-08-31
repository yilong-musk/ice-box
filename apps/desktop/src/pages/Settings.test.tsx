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
import { Settings } from "./Settings";

const getSettings = vi.fn();
const getStatus = vi.fn();
const saveSettings = vi.fn();
const installHelper = vi.fn();
const uninstallHelper = vi.fn();

vi.mock("../api/tauri", () => ({
  api: {
    getSettings: (...args: unknown[]) => getSettings(...args),
    getStatus: (...args: unknown[]) => getStatus(...args),
    saveSettings: (...args: unknown[]) => saveSettings(...args),
    installHelper: (...args: unknown[]) => installHelper(...args),
    uninstallHelper: (...args: unknown[]) => uninstallHelper(...args),
    revealDataDir: vi.fn(),
  },
  formatInvokeError: (err: unknown) => String(err),
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
  helper_stale: false,
} as const;

describe("Settings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.localStorage.removeItem(THEME_STORAGE_KEY);
    document.documentElement.classList.remove("dark");
    getSettings.mockResolvedValue({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: null,
      auto_set_system_proxy: false,
      allow_lan: false,
      proxy_mode: "rule",
      auto_default_rules: true,
      tun: tunSettings,
    });
    getStatus.mockResolvedValue({ ...defaultStatus });
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
      expect(view.getByLabelText("允许局域网共享（Allow LAN）")).toBeInTheDocument();
    });

    fireEvent.click(view.getByLabelText("允许局域网共享（Allow LAN）"));

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
        view.getByLabelText("为无规则的订阅附加默认分流规则"),
      ).toBeInTheDocument();
    });

    const toggle = view.getByLabelText("为无规则的订阅附加默认分流规则");
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
        expect(container.textContent).toContain("不能相同");
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

    const mixedInput = await view.findByLabelText("Mixed 监听");
    expect(mixedInput).toBeDisabled();

    resolveSettings({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: null,
      auto_set_system_proxy: false,
      allow_lan: false,
      proxy_mode: "rule",
      tun: tunSettings,
    });

    await waitFor(() => {
      expect(mixedInput).not.toBeDisabled();
    });
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

    const mixedInput = view.getByLabelText("Mixed 监听");
    expect(mixedInput).toBeDisabled();
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
      allow_lan: false,
      proxy_mode: "rule" as const,
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
    expect(view.getByLabelText("Mixed 监听")).toBeDisabled();
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
      expect(view.getByRole("radio", { name: "跟随系统" })).toHaveAttribute(
        "data-state",
        "on",
      );
    });

    fireEvent.click(view.getByRole("radio", { name: "浅色" }));
    expect(view.getByRole("radio", { name: "浅色" })).toHaveAttribute(
      "data-state",
      "on",
    );
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    expect(saveSettings).not.toHaveBeenCalled();

    fireEvent.click(view.getByRole("radio", { name: "深色" }));
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");
    expect(saveSettings).not.toHaveBeenCalled();

    fireEvent.click(view.getByRole("radio", { name: "跟随系统" }));
    expect(view.getByRole("radio", { name: "跟随系统" })).toHaveAttribute(
      "data-state",
      "on",
    );
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("system");
    expect(saveSettings).not.toHaveBeenCalled();
  });

  it("lets the settings panel fill the content pane", () => {
    const { container } = render(<Settings />);
    expect(
      container.querySelector(".settings-panel")?.className.split(/\s+/),
    ).toEqual(expect.arrayContaining(["flex-1", "min-h-0", "flex-col"]));
    const card = container.querySelector("[data-slot=card]");
    expect(card).not.toBeNull();
    const classes = card!.className.split(/\s+/);
    expect(classes).toContain("w-full");
    expect(classes).not.toContain("max-w-lg");
  });

  it("auto-saves the TUN switch", async () => {
    getStatus.mockResolvedValue({
      ...defaultStatus,
      helper_installed: true,
    });
    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByLabelText("启用 TUN 模式")).toBeInTheDocument();
    });
    expect(view.getByLabelText("启用 TUN 模式")).toHaveAttribute(
      "data-state",
      "unchecked",
    );

    fireEvent.click(view.getByLabelText("启用 TUN 模式"));
    expect(view.getByLabelText("启用 TUN 模式")).toHaveAttribute(
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
      expect(view.getByLabelText("启用 TUN 模式")).toBeInTheDocument();
    });

    // First attempt: no helper -> dialog, switch stays off, nothing saved.
    fireEvent.click(view.getByLabelText("启用 TUN 模式"));
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    expect(view.getByLabelText("启用 TUN 模式")).toHaveAttribute(
      "data-state",
      "unchecked",
    );
    expect(saveSettings).not.toHaveBeenCalled();

    // Cancel: dialog closes, switch stays off, nothing saved.
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    });
    expect(saveSettings).not.toHaveBeenCalled();
    expect(view.getByLabelText("启用 TUN 模式")).toHaveAttribute(
      "data-state",
      "unchecked",
    );

    // Second attempt: confirm -> install runs, then the TUN-on setting saves.
    fireEvent.click(view.getByLabelText("启用 TUN 模式"));
    fireEvent.click(screen.getByRole("button", { name: "安装并启用" }));
    await waitFor(() => {
      expect(installHelper).toHaveBeenCalledTimes(1);
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          tun: expect.objectContaining({ enabled: true }),
        }),
      );
    });
    await waitFor(() => {
      expect(view.getByLabelText("启用 TUN 模式")).toHaveAttribute(
        "data-state",
        "checked",
      );
    });
  });

  it("keeps TUN off when the guided helper install fails", async () => {
    installHelper.mockRejectedValue("tun.helper_install_failed: x");
    getStatus.mockResolvedValue({ ...defaultStatus, helper_installed: false });

    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByLabelText("启用 TUN 模式")).toBeInTheDocument();
    });

    fireEvent.click(view.getByLabelText("启用 TUN 模式"));
    fireEvent.click(screen.getByRole("button", { name: "安装并启用" }));

    await waitFor(() => {
      expect(container.textContent).toContain("tun.helper_install_failed");
    });
    expect(saveSettings).not.toHaveBeenCalled();
    expect(view.getByLabelText("启用 TUN 模式")).toHaveAttribute(
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
      expect(view.getByLabelText("启用 TUN 模式")).toBeInTheDocument();
    });

    // Make the form invalid, like the auto-save guard would block.
    fireEvent.change(view.getByDisplayValue("17890"), {
      target: { value: "80" },
    });
    await waitFor(() => {
      expect(container.textContent).toContain("1024");
      expect(saveSettings).not.toHaveBeenCalled();
    });

    fireEvent.click(view.getByLabelText("启用 TUN 模式"));
    fireEvent.click(screen.getByRole("button", { name: "安装并启用" }));

    await waitFor(() => {
      expect(installHelper).toHaveBeenCalledTimes(1);
      expect(container.textContent).toContain("TUN 设置未保存");
    });
    // No TUN-on save, no success flash, switch stays off.
    expect(saveSettings).not.toHaveBeenCalled();
    expect(container.textContent).not.toContain("已保存");
    expect(view.getByLabelText("启用 TUN 模式")).toHaveAttribute(
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
        expect(view.getByLabelText("启用 TUN 模式")).toBeInTheDocument();
      });

      fireEvent.click(view.getByLabelText("启用 TUN 模式"));
      fireEvent.click(screen.getByRole("button", { name: "安装并启用" }));
      await waitFor(() => {
        expect(installHelper).toHaveBeenCalled();
      });

      // Let the status-poll window (8 x 400ms) exhaust.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(4000);
      });

      expect(container.textContent).toContain("辅助组件状态未确认");
      expect(saveSettings).not.toHaveBeenCalled();
      expect(view.getByLabelText("启用 TUN 模式")).toHaveAttribute(
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
      tun_unavailable_reason: "Windows TUN gate pending",
    });

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByLabelText("启用 TUN 模式")).toBeDisabled();
    });
    expect(container.textContent).toContain("Windows TUN gate pending");
  });

  it("hides the TUN card entirely when the platform hides TUN UI", async () => {
    getStatus.mockResolvedValue({
      ...defaultStatus,
      tun_available: false,
      tun_unavailable_reason: "Windows TUN gate pending",
      tun_ui_hidden: true,
    });

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByLabelText("外观")).toBeInTheDocument();
    });
    expect(view.queryByText("TUN 模式")).not.toBeInTheDocument();
    expect(
      view.queryByLabelText("启用 TUN 模式"),
    ).not.toBeInTheDocument();
    expect(
      view.queryByRole("button", { name: "安装辅助组件" }),
    ).not.toBeInTheDocument();
    expect(container.textContent).not.toContain("Windows TUN gate pending");
  });

  it("disables the TUN switch while a TUN transition is in progress", async () => {
    getStatus.mockResolvedValue({
      ...defaultStatus,
      configured_tun: true,
      tun_status: "preparing",
    });

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByLabelText("启用 TUN 模式")).toBeDisabled();
    });
    expect(container.textContent).toContain("正在启用 TUN…");
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
      allow_lan: false,
      proxy_mode: "rule",
      tun: { ...tunSettings, enabled: true },
    });

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(container.textContent).toContain("接口 utun42");
    });
    expect(view.getByLabelText("启用 TUN 模式")).toHaveAttribute(
      "data-state",
      "checked",
    );
  });

  it("disables install and enables uninstall when the helper is installed", async () => {
    getStatus.mockResolvedValue({ ...defaultStatus, helper_installed: true });

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "安装辅助组件" })).toBeDisabled();
    });
    expect(view.getByRole("button", { name: "卸载辅助组件" })).not.toBeDisabled();
    expect(container.textContent).toContain("辅助组件已安装并授权");
  });

  it("enables install and disables uninstall when the helper is missing", async () => {
    getStatus.mockResolvedValue({ ...defaultStatus, helper_installed: false });

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "安装辅助组件" })).not.toBeDisabled();
    });
    expect(view.getByRole("button", { name: "卸载辅助组件" })).toBeDisabled();
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
      expect(view.getByRole("button", { name: "更新辅助组件" })).toBeInTheDocument();
    });
    expect(container.textContent).toContain("仍在运行旧版内核");
    expect(view.getByLabelText("启用 TUN 模式")).toBeDisabled();

    fireEvent.click(view.getByRole("button", { name: "更新辅助组件" }));
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
      expect(view.getByRole("button", { name: "更新辅助组件" })).toBeDisabled();
    });
    expect(view.getByRole("button", { name: "卸载辅助组件" })).toBeDisabled();
    expect(container.textContent).toContain("当前通过 TUN 接管流量");
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
      expect(view.getByRole("button", { name: "安装辅助组件" })).toBeInTheDocument();
    });

    fireEvent.click(view.getByRole("button", { name: "安装辅助组件" }));
    await waitFor(() => {
      expect(installHelper).toHaveBeenCalled();
      expect(view.getByRole("button", { name: "卸载辅助组件" })).not.toBeDisabled();
    });

    fireEvent.click(view.getByRole("button", { name: "卸载辅助组件" }));
    await waitFor(() => {
      expect(uninstallHelper).toHaveBeenCalled();
    });
    expect(container.textContent).not.toContain("辅助组件安装失败");
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
      expect(view.getByLabelText("启用 TUN 模式")).toHaveAttribute(
        "data-state",
        "checked",
      );
    });

    fireEvent.click(view.getByRole("button", { name: "卸载辅助组件" }));
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
    expect(view.getByLabelText("启用 TUN 模式")).toHaveAttribute(
      "data-state",
      "unchecked",
    );
  });

  it("surfaces a helper install failure without claiming success", async () => {
    installHelper.mockRejectedValue("tun.helper_install_failed: x");

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "安装辅助组件" })).toBeInTheDocument();
    });
    fireEvent.click(view.getByRole("button", { name: "安装辅助组件" }));

    await waitFor(() => {
      expect(container.textContent).toContain("tun.helper_install_failed");
    });
  });
});
