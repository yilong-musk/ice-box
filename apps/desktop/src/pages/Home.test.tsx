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

  it("shows empty-node hint and disables start when no nodes", async () => {
    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(
        view.getByText("暂无节点，请先在「订阅」页导入。"),
      ).toBeInTheDocument();
    });
    expect(view.getByRole("button", { name: "启动" })).toBeDisabled();
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

  it("shows proxy sync hint when core running but system proxy pending", async () => {
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
    });
    listNodes.mockResolvedValue([{ tag: "node-a", outbound_type: "socks" }]);

    const { container } = render(<Home />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByText("系统代理同步中…")).toBeInTheDocument();
    });
  });
});
