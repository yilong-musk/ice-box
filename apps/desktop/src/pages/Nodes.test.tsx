import { fireEvent, render, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Nodes } from "./Nodes";

const listNodes = vi.fn();
const getSettings = vi.fn();
const getStatus = vi.fn();
const setGroupSelection = vi.fn();

vi.mock("../api/tauri", () => ({
  api: {
    listNodes: (...args: unknown[]) => listNodes(...args),
    getSettings: (...args: unknown[]) => getSettings(...args),
    getStatus: (...args: unknown[]) => getStatus(...args),
    setSelectedNode: vi.fn(),
    setGroupSelection: (...args: unknown[]) => setGroupSelection(...args),
    testNodeDelay: vi.fn(),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

describe("Nodes", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listNodes.mockResolvedValue([
      { tag: "node-a", outbound_type: "socks", group_now: null, group_all: null },
      { tag: "node-b", outbound_type: "vmess", group_now: null, group_all: null },
      {
        tag: "选择组",
        outbound_type: "selector",
        group_now: "node-a",
        group_all: ["node-a", "node-b"],
      },
      {
        tag: "自动组",
        outbound_type: "urltest",
        group_now: "node-b",
        group_all: ["node-a", "node-b"],
      },
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
    getStatus.mockResolvedValue({
      core: { status: "running", message: null, inbound_host: "127.0.0.1", inbound_port: 17890 },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: true,
      system_proxy_recorded: true,
      system_proxy_available: true,
    });
    setGroupSelection.mockResolvedValue(undefined);
  });

  it("renders node list from backend", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getAllByText("node-a").length).toBeGreaterThan(0);
      expect(view.getAllByText("node-b").length).toBeGreaterThan(0);
    });
  });

  it("shows live exit for strategy groups", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByText("→ node-b")).toBeInTheDocument();
    });
    expect(view.getByLabelText("选择组 出口")).toHaveValue("node-a");
  });

  it("switches selector group member via dropdown", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByLabelText("选择组 出口")).toBeInTheDocument();
    });
    fireEvent.change(view.getByLabelText("选择组 出口"), {
      target: { value: "node-b" },
    });
    await waitFor(() => {
      expect(setGroupSelection).toHaveBeenCalledWith("选择组", "node-b");
    });
  });

  it("persists selection even when core is stopped", async () => {
    getStatus.mockResolvedValue({
      core: { status: "stopped", message: null, inbound_host: null, inbound_port: null },
      subscription_count: 1,
      proxy_recovery_warning: null,
      system_proxy_applied: null,
      system_proxy_recorded: null,
      system_proxy_available: true,
    });
    const { container } = render(<Nodes />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByLabelText("选择组 出口")).toBeInTheDocument();
    });
    fireEvent.change(view.getByLabelText("选择组 出口"), {
      target: { value: "node-b" },
    });
    await waitFor(() => {
      expect(setGroupSelection).toHaveBeenCalledWith("选择组", "node-b");
    });
  });
});