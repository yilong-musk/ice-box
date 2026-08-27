import { act, fireEvent, render, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Home } from "./Home";

const getStatus = vi.fn();
const listNodes = vi.fn();
const getSettings = vi.fn();
const getConnectionStats = vi.fn();
const getTrafficSnapshot = vi.fn();
const setProxyMode = vi.fn();
const start = vi.fn();
const stopSystemProxy = vi.fn();

vi.mock("../api/tauri", () => ({
  api: {
    getStatus: (...args: unknown[]) => getStatus(...args),
    listNodes: (...args: unknown[]) => listNodes(...args),
    getSettings: (...args: unknown[]) => getSettings(...args),
    getConnectionStats: (...args: unknown[]) => getConnectionStats(...args),
    getTrafficSnapshot: (...args: unknown[]) => getTrafficSnapshot(...args),
    setProxyMode: (...args: unknown[]) => setProxyMode(...args),
    start: (...args: unknown[]) => start(...args),
    stopSystemProxy: (...args: unknown[]) => stopSystemProxy(...args),
    stop: vi.fn(),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

describe("Home", () => {
  beforeEach(() => {
    vi.clearAllMocks();
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
    });
    getConnectionStats.mockResolvedValue({ connection_count: 0 });
    getTrafficSnapshot.mockResolvedValue({ points: [], latest: null });
    start.mockResolvedValue(undefined);
    stopSystemProxy.mockResolvedValue(undefined);
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
    expect(view.getByText("代理模式")).toBeInTheDocument();
    expect(view.queryByText("系统代理")).toBeNull();
    expect(container.querySelector(".home-panel")?.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["flex-1", "min-h-0", "flex-col"]),
    );
    expect(view.getByRole("radiogroup", { name: "模式" })).toBeInTheDocument();
    expect(view.getByRole("radio", { name: "规则" })).toHaveAttribute("data-state", "on");
    expect(view.getByRole("radio", { name: "全局" })).toHaveAttribute("data-state", "off");
    expect(view.getByRole("radio", { name: "直连" })).toHaveAttribute("data-state", "off");
    expect(view.getByRole("button", { name: "停止代理服务" })).toBeInTheDocument();
    expect(view.queryByRole("button", { name: "测延迟" })).toBeNull();
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
    });

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByText("当前平台不支持系统代理接管")).toBeInTheDocument();
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
});
