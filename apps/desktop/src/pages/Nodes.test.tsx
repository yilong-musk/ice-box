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
    expect(container.querySelector(".node-table")).toBeNull();
    expect(view.getByRole("list", { name: "节点列表" })).toBeInTheDocument();
    expect(view.getByText("节点")).toBeInTheDocument();
    expect(view.getByRole("button", { name: "批量测延迟" })).toBeInTheDocument();
    expect(view.getByRole("button", { name: "按延迟排序" })).toHaveAttribute(
      "data-state",
      "off",
    );
    expect(container.querySelector(".nodes-panel")?.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["flex-1", "min-h-0", "flex-col"]),
    );
  });

  it("shows empty-state guide when no nodes", async () => {
    listNodes.mockResolvedValue([]);
    const onNavigate = vi.fn();
    const { container } = render(<Nodes onNavigate={onNavigate} />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByText("暂无节点")).toBeInTheDocument();
    });
    expect(view.queryByRole("list", { name: "节点列表" })).toBeNull();
    fireEvent.click(view.getByRole("button", { name: "前往订阅页导入" }));
    expect(onNavigate).toHaveBeenCalledWith("subs");
  });

  it("shows live exit for strategy groups", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByText("→ node-b")).toBeInTheDocument();
      expect(view.getByText("→ node-a")).toBeInTheDocument();
    });
  });

  async function expandGroup(
    view: ReturnType<typeof within>,
    groupName: string,
  ) {
    const toggle = await view.findByRole("button", {
      name: groupName,
      expanded: false,
    });
    fireEvent.click(toggle);
    expect(await view.findByLabelText(`${groupName} 成员`)).toBeInTheDocument();
  }

  it("expands selector group and switches exit by clicking a member", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);

    await expandGroup(view, "选择组");
    fireEvent.click(view.getByLabelText("将 node-b 设为 选择组 出口"));

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

    await expandGroup(view, "选择组");
    fireEvent.click(view.getByLabelText("将 node-b 设为 选择组 出口"));

    await waitFor(() => {
      expect(setGroupSelection).toHaveBeenCalledWith("选择组", "node-b");
    });
  });

  it("expands non-selector groups as read-only member list", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);

    await expandGroup(view, "自动组");

    expect(view.queryByLabelText("将 node-a 设为 自动组 出口")).not.toBeInTheDocument();
    expect(setGroupSelection).not.toHaveBeenCalled();
  });

  it("uses a space-safe id for group member panels", async () => {
    listNodes.mockResolvedValue([
      {
        tag: "My Group",
        outbound_type: "selector",
        group_now: "node-a",
        group_all: ["node-a", "node-b"],
      },
    ]);
    const { container } = render(<Nodes />);
    const view = within(container);

    await expandGroup(view, "My Group");
    const panel = view.getByLabelText("My Group 成员").closest("[id]");
    expect(panel?.id).toBe("group-members-My_20Group");
    expect(panel?.id.includes(" ")).toBe(false);
  });
});
