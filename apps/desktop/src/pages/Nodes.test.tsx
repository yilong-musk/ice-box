// SPDX-License-Identifier: GPL-3.0-or-later

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { t } from "../lib/i18n";
import { clearNodesSnapshot, readNodesSnapshot, writeNodesSnapshot } from "../lib/nodes";
import { RuntimeStoreProvider } from "../lib/runtimeStore";
import { Nodes } from "./Nodes";

const listNodes = vi.fn();
const getSettings = vi.fn();
const getStatus = vi.fn();
const setSelectedNode = vi.fn();
const setNodeFavorite = vi.fn();
const setGroupSelection = vi.fn();
const testNodeDelay = vi.fn();
const listenStateChanged = vi.fn();
const phoneShell = { value: false };

vi.mock("@platform/shell", () => ({
  isPhoneShell: () => phoneShell.value,
}));

/** Handler the page registers for `app://state-changed` (tray actions). */
let stateChangedHandler: (() => void) | null = null;

vi.mock("../api/client", () => ({
  api: {
    listNodes: (...args: unknown[]) => listNodes(...args),
    getSettings: (...args: unknown[]) => getSettings(...args),
    getStatus: (...args: unknown[]) => getStatus(...args),
    setSelectedNode: (...args: unknown[]) => setSelectedNode(...args),
    setNodeFavorite: (...args: unknown[]) => setNodeFavorite(...args),
    setGroupSelection: (...args: unknown[]) => setGroupSelection(...args),
    testNodeDelay: (...args: unknown[]) => testNodeDelay(...args),
    listenStateChanged: (...args: unknown[]) => listenStateChanged(...args),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

describe("Nodes", () => {
  beforeEach(() => {
    const actions = document.createElement("div");
    actions.dataset.titlebarActions = "";
    document.body.appendChild(actions);
    vi.clearAllMocks();
    phoneShell.value = false;
    stateChangedHandler = null;
    listenStateChanged.mockImplementation((handler: () => void) => {
      stateChangedHandler = handler;
      return Promise.resolve(() => {});
    });
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
      proxy_service_enabled: false,
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
    setSelectedNode.mockResolvedValue(undefined);
    setNodeFavorite.mockResolvedValue(undefined);
    testNodeDelay.mockImplementation(async (tag: string) => ({
      tag,
      delay_ms: 42,
    }));
  });

  afterEach(() => {
    document.querySelector("[data-titlebar-actions]")?.remove();
  });

  it("renders node list from backend", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getAllByText("node-a").length).toBeGreaterThan(0);
      expect(view.getAllByText("node-b").length).toBeGreaterThan(0);
    });
    expect(container.querySelector(".node-table")).toBeNull();
    expect(view.getByRole("list", { name: t("nodes.listAria") })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: t("nodes.batchTest") })).toBeInTheDocument();
    const nodeList = view.getByRole("list", { name: t("nodes.listAria") });
    const scrollArea = nodeList.closest('[data-slot="scroll-area"]');
    expect(
      scrollArea?.querySelector('[data-slot="scroll-area-viewport"]'),
    ).toBeInTheDocument();
    expect(view.getByTestId("nodes-panel")).toBeInTheDocument();
  });

  it("shows no delay placeholder before a node was tested", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);
    await view.findByRole("list", { name: t("nodes.listAria") });

    expect(view.queryByText(t("common.dash"))).not.toBeInTheDocument();
    expect(view.queryByTestId("delay-testing")).not.toBeInTheDocument();
  });

  it("drops the per-node test on the phone and shortens the batch test", async () => {
    phoneShell.value = true;
    const { container } = render(<Nodes embedded />);
    const view = within(container);
    await view.findByRole("list", { name: t("nodes.listAria") });

    expect(view.queryAllByRole("button", { name: t("nodes.test") })).toHaveLength(0);
    expect(view.getAllByRole("button", { name: t("nodes.select") }).length).toBeGreaterThan(0);
    const batch = view.getByRole("button", { name: t("nodes.batchTest") });
    // Icon plus the short label, so it fits beside the card title on a
    // narrow screen; the full name stays as its accessible name.
    expect(batch.textContent).toBe(t("nodes.test"));
    expect(batch.querySelector("svg")).not.toBeNull();
  });

  it("keeps the full row and text batch test when embedded on desktop", async () => {
    const { container } = render(<Nodes embedded />);
    const view = within(container);
    await view.findByRole("list", { name: t("nodes.listAria") });

    // The per-node test is the phone header's gauge icon, named for a11y.
    const rowTest = view.getAllByRole("button", { name: t("nodes.test") })[0];
    expect(rowTest.textContent).toBe("");
    expect(rowTest.querySelector("svg")).not.toBeNull();
    expect(view.getByRole("button", { name: t("nodes.batchTest") })).toHaveTextContent(t("nodes.batchTest"));
  });

  it("does not warn while the service status is still loading", async () => {
    getStatus.mockImplementation(async () => {
      // Hold the status past the node/settings reads: the list used to render
      // with running=false before the status arrived and flashed the warning.
      await new Promise((resolve) => setTimeout(resolve, 20));
      return {
        core: { status: "running", message: null, inbound_host: "127.0.0.1", inbound_port: 17890 },
        subscription_count: 1,
        proxy_recovery_warning: null,
        system_proxy_applied: true,
        system_proxy_recorded: true,
        system_proxy_available: true,
      };
    });
    const { container } = render(
      <RuntimeStoreProvider>
        <Nodes />
      </RuntimeStoreProvider>,
    );
    const view = within(container);

    await view.findAllByText("node-a");
    expect(view.queryByText(t("nodes.notRunningWarn"))).not.toBeInTheDocument();
  });

  it("publishes the selected node to the shared snapshot", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByRole("list", { name: t("nodes.listAria") })).toBeInTheDocument();
    });
    const selectButtons = view.getAllByRole("button", { name: t("nodes.select") });
    const enabled = selectButtons.filter((btn) => !(btn as HTMLButtonElement).disabled);
    fireEvent.click(enabled[0]);
    await waitFor(() => {
      expect(setSelectedNode).toHaveBeenCalledWith("node-b");
    });
    await waitFor(() => {
      expect(readNodesSnapshot()?.selectedTag).toBe("node-b");
    });
  });

  it("shows selection as an icon toggle without an in-use text label", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);
    await view.findByRole("list", { name: t("nodes.listAria") });
    const current = view.getByRole("button", { name: t("nodes.inUse") });
    expect(current).toBeDisabled();
    expect(current).toHaveAttribute("aria-current", "true");
    expect(current).toHaveTextContent("");
    for (const button of view.getAllByRole("button", { name: t("nodes.select") })) {
      expect(button).toHaveTextContent("");
    }
    expect(view.queryByText(t("nodes.inUse"))).not.toBeInTheDocument();
  });

  it("re-reads the active exit after an out-of-window switch", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByRole("list", { name: t("nodes.listAria") })).toBeInTheDocument();
    });
    const fetchesBefore = listNodes.mock.calls.length;

    // The tray menu switched the strategy group to another member.
    listNodes.mockResolvedValue([
      { tag: "node-a", outbound_type: "socks", group_now: null, group_all: null },
      { tag: "node-b", outbound_type: "vmess", group_now: null, group_all: null },
      {
        tag: "选择组",
        outbound_type: "selector",
        group_now: "node-b",
        group_all: ["node-a", "node-b"],
      },
    ]);

    await act(async () => {
      stateChangedHandler?.();
    });

    await waitFor(() => {
      expect(listNodes.mock.calls.length).toBeGreaterThan(fetchesBefore);
      const group = readNodesSnapshot()?.nodes.find((n) => n.tag === "选择组");
      expect(group?.group_now).toBe("node-b");
    });
  });

  it("publishes a completed node switch even when the pane is hidden", async () => {
    let resolveSelect: () => void = () => {};
    setSelectedNode.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          resolveSelect = resolve;
        }),
    );
    const { container, rerender } = render(<Nodes active />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByRole("list", { name: t("nodes.listAria") })).toBeInTheDocument();
    });
    const selectButtons = view.getAllByRole("button", { name: t("nodes.select") });
    const enabled = selectButtons.filter((btn) => !(btn as HTMLButtonElement).disabled);
    fireEvent.click(enabled[0]);
    await waitFor(() => {
      expect(setSelectedNode).toHaveBeenCalledWith("node-b");
    });
    rerender(<Nodes active={false} />);
    resolveSelect();
    await waitFor(() => {
      expect(readNodesSnapshot()?.selectedTag).toBe("node-b");
    });
  });

  it("publishes a group exit switch even when the pane is hidden", async () => {
    let resolveGroup: () => void = () => {};
    setGroupSelection.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          resolveGroup = resolve;
        }),
    );
    const { container, rerender } = render(<Nodes active />);
    const view = within(container);
    await expandGroup(view, "选择组");
    fireEvent.click(
      view.getByLabelText(t("nodes.setMemberAria", { member: "node-b", group: "选择组" })),
    );
    await waitFor(() => {
      expect(setGroupSelection).toHaveBeenCalledWith("选择组", "node-b");
    });
    rerender(<Nodes active={false} />);
    resolveGroup();
    await waitFor(() => {
      const group = readNodesSnapshot()?.nodes.find((n) => n.tag === "选择组");
      expect(group?.group_now).toBe("node-b");
    });
  });

  it("keeps member lists and expanded groups while the pane is hidden", async () => {
    const members = Array.from({ length: 40 }, (_, i) => `leaf-${i}`);
    listNodes.mockResolvedValue([
      {
        tag: "选择组",
        outbound_type: "selector",
        group_now: "leaf-0",
        group_all: members,
      },
      { tag: "leaf-0", outbound_type: "trojan", group_now: null, group_all: null },
    ]);
    const { container, rerender } = render(<Nodes active />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("选择组")).toBeInTheDocument();
    });
    // The session-wide snapshot never holds member lists; the page does.
    expect(readNodesSnapshot()?.nodes[0]?.group_all).toBeNull();

    await expandGroup(view, "选择组");

    rerender(<Nodes active={false} />);
    // The rows unmount, the data stays.
    expect(view.queryByText("选择组")).toBeNull();

    // Coming back renders the expanded group before any refetch lands.
    listNodes.mockImplementation(() => new Promise(() => {}));
    rerender(<Nodes active />);
    expect(
      view.getByRole("button", { name: "选择组", expanded: true }),
    ).toBeInTheDocument();
    expect(
      view.getByLabelText(t("nodes.membersAria", { group: "选择组" })),
    ).toBeInTheDocument();
  });

  it("drops delay results and expanded groups when the lineup changes", async () => {
    testNodeDelay.mockRejectedValue("probe timed out");
    const { container } = render(<Nodes />);
    const view = within(container);
    const nodeList = await view.findByRole("list", { name: t("nodes.listAria") });
    const rowOf = (tag: string) =>
      within(nodeList).getAllByText(tag)[0].closest("[data-slot='item']") as HTMLElement;
    fireEvent.click(within(rowOf("node-a")).getByRole("button", { name: t("nodes.test") }));
    await waitFor(() => {
      expect(within(rowOf("node-a")).getByTestId("delay-error")).toBeInTheDocument();
    });
    await expandGroup(view, "选择组");

    // A refresh with the same lineup (only a group exit moved) keeps both.
    listNodes.mockResolvedValue([
      { tag: "node-a", outbound_type: "socks", group_now: null, group_all: null },
      { tag: "node-b", outbound_type: "vmess", group_now: null, group_all: null },
      {
        tag: "选择组",
        outbound_type: "selector",
        group_now: "node-b",
        group_all: ["node-a", "node-b"],
      },
      {
        tag: "自动组",
        outbound_type: "urltest",
        group_now: "node-a",
        group_all: ["node-a", "node-b"],
      },
    ]);
    await act(async () => {
      stateChangedHandler?.();
    });
    await waitFor(() => {
      expect(readNodesSnapshot()?.nodes[3]?.group_now).toBe("node-a");
    });
    expect(within(rowOf("node-a")).getByTestId("delay-error")).toBeInTheDocument();
    expect(view.getByRole("button", { name: "选择组", expanded: true })).toBeInTheDocument();

    // Another subscription reuses the tags: the old results no longer apply.
    listNodes.mockResolvedValue([
      { tag: "node-a", outbound_type: "socks", group_now: null, group_all: null },
      { tag: "node-c", outbound_type: "vmess", group_now: null, group_all: null },
      {
        tag: "选择组",
        outbound_type: "selector",
        group_now: "node-a",
        group_all: ["node-a", "node-c"],
      },
    ]);
    await act(async () => {
      stateChangedHandler?.();
    });
    await waitFor(() => {
      expect(view.getAllByText("node-c").length).toBeGreaterThan(0);
    });
    expect(within(rowOf("node-a")).queryByTestId("delay-error")).toBeNull();
    expect(view.getByRole("button", { name: "选择组", expanded: false })).toBeInTheDocument();
  });

  it("drops results of a delay test that spans a lineup change", async () => {
    let finish: (value: { tag: string; delay_ms: number }) => void = () => {};
    testNodeDelay.mockImplementation(
      () => new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const { container } = render(<Nodes />);
    const view = within(container);
    const nodeList = await view.findByRole("list", { name: t("nodes.listAria") });
    const rowOf = (tag: string) =>
      within(nodeList).getAllByText(tag)[0].closest("[data-slot='item']") as HTMLElement;
    fireEvent.click(within(rowOf("node-a")).getByRole("button", { name: t("nodes.test") }));
    await waitFor(() => {
      expect(within(rowOf("node-a")).getByTestId("delay-testing")).toBeInTheDocument();
    });

    listNodes.mockResolvedValue([
      { tag: "node-a", outbound_type: "socks", group_now: null, group_all: null },
      { tag: "node-c", outbound_type: "vmess", group_now: null, group_all: null },
    ]);
    await act(async () => {
      stateChangedHandler?.();
    });
    await waitFor(() => {
      expect(view.getAllByText("node-c").length).toBeGreaterThan(0);
    });
    expect(within(rowOf("node-a")).queryByTestId("delay-testing")).toBeNull();

    await act(async () => {
      finish({ tag: "node-a", delay_ms: 42 });
    });
    expect(within(rowOf("node-a")).queryByText(/42/)).toBeNull();
    // The test still ends: the controls are usable again.
    await waitFor(() => {
      expect(
        within(rowOf("node-a")).getByRole("button", { name: t("nodes.test") }),
      ).toBeEnabled();
    });
  });

  it("renders later rows when scrolling a freshly loaded long list", async () => {
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
      expect(view.getByRole("list", { name: t("nodes.listAria") })).toBeInTheDocument();
      expect(view.getByText("node-0")).toBeInTheDocument();
    });
    fireEvent.scroll(container.querySelector('[data-slot="scroll-area-viewport"]')!, {
      target: { scrollTop: 3880 },
    });
    await waitFor(() => {
      expect(view.getByText("node-79")).toBeInTheDocument();
    });
    const titles = container.querySelectorAll(
      `[aria-label="${t("nodes.listAria")}"] [data-slot="item-title"]`,
    );
    expect(titles.length).toBeLessThanOrEqual(15);
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

    expect(view.queryByText(t("nodes.emptyTitle"))).toBeNull();
    expect(view.getByText(t("nodes.loading"))).toBeInTheDocument();

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
    expect(view.queryByText(t("nodes.emptyTitle"))).toBeNull();
    expect(view.queryByText(t("nodes.loading"))).toBeNull();
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
    expect(view.queryByText(t("nodes.emptyTitle"))).toBeNull();
    expect(view.queryByText(t("nodes.loading"))).toBeNull();
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

    expect(view.queryByText("cached-node")).toBeNull();
    expect(listNodes).not.toHaveBeenCalled();

    rerender(<Nodes active />);
    await waitFor(() => {
      expect(view.getByText("fresh-node")).toBeInTheDocument();
    });
    expect(view.queryByText("cached-node")).toBeNull();
  });

  it("does not mount collapsed strategy-group members", async () => {
    listNodes.mockResolvedValue([
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
    ]);
    const { container } = render(<Nodes />);
    const view = within(container);

    expect(
      await view.findByRole("button", { name: "选择组", expanded: false }),
    ).toBeInTheDocument();
    expect(view.queryByLabelText(t("nodes.membersAria", { group: "选择组" }))).not.toBeInTheDocument();
    expect(view.queryByText("leaf-89")).toBeNull();
  });

  it("virtualizes strategy-group members while scrolling", async () => {
    const groupAll = Array.from({ length: 90 }, (_, i) => `leaf-${i}`);
    listNodes.mockResolvedValue([
      {
        tag: "选择组",
        outbound_type: "selector",
        group_now: "leaf-0",
        group_all: groupAll,
      },
    ]);
    getSettings.mockResolvedValue({
      ...(await getSettings()),
      selected_tag: "选择组",
    });
    const { container } = render(<Nodes />);
    const view = within(container);

    await expandGroup(view, "选择组");
    expect(view.queryByText("leaf-89")).toBeNull();
    const viewport = container.querySelector('[data-slot="scroll-area-viewport"]')!;
    fireEvent.scroll(viewport, { target: { scrollTop: 2336 } });
    await waitFor(() => {
      expect(
        view.getByLabelText(t("nodes.setMemberAria", { member: "leaf-89", group: "选择组" })),
      ).toBeInTheDocument();
    });
    expect(view.queryByLabelText(t("nodes.memberCurrentAria", { member: "leaf-0" }))).toBeNull();
  });

  it("bounds long-list DOM and restores scrolling after hiding", async () => {
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
    const { container, rerender } = render(<Nodes />);
    const view = within(container);

    expect(view.getByText("node-0")).toBeInTheDocument();
    const firstPaint = container.querySelectorAll(
      `[aria-label="${t("nodes.listAria")}"] [data-slot="item-title"]`,
    );
    expect(firstPaint.length).toBeGreaterThan(0);
    expect(firstPaint.length).toBeLessThanOrEqual(15);
    expect(view.queryByText("node-79")).toBeNull();

    fireEvent.scroll(container.querySelector('[data-slot="scroll-area-viewport"]')!, {
      target: { scrollTop: 3880 },
    });
    await waitFor(() => {
      expect(view.getByText("node-79")).toBeInTheDocument();
    });
    expect(view.queryByText("node-0")).toBeNull();
    expect(
      container.querySelectorAll(`[aria-label="${t("nodes.listAria")}"] [data-slot="item-title"]`),
    ).toHaveLength(14);
    rerender(<Nodes active={false} />);
    expect(view.queryByRole("list", { name: t("nodes.listAria") })).toBeNull();
    rerender(<Nodes active />);
    expect(view.getByText("node-79")).toBeInTheDocument();
    expect(container.querySelector('[data-slot="scroll-area-viewport"]')).toHaveProperty("scrollTop", 3880);
  });

  it("shows empty-state guide when no nodes", async () => {
    listNodes.mockResolvedValue([]);
    const onNavigate = vi.fn();
    const { container } = render(<Nodes onNavigate={onNavigate} />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByText(t("nodes.emptyTitle"))).toBeInTheDocument();
    });
    expect(view.queryByRole("list", { name: t("nodes.listAria") })).toBeNull();
    fireEvent.click(view.getByRole("button", { name: t("home.goToSubs") }));
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
    expect(await view.findByLabelText(t("nodes.membersAria", { group: groupName }))).toBeInTheDocument();
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

    fireEvent.click(view.getByText(t("nodes.groupType", { type: "selector" })));
    expect(await view.findByLabelText(t("nodes.membersAria", { group: "选择组" }))).toBeInTheDocument();

    fireEvent.click(view.getByText(t("nodes.groupType", { type: "selector" })));
    expect(view.queryByLabelText(t("nodes.membersAria", { group: "选择组" }))).not.toBeInTheDocument();

    fireEvent.click(rowView.getByText("→ node-a"));
    expect(await view.findByLabelText(t("nodes.membersAria", { group: "选择组" }))).toBeInTheDocument();

    fireEvent.click(rowView.getByRole("button", { name: t("nodes.test") }));
    expect(view.getByLabelText(t("nodes.membersAria", { group: "选择组" }))).toBeInTheDocument();

    fireEvent.click(rowView.getByRole("button", { name: t("nodes.select") }));
    expect(view.getByLabelText(t("nodes.membersAria", { group: "选择组" }))).toBeInTheDocument();
  });

  it("expands selector group and switches exit by clicking a member", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);

    await expandGroup(view, "选择组");
    fireEvent.click(view.getByLabelText(t("nodes.setMemberAria", { member: "node-b", group: "选择组" })));

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
    fireEvent.click(view.getByLabelText(t("nodes.setMemberAria", { member: "node-b", group: "选择组" })));

    await waitFor(() => {
      expect(setGroupSelection).toHaveBeenCalledWith("选择组", "node-b");
    });
  });

  it("expands non-selector groups as read-only member list", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);

    await expandGroup(view, "自动组");

    expect(view.queryByLabelText(t("nodes.setMemberAria", { member: "node-a", group: "自动组" }))).not.toBeInTheDocument();
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
    const panel = view.getByLabelText(t("nodes.membersAria", { group: "My Group" })).closest("[id]");
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
      within(row as HTMLElement).getByRole("button", { name: t("nodes.test") }),
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
      expect(within(row as HTMLElement).getByTestId("delay-ok")).toHaveTextContent(
        "42 ms",
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

  it("marks a failed single-node delay test on the cell without a page alert", async () => {
    testNodeDelay.mockRejectedValueOnce("probe timed out");
    const { container } = render(<Nodes />);
    const view = within(container);

    const nodeList = await view.findByRole("list", { name: t("nodes.listAria") });
    const row = within(nodeList)
      .getAllByRole("button", { name: t("nodes.test") })[0]
      .closest("[data-slot='item']");
    expect(row).not.toBeNull();
    fireEvent.click(
      within(row as HTMLElement).getByRole("button", { name: t("nodes.test") }),
    );

    await waitFor(() => {
      expect(within(row as HTMLElement).getByTestId("delay-error")).toHaveTextContent(
        t("delay.failed"),
      );
    });
    expect(view.queryByRole("alert")).not.toBeInTheDocument();
    expect(view.queryByText("probe timed out")).not.toBeInTheDocument();
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
      await view.findByText(t("nodes.noTestableGroup")),
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

    fireEvent.click(await screen.findByRole("button", { name: t("nodes.batchTest") }));

    expect(await view.findByText(t("nodes.noTestable"))).toBeInTheDocument();
    expect(testNodeDelay).not.toHaveBeenCalled();
  });

  // The batch test is shared by every platform: a collapsed group probes only
  // its current exit (shown on the group row), an expanded one every member.
  describe.each([
    ["desktop", false],
    ["phone", true],
  ])("batch delay test on %s", (_platform, embedded) => {
    beforeEach(() => {
      phoneShell.value = embedded;
      listNodes.mockResolvedValue([
        { tag: "node-a", outbound_type: "vmess", group_now: null, group_all: null },
        {
          tag: "选择组",
          outbound_type: "selector",
          group_now: "hk-1",
          group_all: ["hk-1", "hk-2", "hk-3"],
        },
      ]);
    });

    it("tests only the current exit of a collapsed group", async () => {
      const { container } = render(<Nodes embedded={embedded} />);
      const view = within(container);
      await view.findByRole("button", { name: "选择组", expanded: false });

      fireEvent.click(await screen.findByRole("button", { name: t("nodes.batchTest") }));

      await waitFor(() => {
        expect(within(rowOf(view, "选择组")).getByText("42 ms")).toBeInTheDocument();
      });
      const probed = testNodeDelay.mock.calls.map(([tag]) => tag);
      expect(probed.sort()).toEqual(["hk-1", "node-a"]);
    });

    it("tests every member of an expanded group", async () => {
      const { container } = render(<Nodes embedded={embedded} />);
      const view = within(container);
      await expandGroup(view, "选择组");

      fireEvent.click(await screen.findByRole("button", { name: t("nodes.batchTest") }));

      await waitFor(() => {
        expect(testNodeDelay).toHaveBeenCalledTimes(4);
      });
      const probed = testNodeDelay.mock.calls.map(([tag]) => tag);
      expect(probed.sort()).toEqual(["hk-1", "hk-2", "hk-3", "node-a"]);
    });
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
    expect(await view.findByLabelText(t("nodes.membersAria", { group: "选择组" }))).toBeInTheDocument();
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
      fireEvent.click(screen.getByRole("button", { name: t("common.cancel") }));
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

    fireEvent.click(await screen.findByRole("button", { name: t("nodes.batchTest") }));

    await waitFor(() => {
      expect(testNodeDelay).toHaveBeenCalledTimes(2);
    });
    expect(testNodeDelay).toHaveBeenCalledWith("node-a");
    expect(testNodeDelay).toHaveBeenCalledWith("node-b");
    expect(testNodeDelay).not.toHaveBeenCalledWith("选择组");
    expect(testNodeDelay).not.toHaveBeenCalledWith("自动组");
    await waitFor(() => {
      expect(screen.getByRole("button", { name: t("nodes.batchTest") })).toBeEnabled();
    });

    const titles = [
      ...container.querySelectorAll(
        `[aria-label="${t("nodes.listAria")}"] [data-slot="item-title"]`,
      ),
    ].map((el) =>
      (el.textContent ?? "").replace(new RegExp(t("nodes.inUse"), "g"), "").replace(/\s+/g, " ").trim(),
    );
    expect(titles).toEqual(["node-a", "node-b", "选择组", "自动组"]);

    const selectorRow = view
      .getByRole("button", { name: "选择组" })
      .closest("[data-slot='item']");
    expect(within(selectorRow as HTMLElement).getByTestId("delay-warn")).toHaveTextContent(
      "800 ms",
    );
    const autoRow = view
      .getByRole("button", { name: "自动组" })
      .closest("[data-slot='item']");
    expect(within(autoRow as HTMLElement).getByTestId("delay-ok")).toHaveTextContent(
      "10 ms",
    );
  });

  it("probes a batch with several nodes in flight at once", async () => {
    const pending = new Map<
      string,
      (value: { tag: string; delay_ms: number }) => void
    >();
    testNodeDelay.mockImplementation(
      (tag: string) =>
        new Promise((resolve) => {
          pending.set(tag, resolve);
        }),
    );
    render(<Nodes />);

    fireEvent.click(await screen.findByRole("button", { name: t("nodes.batchTest") }));

    // Both leaves are claimed before either probe returns: the run does not
    // wait for one node before starting the next.
    await waitFor(() => {
      expect(testNodeDelay).toHaveBeenCalledWith("node-a");
      expect(testNodeDelay).toHaveBeenCalledWith("node-b");
    });

    await act(async () => {
      pending.get("node-a")?.({ tag: "node-a", delay_ms: 10 });
      pending.get("node-b")?.({ tag: "node-b", delay_ms: 20 });
    });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: t("nodes.batchTest") })).toBeEnabled();
    });
  });

  function rowOf(view: ReturnType<typeof within>, tag: string): HTMLElement {
    const toggle = view.queryByRole("button", { name: tag });
    const item =
      toggle?.closest("[data-slot='item']") ??
      view.getByText(tag).closest("[data-slot='item']");
    expect(item).not.toBeNull();
    return item as HTMLElement;
  }

  function star(row: HTMLElement, favorite = false): HTMLButtonElement {
    return within(row).getByRole("button", {
      name: t(favorite ? "nodes.unfavoriteAria" : "nodes.favoriteAria"),
    }) as HTMLButtonElement;
  }

  function rowTitles(container: HTMLElement): string[] {
    return [
      ...container.querySelectorAll(
        `[aria-label="${t("nodes.listAria")}"] [data-slot="item-title"]`,
      ),
    ].map((el) =>
      (el.textContent ?? "")
        .replace(new RegExp(t("nodes.inUse"), "g"), "")
        .replace(/\s+/g, " ")
        .trim(),
    );
  }

  function useBackendNodes(nodes: Array<Record<string, unknown>>) {
    const backend = nodes.map((node) => ({ ...node }));
    listNodes.mockImplementation(async () => backend.map((node) => ({ ...node })));
    return backend;
  }

  it("shows a star on top-level rows and none on expanded members", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);
    await view.findByRole("list", { name: t("nodes.listAria") });

    expect(
      view.getAllByRole("button", { name: t("nodes.favoriteAria") }),
    ).toHaveLength(4);

    await expandGroup(view, "选择组");
    expect(
      view.getAllByRole("button", { name: t("nodes.favoriteAria") }),
    ).toHaveLength(4);
    expect(
      view.queryByLabelText(
        t("nodes.setMemberAria", { member: "node-b", group: "选择组" }),
      ),
    ).toBeInTheDocument();
  });

  it("toggles a favorite without selecting or expanding the row", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);
    await view.findByRole("list", { name: t("nodes.listAria") });

    fireEvent.click(star(rowOf(view, "选择组")));

    await waitFor(() => {
      expect(setNodeFavorite).toHaveBeenCalledWith("选择组", true);
    });
    expect(setSelectedNode).not.toHaveBeenCalled();
    expect(
      view.queryByLabelText(t("nodes.membersAria", { group: "选择组" })),
    ).not.toBeInTheDocument();
  });

  it("pins the newest favorite above earlier favorites", async () => {
    const backend = useBackendNodes([
      {
        tag: "node-a",
        outbound_type: "socks",
        group_now: null,
        group_all: null,
        favorite: true,
        favorite_rank: 0,
      },
      {
        tag: "node-b",
        outbound_type: "vmess",
        group_now: null,
        group_all: null,
        favorite: false,
        favorite_rank: null,
      },
    ]);
    setNodeFavorite.mockImplementation(async (tag: string, favorite: boolean) => {
      for (const node of backend) {
        if (node.tag === tag) {
          node.favorite = favorite;
          node.favorite_rank = favorite ? 0 : null;
        } else if (favorite && node.favorite === true) {
          node.favorite_rank = Number(node.favorite_rank ?? 0) + 1;
        }
      }
    });
    const { container } = render(<Nodes />);
    const view = within(container);
    await waitFor(() => {
      expect(rowTitles(container)).toEqual(["node-a", "node-b"]);
    });

    fireEvent.click(star(rowOf(view, "node-b")));

    await waitFor(() => {
      expect(setNodeFavorite).toHaveBeenCalledWith("node-b", true);
      expect(rowTitles(container)).toEqual(["node-b", "node-a"]);
    });
  });

  it("restores the provider position after unfavoriting", async () => {
    const backend = useBackendNodes([
      {
        tag: "node-a",
        outbound_type: "socks",
        group_now: null,
        group_all: null,
        favorite: false,
        favorite_rank: null,
      },
      {
        tag: "node-b",
        outbound_type: "vmess",
        group_now: null,
        group_all: null,
        favorite: true,
        favorite_rank: 0,
      },
    ]);
    setNodeFavorite.mockImplementation(async (tag: string, favorite: boolean) => {
      for (const node of backend) {
        if (node.tag === tag) {
          node.favorite = favorite;
          node.favorite_rank = favorite ? 0 : null;
        }
      }
    });
    const { container } = render(<Nodes />);
    const view = within(container);
    await waitFor(() => {
      expect(rowTitles(container)).toEqual(["node-b", "node-a"]);
    });

    fireEvent.click(star(rowOf(view, "node-b"), true));

    await waitFor(() => {
      expect(setNodeFavorite).toHaveBeenCalledWith("node-b", false);
      expect(rowTitles(container)).toEqual(["node-a", "node-b"]);
    });
  });

  it("keeps the optimistic star while the write is in flight and ignores a poll", async () => {
    let release: () => void = () => {};
    setNodeFavorite.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
    const { container } = render(<Nodes />);
    const view = within(container);
    await view.findByRole("list", { name: t("nodes.listAria") });
    const fetches = listNodes.mock.calls.length;

    const row = rowOf(view, "node-a");
    fireEvent.click(star(row));

    await waitFor(() => {
      expect(star(row, true)).toHaveAttribute("aria-pressed", "true");
    });
    expect(star(row, true)).toBeDisabled();
    expect(readNodesSnapshot()?.nodes[0]?.favorite).not.toBe(true);

    await act(async () => {
      stateChangedHandler?.();
    });
    expect(listNodes.mock.calls.length).toBe(fetches);

    release();
    await waitFor(() => expect(star(rowOf(view, "node-a"))).toBeEnabled());
  });

  it("sends one request for a rapid second click and disables row actions", async () => {
    let release: () => void = () => {};
    setNodeFavorite.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
    const { container } = render(<Nodes />);
    const view = within(container);
    await view.findByRole("list", { name: t("nodes.listAria") });

    const first = star(rowOf(view, "node-a"));
    fireEvent.click(first);
    fireEvent.click(star(rowOf(view, "node-b")));
    fireEvent.click(first);
    expect(setNodeFavorite).toHaveBeenCalledTimes(1);
    expect(setNodeFavorite).toHaveBeenCalledWith("node-a", true);

    for (const starButton of container.querySelectorAll("button[aria-pressed]")) {
      expect(starButton).toBeDisabled();
    }
    for (const name of [t("nodes.test"), t("nodes.select")]) {
      for (const button of view.getAllByRole("button", { name })) {
        expect(button).toBeDisabled();
      }
    }

    await act(async () => {
      release();
    });
  });

  it("ignores a star click while a selection is pending", async () => {
    let release: () => void = () => {};
    setSelectedNode.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
    const { container } = render(<Nodes />);
    const view = within(container);
    await view.findByRole("list", { name: t("nodes.listAria") });

    const row = rowOf(view, "node-b");
    fireEvent.click(
      within(row).getByRole("button", { name: t("nodes.select") }),
    );
    await waitFor(() => expect(star(row)).toBeDisabled());

    fireEvent.click(star(row));
    expect(setNodeFavorite).not.toHaveBeenCalled();
    expect(
      readNodesSnapshot()?.nodes.find((node) => node.tag === "node-b")?.favorite,
    ).not.toBe(true);

    await act(async () => {
      release();
    });
    await waitFor(() => expect(star(rowOf(view, "node-b"))).toBeEnabled());
  });

  it("keeps the single-flight guard through the post-success refresh", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);
    await view.findByRole("list", { name: t("nodes.listAria") });
    const fetchesBefore = listNodes.mock.calls.length;

    let releaseRefresh: (nodes: Array<Record<string, unknown>>) => void = () => {};
    listNodes.mockImplementation(
      () =>
        new Promise<Array<Record<string, unknown>>>((resolve) => {
          releaseRefresh = resolve;
        }),
    );

    fireEvent.click(star(rowOf(view, "node-a")));
    await waitFor(() => {
      expect(setNodeFavorite).toHaveBeenCalledWith("node-a", true);
      expect(listNodes.mock.calls.length).toBe(fetchesBefore + 1);
    });
    expect(star(rowOf(view, "node-a"), true)).toBeDisabled();

    fireEvent.click(star(rowOf(view, "node-b")));
    expect(setNodeFavorite).toHaveBeenCalledTimes(1);

    await act(async () => {
      releaseRefresh([
        {
          tag: "node-a",
          outbound_type: "socks",
          group_now: null,
          group_all: null,
          favorite: true,
          favorite_rank: 0,
        },
        {
          tag: "node-b",
          outbound_type: "vmess",
          group_now: null,
          group_all: null,
        },
      ]);
    });
    await waitFor(() => expect(star(rowOf(view, "node-b"))).toBeEnabled());
  });

  it("rolls back and shows the error when the favorite write fails", async () => {
    setNodeFavorite.mockRejectedValueOnce("favorite failed");
    const { container } = render(<Nodes />);
    const view = within(container);
    await view.findByRole("list", { name: t("nodes.listAria") });

    fireEvent.click(star(rowOf(view, "node-a")));

    expect(await view.findByText("favorite failed")).toBeInTheDocument();
    await waitFor(() => {
      expect(star(rowOf(view, "node-a"))).toHaveAttribute("aria-pressed", "false");
    });
    expect(
      readNodesSnapshot()?.nodes.find((node) => node.tag === "node-a")?.favorite,
    ).not.toBe(true);
  });

  it("keeps the star enabled while the core is stopped", async () => {
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
    await view.findByRole("list", { name: t("nodes.listAria") });

    expect(star(rowOf(view, "node-a"))).toBeEnabled();
  });

  it("renders the backend favorite flag after a subscription switch", async () => {
    useBackendNodes([
      {
        tag: "node-a",
        outbound_type: "socks",
        group_now: null,
        group_all: null,
        favorite: true,
        favorite_rank: 0,
      },
    ]);
    const { container } = render(<Nodes />);
    const view = within(container);
    await waitFor(() => {
      expect(star(rowOf(view, "node-a"), true)).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });

    listNodes.mockResolvedValue([
      {
        tag: "node-a",
        outbound_type: "socks",
        group_now: null,
        group_all: null,
        favorite: false,
        favorite_rank: null,
      },
    ]);
    await act(async () => {
      stateChangedHandler?.();
    });

    await waitFor(() => {
      expect(star(rowOf(view, "node-a"))).toHaveAttribute("aria-pressed", "false");
    });
  });
});
