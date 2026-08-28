import { fireEvent, render, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearNodesSnapshot, writeNodesSnapshot } from "../lib/nodes";
import { Nodes } from "./Nodes";

const listNodes = vi.fn();
const getSettings = vi.fn();
const getStatus = vi.fn();
const setGroupSelection = vi.fn();
const testNodeDelay = vi.fn();

vi.mock("../api/tauri", () => ({
  api: {
    listNodes: (...args: unknown[]) => listNodes(...args),
    getSettings: (...args: unknown[]) => getSettings(...args),
    getStatus: (...args: unknown[]) => getStatus(...args),
    setSelectedNode: vi.fn(),
    setGroupSelection: (...args: unknown[]) => setGroupSelection(...args),
    testNodeDelay: (...args: unknown[]) => testNodeDelay(...args),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

describe("Nodes", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    clearNodesSnapshot();
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
    testNodeDelay.mockImplementation(async (tag: string) => ({
      tag,
      delay_ms: 42,
    }));
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
    expect(view.queryByRole("button", { name: "按延迟排序" })).toBeNull();
    const nodeList = view.getByRole("list", { name: "节点列表" });
    const scrollArea = nodeList.closest('[data-slot="scroll-area"]');
    expect(scrollArea?.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["min-h-0", "flex-1", "overflow-hidden"]),
    );
    expect(
      scrollArea?.querySelector('[data-slot="scroll-area-viewport"]'),
    ).toBeInTheDocument();
    expect(container.querySelector(".nodes-panel")?.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["flex-1", "min-h-0", "flex-col"]),
    );
  });

  it("renders a long node list without dropping later rows", async () => {
    listNodes.mockResolvedValue(
      Array.from({ length: 80 }, (_, i) => ({
        tag: `node-${i}`,
        outbound_type: "trojan",
        group_now: null,
        group_all: null,
      })),
    );
    const { container } = render(<Nodes />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("list", { name: "节点列表" })).toBeInTheDocument();
      expect(view.getByText("node-0")).toBeInTheDocument();
    });
    await waitFor(() => {
      expect(view.getByText("node-79")).toBeInTheDocument();
    });
    const titles = container.querySelectorAll(
      '[aria-label="节点列表"] [data-slot="item-title"]',
    );
    expect(titles).toHaveLength(80);
  });

  it("does not flash the empty-state guide before nodes load", async () => {
    let resolveNodes: (value: unknown[]) => void = () => {};
    listNodes.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveNodes = resolve;
        }),
    );
    const { container } = render(<Nodes />);
    const view = within(container);

    expect(view.queryByText("暂无节点")).toBeNull();
    expect(view.getByText("加载节点列表…")).toBeInTheDocument();

    resolveNodes([
      {
        tag: "node-a",
        outbound_type: "socks",
        group_now: null,
        group_all: null,
      },
    ]);
    await waitFor(() => {
      expect(view.getByText("node-a")).toBeInTheDocument();
    });
    expect(view.queryByText("暂无节点")).toBeNull();
    expect(view.queryByText("加载节点列表…")).toBeNull();
  });

  it("hydrates from the shared snapshot while the next fetch is in flight", () => {
    writeNodesSnapshot({
      nodes: [
        {
          tag: "cached-node",
          outbound_type: "trojan",
          group_now: null,
          group_all: null,
        },
      ],
      selectedTag: "cached-node",
      running: true,
    });
    listNodes.mockImplementation(() => new Promise(() => {}));
    const { container } = render(<Nodes />);
    const view = within(container);

    expect(view.getByText("cached-node")).toBeInTheDocument();
    expect(view.queryByText("暂无节点")).toBeNull();
    expect(view.queryByText("加载节点列表…")).toBeNull();
  });

  it("refreshes cached nodes immediately when the pane is activated", async () => {
    writeNodesSnapshot({
      nodes: [
        {
          tag: "cached-node",
          outbound_type: "trojan",
          group_now: null,
          group_all: null,
        },
      ],
      selectedTag: "cached-node",
      running: true,
    });
    listNodes.mockResolvedValue([
      {
        tag: "fresh-node",
        outbound_type: "trojan",
        group_now: null,
        group_all: null,
      },
    ]);
    const { container, rerender } = render(<Nodes active={false} />);
    const view = within(container);

    expect(view.getByText("cached-node")).toBeInTheDocument();
    expect(listNodes).not.toHaveBeenCalled();

    rerender(<Nodes active />);
    await waitFor(() => {
      expect(view.getByText("fresh-node")).toBeInTheDocument();
    });
    expect(view.queryByText("cached-node")).toBeNull();
  });

  it("does not mount collapsed strategy-group members", () => {
    writeNodesSnapshot({
      nodes: [
        {
          tag: "选择组",
          outbound_type: "selector",
          group_now: "leaf-0",
          group_all: Array.from({ length: 90 }, (_, i) => `leaf-${i}`),
        },
        {
          tag: "leaf-0",
          outbound_type: "trojan",
          group_now: null,
          group_all: null,
        },
      ],
      selectedTag: "leaf-0",
      running: true,
    });
    listNodes.mockImplementation(() => new Promise(() => {}));
    const { container } = render(<Nodes />);
    const view = within(container);

    expect(
      view.getByRole("button", { name: "选择组", expanded: false }),
    ).toBeInTheDocument();
    expect(view.queryByLabelText("选择组 成员")).not.toBeInTheDocument();
    expect(view.queryByText("leaf-89")).toBeNull();
  });

  it("reveals a large strategy-group member list after expand", async () => {
    const groupAll = Array.from({ length: 90 }, (_, i) => `leaf-${i}`);
    writeNodesSnapshot({
      nodes: [
        {
          tag: "选择组",
          outbound_type: "selector",
          group_now: "leaf-0",
          group_all: groupAll,
        },
      ],
      selectedTag: "选择组",
      running: true,
    });
    listNodes.mockImplementation(() => new Promise(() => {}));
    const { container } = render(<Nodes />);
    const view = within(container);

    await expandGroup(view, "选择组");
    await waitFor(() => {
      expect(
        view.getByLabelText("将 leaf-89 设为 选择组 出口"),
      ).toBeInTheDocument();
    });
  });

  it("opens a long list with one screen then fills the rest", async () => {
    writeNodesSnapshot({
      nodes: Array.from({ length: 80 }, (_, i) => ({
        tag: `node-${i}`,
        outbound_type: "trojan",
        group_now: null,
        group_all: null,
      })),
      selectedTag: "node-0",
      running: true,
    });
    listNodes.mockImplementation(() => new Promise(() => {}));
    const { container } = render(<Nodes />);
    const view = within(container);

    expect(view.getByText("node-0")).toBeInTheDocument();
    const firstPaint = container.querySelectorAll(
      '[aria-label="节点列表"] [data-slot="item-title"]',
    );
    expect(firstPaint.length).toBeGreaterThan(0);
    expect(firstPaint.length).toBeLessThanOrEqual(8);
    expect(view.queryByText("node-79")).toBeNull();

    await waitFor(() => {
      expect(view.getByText("node-79")).toBeInTheDocument();
    });
    expect(
      container.querySelectorAll('[aria-label="节点列表"] [data-slot="item-title"]'),
    ).toHaveLength(80);
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

  it("toggles a strategy group from the row except 测速 and 选用", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);

    const toggle = await view.findByRole("button", {
      name: "选择组",
      expanded: false,
    });
    const row = toggle.closest("[data-slot='item']");
    expect(row).not.toBeNull();
    const rowView = within(row as HTMLElement);

    fireEvent.click(view.getByText("策略组 · selector"));
    expect(await view.findByLabelText("选择组 成员")).toBeInTheDocument();

    fireEvent.click(view.getByText("策略组 · selector"));
    expect(view.queryByLabelText("选择组 成员")).not.toBeInTheDocument();

    fireEvent.click(rowView.getByText("→ node-a"));
    expect(await view.findByLabelText("选择组 成员")).toBeInTheDocument();

    fireEvent.click(rowView.getByRole("button", { name: "测速" }));
    expect(view.getByLabelText("选择组 成员")).toBeInTheDocument();

    fireEvent.click(rowView.getByRole("button", { name: "选用" }));
    expect(view.getByLabelText("选择组 成员")).toBeInTheDocument();
  });

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

  async function clickGroupDelayTest(
    view: ReturnType<typeof within>,
    groupName: string,
    expanded: boolean,
  ) {
    const toggle = await view.findByRole("button", {
      name: groupName,
      expanded,
    });
    const row = toggle.closest("[data-slot='item']");
    expect(row).not.toBeNull();
    fireEvent.click(
      within(row as HTMLElement).getByRole("button", { name: "测速" }),
    );
  }

  it("tests only the selected exit when a strategy group is collapsed", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);

    await clickGroupDelayTest(view, "选择组", false);

    await waitFor(() => {
      expect(testNodeDelay).toHaveBeenCalledWith("node-a");
    });
    expect(testNodeDelay).toHaveBeenCalledTimes(1);
    expect(testNodeDelay).not.toHaveBeenCalledWith("选择组");
    expect(testNodeDelay).not.toHaveBeenCalledWith("node-b");
    const toggle = view.getByRole("button", { name: "选择组" });
    const row = toggle.closest("[data-slot='item']");
    await waitFor(() => {
      expect(within(row as HTMLElement).getByText("42 ms")).toHaveClass(
        "text-ok",
      );
    });
  });

  it("tests every member when a strategy group is expanded", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);

    await expandGroup(view, "选择组");
    await clickGroupDelayTest(view, "选择组", true);

    await waitFor(() => {
      expect(testNodeDelay).toHaveBeenCalledWith("node-a");
      expect(testNodeDelay).toHaveBeenCalledWith("node-b");
    });
    expect(testNodeDelay).toHaveBeenCalledTimes(2);
    expect(testNodeDelay).not.toHaveBeenCalledWith("选择组");
  });

  it("shows an error when a collapsed group has no exit to test", async () => {
    listNodes.mockResolvedValue([
      {
        tag: "选择组",
        outbound_type: "selector",
        group_now: null,
        group_all: ["node-a", "node-b"],
      },
    ]);
    const { container } = render(<Nodes />);
    const view = within(container);

    await clickGroupDelayTest(view, "选择组", false);

    expect(
      await view.findByText("当前策略组没有可测的出口"),
    ).toBeInTheDocument();
    expect(testNodeDelay).not.toHaveBeenCalled();
  });

  it("shows an error when batch delay test has no leaf exits", async () => {
    listNodes.mockResolvedValue([
      {
        tag: "选择组",
        outbound_type: "selector",
        group_now: null,
        group_all: ["node-a"],
      },
    ]);
    const { container } = render(<Nodes />);
    const view = within(container);

    fireEvent.click(await view.findByRole("button", { name: "批量测延迟" }));

    expect(await view.findByText("当前没有可测的出口")).toBeInTheDocument();
    expect(testNodeDelay).not.toHaveBeenCalled();
  });

  it("expands a strategy group from the keyboard", async () => {
    const user = userEvent.setup();
    const { container } = render(<Nodes />);
    const view = within(container);
    const toggle = await view.findByRole("button", {
      name: "选择组",
      expanded: false,
    });
    toggle.focus();
    await user.keyboard("{Enter}");
    expect(await view.findByLabelText("选择组 成员")).toBeInTheDocument();
  });

  it("clears an in-flight delay cell when cancelled", async () => {
    let settle = (_value: { tag: string; delay_ms: number }) => {};
    testNodeDelay.mockImplementation(
      () =>
        new Promise((resolve) => {
          settle = resolve;
        }),
    );
    const { container } = render(<Nodes />);
    const view = within(container);

    await expandGroup(view, "选择组");
    await clickGroupDelayTest(view, "选择组", true);

    try {
      await waitFor(() => {
        expect(view.getAllByText("…").length).toBeGreaterThan(0);
      });
      fireEvent.click(view.getByRole("button", { name: "取消" }));
      await waitFor(() => {
        expect(view.queryAllByText("…")).toHaveLength(0);
      });
    } finally {
      settle({ tag: "node-a", delay_ms: 1 });
    }
  });

  it("keeps original node order after batch delay test", async () => {
    testNodeDelay.mockImplementation(async (tag: string) => ({
      tag,
      delay_ms: tag === "node-b" ? 10 : 800,
    }));
    const { container } = render(<Nodes />);
    const view = within(container);

    fireEvent.click(await view.findByRole("button", { name: "批量测延迟" }));

    await waitFor(() => {
      expect(testNodeDelay).toHaveBeenCalledTimes(2);
    });
    expect(testNodeDelay).toHaveBeenCalledWith("node-a");
    expect(testNodeDelay).toHaveBeenCalledWith("node-b");
    expect(testNodeDelay).not.toHaveBeenCalledWith("选择组");
    expect(testNodeDelay).not.toHaveBeenCalledWith("自动组");
    await waitFor(() => {
      expect(view.getByRole("button", { name: "批量测延迟" })).toBeEnabled();
    });

    const titles = [
      ...container.querySelectorAll(
        '[aria-label="节点列表"] [data-slot="item-title"]',
      ),
    ].map((el) =>
      (el.textContent ?? "").replace(/选用中/g, "").replace(/\s+/g, " ").trim(),
    );
    expect(titles).toEqual(["node-a", "node-b", "选择组", "自动组"]);

    const selectorRow = view
      .getByRole("button", { name: "选择组" })
      .closest("[data-slot='item']");
    expect(within(selectorRow as HTMLElement).getByText("800 ms")).toHaveClass(
      "text-warn",
    );
    const autoRow = view
      .getByRole("button", { name: "自动组" })
      .closest("[data-slot='item']");
    expect(within(autoRow as HTMLElement).getByText("10 ms")).toHaveClass(
      "text-ok",
    );
  });
});
