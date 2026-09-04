import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearNodesSnapshot, readNodesSnapshot } from "../lib/nodes";
import { Home } from "./Home";

const getStatus = vi.fn();
const listNodes = vi.fn();
const getSettings = vi.fn();
const getTrafficSnapshot = vi.fn();
const setProxyMode = vi.fn();
const start = vi.fn();
const stopSystemProxy = vi.fn();
const saveSettings = vi.fn();
const recoverTun = vi.fn();
const installHelper = vi.fn();
const relaunchElevatedForTun = vi.fn();
const ensureTunElevation = vi.fn();

vi.mock("../api/tauri", () => ({
  api: {
    getStatus: (...args: unknown[]) => getStatus(...args),
    listNodes: (...args: unknown[]) => listNodes(...args),
    getSettings: (...args: unknown[]) => getSettings(...args),
    getTrafficSnapshot: (...args: unknown[]) => getTrafficSnapshot(...args),
    setProxyMode: (...args: unknown[]) => setProxyMode(...args),
    start: (...args: unknown[]) => start(...args),
    stopSystemProxy: (...args: unknown[]) => stopSystemProxy(...args),
    saveSettings: (...args: unknown[]) => saveSettings(...args),
    recoverTun: (...args: unknown[]) => recoverTun(...args),
    installHelper: (...args: unknown[]) => installHelper(...args),
    relaunchElevatedForTun: (...args: unknown[]) =>
      relaunchElevatedForTun(...args),
    ensureTunElevation: (...args: unknown[]) => ensureTunElevation(...args),
    stop: vi.fn(),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

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

describe("Home", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    clearNodesSnapshot();
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
    listNodes.mockResolvedValue([]);
    getSettings.mockResolvedValue({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: null,
      auto_set_system_proxy: true,
      allow_lan: false,
      proxy_mode: "rule",
      tun: tunSettings,
    });
    getTrafficSnapshot.mockResolvedValue({ points: [], latest: null, peak: null });
    start.mockResolvedValue(undefined);
    stopSystemProxy.mockResolvedValue(undefined);
    saveSettings.mockResolvedValue(undefined);
    recoverTun.mockResolvedValue(null);
  });

  it("shows current outbound in the status list", async () => {
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: true,
      system_proxy_recorded: true,
      system_proxy_available: true,
      ...tunStatus,
    });
    listNodes.mockResolvedValue([
      {
        tag: "Proxies",
        outbound_type: "selector",
        group_now: "HK-1",
        group_all: ["HK-1", "JP-1"],
      },
    ]);
    getSettings.mockResolvedValue({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: "Proxies",
      auto_set_system_proxy: true,
      allow_lan: false,
      proxy_mode: "rule",
    });

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByText("当前出站")).toBeInTheDocument();
    });
    expect(view.getByText("Proxies → HK-1")).toBeInTheDocument();
    expect(view.getByText("流量")).toBeInTheDocument();
    expect(view.getByText("代理状态")).toBeInTheDocument();
    expect(view.getByText("信息")).toBeInTheDocument();
    expect(view.queryByText("系统代理")).toBeNull();
    expect(container.querySelector(".home-panel")?.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["flex-1", "min-h-0", "flex-col"]),
    );
    // The mode switch lives inside the 代理状态 card, below the power button.
    const statusCard = view
      .getByText("代理状态")
      .closest("[data-slot=card]") as HTMLElement;
    expect(
      within(statusCard).getByRole("radiogroup", { name: "模式" }),
    ).toBeInTheDocument();
    expect(view.getByRole("radio", { name: "规则" })).toHaveAttribute("data-state", "on");
    expect(view.getByRole("radio", { name: "全局" })).toHaveAttribute("data-state", "off");
    expect(view.getByRole("radio", { name: "直连" })).toHaveAttribute("data-state", "off");
    expect(
      within(statusCard).getByRole("button", { name: "停止代理服务" }),
    ).toBeInTheDocument();
    expect(view.queryByRole("button", { name: "测延迟" })).toBeNull();
  });

  it("ignores a poll response that finishes after the pane is deactivated", async () => {
    let resolveNodes: (value: unknown[]) => void = () => {};
    listNodes.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveNodes = resolve;
        }),
    );
    const { rerender } = render(<Home active />);

    await waitFor(() => {
      expect(listNodes).toHaveBeenCalled();
    });
    rerender(<Home active={false} />);

    await act(async () => {
      resolveNodes([
        {
          tag: "stale-node",
          outbound_type: "trojan",
          group_now: null,
          group_all: null,
        },
      ]);
      await Promise.resolve();
    });

    expect(readNodesSnapshot()).toBeUndefined();
  });

  it("switches proxy mode and reverts when it fails", async () => {
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: true,
      system_proxy_recorded: true,
      system_proxy_available: true,
      ...tunStatus,
    });
    listNodes.mockResolvedValue([
      { tag: "node-a", outbound_type: "socks", group_now: null, group_all: null },
    ]);
    let currentMode = "rule";
    getSettings.mockImplementation(() =>
      Promise.resolve({
        mixed_listen: "127.0.0.1",
        mixed_port: 17890,
        clash_api_listen: "127.0.0.1",
        clash_api_port: 19090,
        selected_tag: "node-a",
        auto_set_system_proxy: true,
        allow_lan: false,
        proxy_mode: currentMode,
      }),
    );
    setProxyMode.mockImplementation((mode: string) => {
      currentMode = mode;
      return Promise.resolve();
    });

    const { container } = render(<Home />);
    const view = within(container);

    const globalButton = () => view.getByRole("radio", { name: "全局" });
    const directButton = () => view.getByRole("radio", { name: "直连" });

    await waitFor(() => {
      expect(globalButton()).toBeInTheDocument();
    });
    expect(globalButton()).toHaveAttribute("data-state", "off");

    fireEvent.click(globalButton());
    await waitFor(() => {
      expect(setProxyMode).toHaveBeenCalledWith("global");
    });
    await waitFor(() => {
      expect(globalButton()).toHaveAttribute("data-state", "on");
    });

    setProxyMode.mockRejectedValue("mode switch failed");
    fireEvent.click(directButton());
    await waitFor(() => {
      expect(view.getByText("mode switch failed")).toBeInTheDocument();
    });
    await waitFor(() => {
      expect(globalButton()).toHaveAttribute("data-state", "on");
      expect(directButton()).toHaveAttribute("data-state", "off");
    });
  });

  it("shows empty-state guide and system-proxy toggle when no nodes", async () => {
    const onNavigate = vi.fn();
    const { container } = render(<Home onNavigate={onNavigate} />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByText("还没有可用节点")).toBeInTheDocument();
    });
    expect(view.getByText("当前出站").parentElement).toHaveTextContent(/当前出站\s*—/);
    expect(view.getByText("流量")).toBeInTheDocument();
    const power = view.getByRole("button", { name: "启动代理服务" });
    expect(power).not.toBeDisabled();
    expect(power).toHaveAttribute("aria-pressed", "false");
    expect(view.queryByRole("button", { name: "停止代理服务" })).toBeNull();
    fireEvent.click(view.getByRole("button", { name: "前往订阅页导入" }));
    expect(onNavigate).toHaveBeenCalledWith("subs");
  });

  it("hides proxy-off hint when core running but proxy simply not started", async () => {
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: false,
      system_proxy_recorded: false,
      system_proxy_available: true,
      ...tunStatus,
    });
    listNodes.mockResolvedValue([
      { tag: "node-a", outbound_type: "socks", group_now: null, group_all: null },
    ]);

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "启动代理服务" })).toBeInTheDocument();
    });
    expect(view.queryByText("系统代理未接管或已不同步")).toBeNull();
    const power = view.getByRole("button", { name: "启动代理服务" });
    expect(power).not.toBeDisabled();
    expect(power).toHaveAttribute("aria-pressed", "false");
  });

  it("shows desync hint when recorded but live check is false", async () => {
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: false,
      system_proxy_recorded: true,
      system_proxy_available: true,
      ...tunStatus,
    });
    listNodes.mockResolvedValue([
      { tag: "node-a", outbound_type: "socks", group_now: null, group_all: null },
    ]);

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("alert")).toHaveTextContent(
        "系统代理未接管或已不同步",
      );
    });
  });

  it("hides system-proxy controls when backend is unavailable", async () => {
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 0,
      proxy_recovery_warning: null,
      system_proxy_applied: null,
      system_proxy_recorded: null,
      system_proxy_available: false,
      ...tunStatus,
      tun_available: false,
    });

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByText("当前平台不支持系统代理或 TUN 接管")).toBeInTheDocument();
    });
    expect(view.queryByRole("button", { name: "启动代理服务" })).toBeNull();
    expect(view.queryByRole("button", { name: "停止代理服务" })).toBeNull();
  });

  it("shows stop on the power toggle when disk recorded but live check is false", async () => {
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: false,
      system_proxy_recorded: true,
      system_proxy_available: true,
      ...tunStatus,
    });
    listNodes.mockResolvedValue([
      { tag: "node-a", outbound_type: "socks", group_now: null, group_all: null },
    ]);

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "停止代理服务" })).not.toBeDisabled();
    });
    const power = view.getByRole("button", { name: "停止代理服务" });
    expect(power).toHaveAttribute("aria-pressed", "true");
    expect(view.queryByRole("button", { name: "启动代理服务" })).toBeNull();
    expect(view.getByRole("alert")).toHaveTextContent("系统代理未接管或已不同步");
    expect(view.queryByText("系统代理")).toBeNull();
  });

  it("does not flash poll errors while power toggle start is pending", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
    let resolveStart: (() => void) | undefined;
    start.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          resolveStart = resolve;
        }),
    );

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "启动代理服务" })).not.toBeDisabled();
    });

    fireEvent.click(view.getByRole("button", { name: "启动代理服务" }));
    await waitFor(() => {
      expect(start).toHaveBeenCalled();
    });

    getStatus.mockRejectedValue("clash api not ready");
    listNodes.mockRejectedValue("clash api not ready");
    getSettings.mockRejectedValue("clash api not ready");

    const statusCalls = getStatus.mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });

    expect(getStatus.mock.calls.length).toBe(statusCalls);
    expect(view.queryByText(/clash api not ready/i)).toBeNull();
    expect(container.querySelector(".error")).toBeNull();

    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 0,
      proxy_recovery_warning: null,
      system_proxy_applied: true,
      system_proxy_recorded: true,
      system_proxy_available: true,
      ...tunStatus,
    });
    listNodes.mockResolvedValue([]);
    getSettings.mockResolvedValue({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: null,
      auto_set_system_proxy: true,
      allow_lan: false,
      proxy_mode: "rule",
      tun: tunSettings,
    });

    resolveStart?.();
    await waitFor(() => {
      expect(start).toHaveBeenCalled();
      expect(view.queryByText(/clash api not ready/i)).toBeNull();
    });
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows TUN active state and interface on the power control", async () => {
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: false,
      system_proxy_recorded: false,
      system_proxy_available: true,
      ...tunStatus,
      configured_tun: true,
      traffic_capture: "tun",
      tun_status: "enabled",
      tun_interface: "utun42",
    });
    listNodes.mockResolvedValue([
      { tag: "node-a", outbound_type: "socks", group_now: null, group_all: null },
    ]);
    getSettings.mockResolvedValue({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: "node-a",
      auto_set_system_proxy: true,
      allow_lan: false,
      proxy_mode: "rule",
      tun: { ...tunSettings, enabled: true },
    });

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "停止代理服务" })).not.toBeDisabled();
    });
    const power = view.getByRole("button", { name: "停止代理服务" });
    expect(power).toHaveAttribute("aria-pressed", "true");
    expect(power).toHaveTextContent("TUN 已接管（utun42）");
    expect(view.getByText("捕获")).toBeInTheDocument();
    expect(view.getByText("TUN（utun42）")).toBeInTheDocument();
  });

  it("shows TUN-configured subtitle when service is off", async () => {
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: false,
      system_proxy_recorded: false,
      system_proxy_available: true,
      ...tunStatus,
      configured_tun: true,
    });

    const { container } = render(<Home />);
    const view = within(container);

    // The power button is enabled before the first status arrives; wait for
    // the status-dependent subtitle so the assertion cannot race the mocked
    // status resolution.
    await waitFor(() => {
      expect(view.getByRole("button", { name: "启动代理服务" })).toHaveTextContent(
        "将启用 TUN 模式接管流量",
      );
    });
  });

  it("disables the power control and shows the reason when TUN is unavailable", async () => {
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: false,
      system_proxy_recorded: false,
      system_proxy_available: true,
      ...tunStatus,
      configured_tun: true,
      tun_available: false,
      tun_unavailable_reason: "Windows TUN gate pending",
    });

    const { container } = render(<Home />);
    const view = within(container);

    // The power button renders before the first status arrives; wait for the
    // status-dependent reason text so the disabled assertion below cannot
    // race the mocked status resolution.
    await waitFor(() => {
      expect(view.getByText("Windows TUN gate pending")).toBeInTheDocument();
    });
    const power = view.getByRole("button", { name: "启动代理服务" });
    expect(power).toBeDisabled();
  });

  it("hides the TUN toggle and ignores TUN state when the platform hides TUN UI", async () => {
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: false,
      system_proxy_recorded: false,
      system_proxy_available: true,
      ...tunStatus,
      configured_tun: true,
      tun_available: false,
      tun_unavailable_reason: "Windows TUN gate pending",
      tun_ui_hidden: true,
    });

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "启动代理服务" })).toBeInTheDocument();
    });
    // No TUN switch on the home page, no TUN reason text anywhere.
    expect(view.queryByRole("button", { name: "TUN 模式" })).not.toBeInTheDocument();
    expect(container.textContent).not.toContain("Windows TUN gate pending");
    // The power control ignores the hidden TUN desire: system proxy stays usable.
    expect(view.getByRole("button", { name: "启动代理服务" })).not.toBeDisabled();
    expect(view.getByRole("button", { name: "启动代理服务" })).toHaveTextContent(
      "点击接管系统代理",
    );
  });

  it("shows permission-required state with a system-proxy fallback action", async () => {
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: false,
      system_proxy_recorded: false,
      system_proxy_available: true,
      ...tunStatus,
      configured_tun: true,
      tun_status: "permission_required",
      tun_error: { code: "tun.permission_required", message: "sudo" },
    });
    getSettings.mockResolvedValue({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: null,
      auto_set_system_proxy: true,
      allow_lan: false,
      proxy_mode: "rule",
      tun: { ...tunSettings, enabled: true },
    });

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("alert")).toHaveTextContent("启用 TUN 需要系统权限");
    });
    installHelper.mockResolvedValue(undefined);
    start.mockResolvedValue(undefined);
    fireEvent.click(view.getByRole("button", { name: "安装辅助组件" }));
    await waitFor(() => {
      expect(installHelper).toHaveBeenCalled();
      expect(start).toHaveBeenCalled();
    });
    fireEvent.click(view.getByRole("button", { name: "停用 TUN，改用系统代理" }));
    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          tun: expect.objectContaining({ enabled: false }),
        }),
      );
      expect(start).toHaveBeenCalled();
    });
  });

  it("shows recovery-required state with a recovery action, no fallback", async () => {
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: false,
      system_proxy_recorded: false,
      system_proxy_available: true,
      ...tunStatus,
      configured_tun: true,
      tun_status: "recovery_required",
      tun_error: { code: "tun.recovery_required", message: "cleanup" },
    });

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("alert")).toHaveTextContent("TUN 清理未确认");
    });
    expect(
      view.queryByRole("button", { name: "停用 TUN，改用系统代理" }),
    ).toBeNull();
    fireEvent.click(view.getByRole("button", { name: "重试恢复" }));
    await waitFor(() => {
      expect(recoverTun).toHaveBeenCalled();
    });
  });

  it("toggles the TUN setting from the home page when the helper is installed", async () => {
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: false,
      system_proxy_recorded: false,
      system_proxy_available: true,
      ...tunStatus,
      configured_tun: false,
      helper_installed: true,
    });
    getSettings.mockResolvedValue({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: null,
      auto_set_system_proxy: true,
      allow_lan: false,
      proxy_mode: "rule",
      tun: tunSettings,
    });

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "TUN 模式" })).toBeInTheDocument();
    });
    const tunToggle = view.getByRole("button", { name: "TUN 模式" });
    expect(tunToggle).toHaveAttribute("aria-pressed", "false");

    fireEvent.click(tunToggle);
    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          tun: expect.objectContaining({ enabled: true }),
        }),
      );
    });
  });

  it("reflects the TUN toggle optimistically and snaps back when the save is not committed", async () => {
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: false,
      system_proxy_recorded: false,
      system_proxy_available: true,
      ...tunStatus,
      configured_tun: false,
      helper_installed: true,
    });

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "TUN 模式" })).toBeInTheDocument();
    });
    const tunToggle = view.getByRole("button", { name: "TUN 模式" });
    expect(tunToggle).toHaveAttribute("aria-pressed", "false");

    // The toggle reflects the intent immediately instead of waiting for the
    // next 2s status poll.
    fireEvent.click(tunToggle);
    expect(tunToggle).toHaveAttribute("aria-pressed", "true");
    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          tun: expect.objectContaining({ enabled: true }),
        }),
      );
    });

    // The post-action refresh reports the committed value; the backend still
    // says TUN is not configured, so the toggle snaps back.
    await waitFor(() => {
      expect(tunToggle).toHaveAttribute("aria-pressed", "false");
    });
  });

  it("enables TUN via the one-time scheduled-task elevation on the home page", async () => {
    getSettings.mockResolvedValue({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: null,
      auto_set_system_proxy: true,
      allow_lan: false,
      proxy_mode: "rule",
      tun: tunSettings,
    });
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: false,
      system_proxy_recorded: false,
      system_proxy_available: true,
      ...tunStatus,
      helper_supported: false,
      helper_installed: false,
    });
    ensureTunElevation.mockResolvedValue(undefined);

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "TUN 模式" })).toBeInTheDocument();
    });
    const tunToggle = view.getByRole("button", { name: "TUN 模式" });

    // Windows (plan B): the one-time elevation component is installed
    // (single UAC), the TUN-on setting persists, and the service starts —
    // no dialog, no installHelper, no app relaunch.
    fireEvent.click(tunToggle);
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(installHelper).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(ensureTunElevation).toHaveBeenCalledTimes(1);
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          tun: expect.objectContaining({ enabled: true }),
        }),
      );
      expect(start).toHaveBeenCalledTimes(1);
    });
  });

  it("reports a cancelled one-time elevation on the home page", async () => {
    getSettings.mockResolvedValue({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: null,
      auto_set_system_proxy: true,
      allow_lan: false,
      proxy_mode: "rule",
      tun: tunSettings,
    });
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: false,
      system_proxy_recorded: false,
      system_proxy_available: true,
      ...tunStatus,
      helper_supported: false,
      helper_installed: false,
    });
    ensureTunElevation.mockRejectedValue("tun.elevation_cancelled: x");

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "TUN 模式" })).toBeInTheDocument();
    });
    const tunToggle = view.getByRole("button", { name: "TUN 模式" });

    // Cancelled: nothing was persisted or started, the error surfaces.
    fireEvent.click(tunToggle);
    await waitFor(() => {
      expect(container.textContent).toContain("tun.elevation_cancelled");
    });
    expect(saveSettings).not.toHaveBeenCalled();
    expect(start).not.toHaveBeenCalled();
    expect(tunToggle).toHaveAttribute("aria-pressed", "false");
  });

  it("prompts helper install before enabling TUN on the home page", async () => {
    installHelper.mockResolvedValue(undefined);
    getSettings.mockResolvedValue({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: null,
      auto_set_system_proxy: true,
      allow_lan: false,
      proxy_mode: "rule",
      tun: tunSettings,
    });
    getStatus.mockResolvedValue({
      core: {
        status: "running",
        message: null,
        inbound_host: "127.0.0.1",
        inbound_port: 17890,
      },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: false,
      system_proxy_recorded: false,
      system_proxy_available: true,
      ...tunStatus,
    });

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "TUN 模式" })).toBeInTheDocument();
    });
    const tunToggle = view.getByRole("button", { name: "TUN 模式" });

    // No helper: dialog appears, nothing is saved.
    fireEvent.click(tunToggle);
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    expect(saveSettings).not.toHaveBeenCalled();

    // Cancel: dialog closes, still nothing saved.
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    });
    expect(saveSettings).not.toHaveBeenCalled();

    // Confirm: install runs, then the TUN-on setting is saved.
    fireEvent.click(tunToggle);
    fireEvent.click(screen.getByRole("button", { name: "安装并启用" }));
    await waitFor(() => {
      expect(installHelper).toHaveBeenCalledTimes(1);
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          tun: expect.objectContaining({ enabled: true }),
        }),
      );
    });
  });
});
