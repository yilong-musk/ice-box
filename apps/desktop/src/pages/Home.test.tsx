import { fireEvent, render, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Home } from "./Home";

const getStatus = vi.fn();
const listNodes = vi.fn();
const getSettings = vi.fn();
const getConnectionStats = vi.fn();
const getTrafficSample = vi.fn();
const setSelectedNode = vi.fn();
const setProxyMode = vi.fn();
const testNodeDelay = vi.fn();

vi.mock("../api/tauri", () => ({
  api: {
    getStatus: (...args: unknown[]) => getStatus(...args),
    listNodes: (...args: unknown[]) => listNodes(...args),
    getSettings: (...args: unknown[]) => getSettings(...args),
    getConnectionStats: (...args: unknown[]) => getConnectionStats(...args),
    getTrafficSample: (...args: unknown[]) => getTrafficSample(...args),
    setSelectedNode: (...args: unknown[]) => setSelectedNode(...args),
    setProxyMode: (...args: unknown[]) => setProxyMode(...args),
    testNodeDelay: (...args: unknown[]) => testNodeDelay(...args),
    start: vi.fn(),
    stopSystemProxy: vi.fn(),
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
    getTrafficSample.mockResolvedValue({ up: 0, down: 0 });
  });

  it("reverts node selection when setSelectedNode fails", async () => {
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
      { tag: "node-a", outbound_type: "socks" },
      { tag: "node-b", outbound_type: "vmess" },
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
    });
    setSelectedNode.mockRejectedValue("switch failed");

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("combobox")).toBeInTheDocument();
    });

    const nodeSelect = view.getByRole("combobox");
    fireEvent.change(nodeSelect, {
      target: { value: "node-b" },
    });

    await waitFor(() => {
      expect(setSelectedNode).toHaveBeenCalledWith("node-b");
    });

    await waitFor(() => {
      expect(view.getByText("switch failed")).toBeInTheDocument();
    });
    await waitFor(() => {
      expect((nodeSelect as HTMLSelectElement).value).toBe("node-a");
    });
  });

  it("switches proxy mode and reverts the select when it fails", async () => {
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
    listNodes.mockResolvedValue([{ tag: "node-a", outbound_type: "socks" }]);
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

    const globalButton = () => view.getByRole("button", { name: "全局" });
    const directButton = () => view.getByRole("button", { name: "直连" });

    await waitFor(() => {
      expect(globalButton()).toBeInTheDocument();
    });
    expect(globalButton().className).toContain("mode-button");
    expect(globalButton().className).not.toContain("active");

    fireEvent.click(globalButton());
    await waitFor(() => {
      expect(setProxyMode).toHaveBeenCalledWith("global");
    });
    await waitFor(() => {
      expect(globalButton().className).toContain("active");
    });

    setProxyMode.mockRejectedValue("mode switch failed");
    fireEvent.click(directButton());
    await waitFor(() => {
      expect(view.getByText("mode switch failed")).toBeInTheDocument();
    });
    await waitFor(() => {
      expect(globalButton().className).toContain("active");
      expect(directButton().className).not.toContain("active");
    });
  });

  it("shows empty-state guide and system-proxy buttons when no nodes", async () => {
    const onNavigate = vi.fn();
    const { container } = render(<Home onNavigate={onNavigate} />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByText("还没有可用节点")).toBeInTheDocument();
    });
    expect(view.getByRole("button", { name: "启动代理服务" })).not.toBeDisabled();
    expect(view.getByRole("button", { name: "停止代理服务" })).toBeDisabled();
    fireEvent.click(view.getByRole("button", { name: "前往订阅页导入" }));
    expect(onNavigate).toHaveBeenCalledWith("subs");
  });

  it("shows delay result even while status polling continues", async () => {
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
    listNodes.mockResolvedValue([{ tag: "node-a", outbound_type: "socks" }]);
    getSettings.mockResolvedValue({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: "node-a",
      auto_set_system_proxy: true,
      allow_lan: false,
      proxy_mode: "rule",
    });
    testNodeDelay.mockImplementation(
      () =>
        new Promise((resolve) => {
          window.setTimeout(() => resolve({ tag: "node-a", delay_ms: 42 }), 50);
        }),
    );

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "测延迟" })).not.toBeDisabled();
    });

    fireEvent.click(view.getByRole("button", { name: "测延迟" }));

    await waitFor(() => {
      expect(view.getByText("42 ms")).toBeInTheDocument();
    });
  });

  it("shows proxy-off hint when core running but system proxy not applied", async () => {
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
    listNodes.mockResolvedValue([{ tag: "node-a", outbound_type: "socks" }]);

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByText("系统代理未接管或已不同步")).toBeInTheDocument();
    });
    expect(view.getByRole("button", { name: "启动代理服务" })).not.toBeDisabled();
    expect(view.getByRole("button", { name: "停止代理服务" })).toBeDisabled();
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

  it("keeps stop enabled when disk recorded but live check is false", async () => {
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
    listNodes.mockResolvedValue([{ tag: "node-a", outbound_type: "socks" }]);

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "停止代理服务" })).not.toBeDisabled();
    });
    expect(view.getByRole("button", { name: "启动代理服务" })).not.toBeDisabled();
    expect(view.getByText("已不同步")).toBeInTheDocument();
  });
});
