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

/** A profile without strategy groups: every node is a top-level, selectable
 * row under the injected `proxy` selector. */
const FLAT_NODES = [
  { tag: "node-a", outbound_type: "socks", group_now: null, group_all: null },
  { tag: "node-b", outbound_type: "vmess", group_now: null, group_all: null },
];

/** A group section header or a node card: the units rows used to be. */
const ROW = '[data-testid="group-header"], [data-testid="node-card"]';
/** Header and card titles in document order. */
const TITLES = '[data-testid="group-header-title"], [data-testid="node-card-title"]';

function useFlatProfile() {
  listNodes.mockImplementation(async () => FLAT_NODES.map((node) => ({ ...node })));
}

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
    // Expanded groups are remembered across mounts; start every test collapsed.
    window.localStorage.removeItem("ice-box.nodes.expanded-groups");
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
    // The backend returns the selected node after the pick.
    setGroupSelection.mockImplementation(async () => ({
      selected_tag: (await getSettings()).selected_tag,
    }));
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
    useFlatProfile();
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

  it("keeps a per-card test on the phone and shortens the batch test", async () => {
    phoneShell.value = true;
    useFlatProfile();
    const { container } = render(<Nodes embedded />);
    const view = within(container);
    await view.findByRole("list", { name: t("nodes.listAria") });

    expect(
      view.getByRole("button", { name: t("nodes.testNodeAria", { node: "node-a" }) }),
    ).toBeInTheDocument();
    expect(
      view.getByRole("button", { name: t("nodes.selectNodeAria", { node: "node-b" }) }),
    ).toBeInTheDocument();
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

    await view.findAllByText("选择组");
    expect(view.queryByText(t("nodes.notRunningWarn"))).not.toBeInTheDocument();
  });

  it("publishes the selected node to the shared snapshot", async () => {
    useFlatProfile();
    const { container } = render(<Nodes />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByRole("list", { name: t("nodes.listAria") })).toBeInTheDocument();
    });
    fireEvent.click(
      view.getByRole("button", { name: t("nodes.selectNodeAria", { node: "node-b" }) }),
    );
    await waitFor(() => {
      expect(setSelectedNode).toHaveBeenCalledWith("node-b");
    });
    await waitFor(() => {
      expect(readNodesSnapshot()?.selectedTag).toBe("node-b");
    });
  });

  it("marks the node in use with an icon instead of a text label", async () => {
    useFlatProfile();
    const { container } = render(<Nodes />);
    const view = within(container);
    await view.findByRole("list", { name: t("nodes.listAria") });
    const current = view.getByRole("button", {
      name: t("nodes.memberCurrentAria", { member: "node-a" }),
    });
    expect(current).toBeDisabled();
    expect(current).toHaveAttribute("aria-current", "true");
    expect(current).toHaveTextContent("");
    expect(current.closest(ROW)).toHaveAttribute("data-current", "true");
    // The check is an announced icon, not an "in use" text label.
    expect(view.getByRole("img", { name: t("nodes.inUse") })).toBeInTheDocument();
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
    useFlatProfile();
    const { container, rerender } = render(<Nodes active />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByRole("list", { name: t("nodes.listAria") })).toBeInTheDocument();
    });
    fireEvent.click(
      view.getByRole("button", { name: t("nodes.selectNodeAria", { node: "node-b" }) }),
    );
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
        new Promise<{ selected_tag: string | null }>((resolve) => {
          resolveGroup = () => resolve({ selected_tag: "node-a" });
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

  it("publishes the selected node a main-group pick moved, even when hidden", async () => {
    let release: () => void = () => {};
    setGroupSelection.mockImplementation(
      () =>
        new Promise<{ selected_tag: string | null }>((resolve) => {
          // The backend moves `selected_tag` with a main-group pick and
          // returns it; the page must not need another settings read.
          release = () => resolve({ selected_tag: "node-b" });
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
    expect(readNodesSnapshot()?.selectedTag).toBe("node-a");

    // Hidden: the page skips its refresh, so the snapshot must carry the move.
    rerender(<Nodes active={false} />);
    const settingsReads = getSettings.mock.calls.length;
    await act(async () => {
      release();
    });
    await waitFor(() => {
      expect(readNodesSnapshot()?.selectedTag).toBe("node-b");
    });
    expect(getSettings.mock.calls.length).toBe(settingsReads);
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

  it("drops delay results and another lineup's expanded groups when the lineup changes", async () => {
    testNodeDelay.mockRejectedValue("probe timed out");
    // node-a stays outside every group so it keeps a top-level row.
    listNodes.mockResolvedValue([
      { tag: "node-a", outbound_type: "socks", group_now: null, group_all: null },
      { tag: "node-b", outbound_type: "vmess", group_now: null, group_all: null },
      { tag: "node-d", outbound_type: "vmess", group_now: null, group_all: null },
      {
        tag: "选择组",
        outbound_type: "selector",
        group_now: "node-b",
        group_all: ["node-b", "node-d"],
      },
      {
        tag: "自动组",
        outbound_type: "urltest",
        group_now: "node-b",
        group_all: ["node-b", "node-d"],
      },
    ]);
    const { container } = render(<Nodes />);
    const view = within(container);
    const nodeList = await view.findByRole("list", { name: t("nodes.listAria") });
    const rowOf = (tag: string) =>
      within(nodeList).getAllByText(tag)[0].closest(ROW) as HTMLElement;
    fireEvent.click(within(rowOf("node-a")).getByRole("button", { name: t("nodes.testNodeAria", { node: "node-a" }) }));
    await waitFor(() => {
      expect(within(rowOf("node-a")).getByTestId("delay-error")).toBeInTheDocument();
    });
    await expandGroup(view, "选择组");

    // A refresh with the same lineup (only a group exit moved) keeps both.
    listNodes.mockResolvedValue([
      { tag: "node-a", outbound_type: "socks", group_now: null, group_all: null },
      { tag: "node-b", outbound_type: "vmess", group_now: null, group_all: null },
      { tag: "node-d", outbound_type: "vmess", group_now: null, group_all: null },
      {
        tag: "选择组",
        outbound_type: "selector",
        group_now: "node-b",
        group_all: ["node-b", "node-d"],
      },
      {
        tag: "自动组",
        outbound_type: "urltest",
        group_now: "node-d",
        group_all: ["node-b", "node-d"],
      },
    ]);
    await act(async () => {
      stateChangedHandler?.();
    });
    await waitFor(() => {
      expect(readNodesSnapshot()?.nodes[4]?.group_now).toBe("node-d");
    });
    expect(within(rowOf("node-a")).getByTestId("delay-error")).toBeInTheDocument();
    expect(view.getByRole("button", { name: "选择组", expanded: true })).toBeInTheDocument();

    // Another subscription reuses the tags: the old results no longer apply.
    listNodes.mockResolvedValue([
      { tag: "node-a", outbound_type: "socks", group_now: null, group_all: null },
      { tag: "node-c", outbound_type: "vmess", group_now: null, group_all: null },
      { tag: "node-e", outbound_type: "vmess", group_now: null, group_all: null },
      {
        tag: "选择组",
        outbound_type: "selector",
        group_now: "node-e",
        group_all: ["node-e"],
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
    useFlatProfile();
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
      within(nodeList).getAllByText(tag)[0].closest(ROW) as HTMLElement;
    fireEvent.click(within(rowOf("node-a")).getByRole("button", { name: t("nodes.testNodeAria", { node: "node-a" }) }));
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
        within(rowOf("node-a")).getByRole("button", { name: t("nodes.testNodeAria", { node: "node-a" }) }),
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
      target: { scrollTop: 4850 },
    });
    await waitFor(() => {
      expect(view.getByText("node-79")).toBeInTheDocument();
    });
    const titles = container.querySelectorAll(
      `[aria-label="${t("nodes.listAria")}"] [data-testid="node-card-title"]`,
    );
    expect(titles.length).toBeLessThanOrEqual(16);
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
    fireEvent.scroll(viewport, { target: { scrollTop: 5600 } });
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
      `[aria-label="${t("nodes.listAria")}"] [data-testid="node-card-title"]`,
    );
    expect(firstPaint.length).toBeGreaterThan(0);
    expect(firstPaint.length).toBeLessThanOrEqual(16);
    expect(view.queryByText("node-79")).toBeNull();

    fireEvent.scroll(container.querySelector('[data-slot="scroll-area-viewport"]')!, {
      target: { scrollTop: 4800 },
    });
    await waitFor(() => {
      expect(view.getByText("node-79")).toBeInTheDocument();
    });
    expect(view.queryByText("node-0")).toBeNull();
    const lastPaint = container.querySelectorAll(
      `[aria-label="${t("nodes.listAria")}"] [data-testid="node-card-title"]`,
    );
    expect(lastPaint.length).toBeGreaterThan(0);
    expect(lastPaint.length).toBeLessThanOrEqual(16);
    rerender(<Nodes active={false} />);
    expect(view.queryByRole("list", { name: t("nodes.listAria") })).toBeNull();
    rerender(<Nodes active />);
    expect(view.getByText("node-79")).toBeInTheDocument();
    expect(container.querySelector('[data-slot="scroll-area-viewport"]')).toHaveProperty("scrollTop", 4800);
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

  it("toggles a strategy group from its header except the test button", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);

    const toggle = await view.findByRole("button", {
      name: "选择组",
      expanded: false,
    });
    const row = toggle.closest(ROW);
    expect(row).not.toBeNull();
    const rowView = within(row as HTMLElement);

    fireEvent.click(rowView.getByText("selector"));
    expect(await view.findByLabelText(t("nodes.membersAria", { group: "选择组" }))).toBeInTheDocument();

    fireEvent.click(rowView.getByText("selector"));
    expect(view.queryByLabelText(t("nodes.membersAria", { group: "选择组" }))).not.toBeInTheDocument();

    fireEvent.click(rowView.getByText("→ node-a"));
    expect(await view.findByLabelText(t("nodes.membersAria", { group: "选择组" }))).toBeInTheDocument();

    fireEvent.click(rowView.getByRole("button", { name: t("nodes.test") }));
    expect(view.getByLabelText(t("nodes.membersAria", { group: "选择组" }))).toBeInTheDocument();
  });

  it("expands selector group and switches exit by clicking a member", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);

    await expandGroup(view, "选择组");
    // The current exit member is announced, not only shown as a check icon.
    expect(view.getByRole("img", { name: t("nodes.inUse") })).toBeInTheDocument();
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
    const row = toggle.closest(ROW);
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
    const row = toggle.closest(ROW);
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
      .closest(ROW);
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

    const titles = rowTitles(container);
    // Grouped profile: only the groups are top-level; their members are inside.
    expect(titles).toEqual(["选择组", "自动组"]);

    const selectorRow = view
      .getByRole("button", { name: "选择组" })
      .closest(ROW);
    expect(within(selectorRow as HTMLElement).getByTestId("delay-warn")).toHaveTextContent(
      "800 ms",
    );
    const autoRow = view
      .getByRole("button", { name: "自动组" })
      .closest(ROW);
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
    const toggle = view.queryByRole("button", { name: tag, expanded: undefined });
    const item =
      (toggle?.getAttribute("aria-expanded") != null ? toggle.closest(ROW) : null) ??
      view
        .getAllByTestId("node-card-title")
        .find((title: HTMLElement) => title.textContent === tag)
        ?.closest(ROW);
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
      ...container.querySelectorAll(`[aria-label="${t("nodes.listAria")}"] :is(${TITLES})`),
    ].map((el) => (el.textContent ?? "").replace(/\s+/g, " ").trim());
  }

  function useBackendNodes(nodes: Array<Record<string, unknown>>) {
    const backend = nodes.map((node) => ({ ...node }));
    listNodes.mockImplementation(async () => backend.map((node) => ({ ...node })));
    return backend;
  }

  it("stars cards, not group headers", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);
    await view.findByRole("list", { name: t("nodes.listAria") });

    // Collapsed groups show only their headers, which carry no star.
    expect(view.queryAllByRole("button", { name: t("nodes.favoriteAria") })).toHaveLength(0);

    await expandGroup(view, "选择组");
    const members = view.getByLabelText(t("nodes.membersAria", { group: "选择组" }));
    expect(
      within(members).getAllByRole("button", { name: t("nodes.favoriteAria") }),
    ).toHaveLength(2);
    expect(
      view.queryByLabelText(
        t("nodes.setMemberAria", { member: "node-b", group: "选择组" }),
      ),
    ).toBeInTheDocument();
  });

  it("toggles a favorite without picking the card or collapsing the group", async () => {
    const { container } = render(<Nodes />);
    const view = within(container);
    await expandGroup(view, "选择组");

    fireEvent.click(star(rowOf(view, "node-b")));

    await waitFor(() => {
      expect(setNodeFavorite).toHaveBeenCalledWith("node-b", true);
    });
    expect(setGroupSelection).not.toHaveBeenCalled();
    expect(setSelectedNode).not.toHaveBeenCalled();
    expect(
      view.getByLabelText(t("nodes.membersAria", { group: "选择组" })),
    ).toBeInTheDocument();
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
    useFlatProfile();
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

  it("disables the section test while a favorite write is in flight", async () => {
    let release: () => void = () => {};
    setNodeFavorite.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
    const { container } = render(<Nodes />);
    const view = within(container);
    await expandGroup(view, "选择组");
    const sectionTest = () =>
      within(rowOf(view, "选择组")).getByRole("button", { name: t("nodes.test") });
    expect(sectionTest()).toBeEnabled();

    const members = view.getByLabelText(t("nodes.membersAria", { group: "选择组" }));
    fireEvent.click(star(within(members).getByText("node-a").closest(ROW) as HTMLElement));
    // A delay test now would race the favorite's post-success refresh.
    await waitFor(() => expect(sectionTest()).toBeDisabled());

    await act(async () => {
      release();
    });
    await waitFor(() => expect(sectionTest()).toBeEnabled());
  });

  it("sends one request for a rapid second click and disables row actions", async () => {
    useFlatProfile();
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
    for (const name of [
      t("nodes.testNodeAria", { node: "node-a" }),
      t("nodes.testNodeAria", { node: "node-b" }),
      t("nodes.selectNodeAria", { node: "node-b" }),
    ]) {
      expect(view.getByRole("button", { name })).toBeDisabled();
    }

    await act(async () => {
      release();
    });
  });

  it("ignores a star click while a selection is pending", async () => {
    useFlatProfile();
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
      within(row).getByRole("button", { name: t("nodes.selectNodeAria", { node: "node-b" }) }),
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
    useFlatProfile();
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
    useFlatProfile();
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
    useFlatProfile();
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

  describe("profile structure", () => {
    const STRUCTURED = [
      { tag: "节点选择", outbound_type: "selector", group_now: "自动选择",
        group_all: ["自动选择", "hk-1", "jp-1"] },
      { tag: "自动选择", outbound_type: "urltest", group_now: "jp-1",
        group_all: ["hk-1", "jp-1"] },
      { tag: "hk-1", outbound_type: "vless", group_now: null, group_all: null },
      { tag: "jp-1", outbound_type: "vless", group_now: null, group_all: null },
      { tag: "solo", outbound_type: "vless", group_now: null, group_all: null },
    ];

    it("lists groups and standalone nodes, keeping members inside their groups", async () => {
      useBackendNodes(STRUCTURED);
      const { container } = render(<Nodes />);
      const view = within(container);
      await waitFor(() => {
        expect(rowTitles(container)).toEqual([
          "节点选择",
          "自动选择",
          t("nodes.standalone"),
          "solo",
        ]);
      });
      // No group holds the standalone node and grouped profiles have no flat
      // selector, so it can be tested and starred but not selected.
      const solo = rowOf(view, "solo");
      expect(
        within(solo).getByRole("button", { name: t("nodes.testNodeAria", { node: "solo" }) }),
      ).toBeInTheDocument();
      expect(within(solo).queryByRole("button", { name: t("nodes.selectNodeAria", { node: "solo" }) })).toBeNull();
      expect(within(solo).getByRole("button", { name: t("nodes.favoriteAria") })).toBeInTheDocument();
    });

    it("shows where a nested group member leads", async () => {
      useBackendNodes(STRUCTURED);
      const { container } = render(<Nodes />);
      const view = within(container);
      await expandGroup(view, "节点选择");
      const members = view.getByLabelText(t("nodes.membersAria", { group: "节点选择" }));
      const nested = within(members)
        .getByRole("button", { name: t("nodes.memberCurrentAria", { member: "自动选择" }) })
        .closest(ROW) as HTMLElement;
      expect(within(nested).getByText("→ jp-1")).toBeInTheDocument();
    });

    it("puts favorited members first inside their group", async () => {
      const backend = useBackendNodes(STRUCTURED);
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
      await expandGroup(view, "自动选择");
      const members = () =>
        view.getByLabelText(t("nodes.membersAria", { group: "自动选择" }));
      const memberNames = () =>
        [...members().querySelectorAll('[data-testid="node-card-title"]')].map(
          (el) => el.textContent,
        );
      expect(memberNames()).toEqual(["hk-1", "jp-1"]);

      const jpRow = within(members()).getByText("jp-1").closest(ROW) as HTMLElement;
      fireEvent.click(within(jpRow).getByRole("button", { name: t("nodes.favoriteAria") }));

      await waitFor(() => {
        expect(setNodeFavorite).toHaveBeenCalledWith("jp-1", true);
        expect(memberNames()).toEqual(["jp-1", "hk-1"]);
      });
      // The group order itself does not move.
      expect(
        rowTitles(container).filter((title) => !["hk-1", "jp-1"].includes(title)),
      ).toEqual(["节点选择", "自动选择", t("nodes.standalone"), "solo"]);
    });

    it("starts collapsed and remembers expanded groups across mounts", async () => {
      useBackendNodes(STRUCTURED);
      const first = render(<Nodes />);
      const view = within(first.container);
      expect(
        await view.findByRole("button", { name: "节点选择", expanded: false }),
      ).toBeInTheDocument();
      expect(view.getByRole("button", { name: "自动选择", expanded: false })).toBeInTheDocument();
      await expandGroup(view, "自动选择");
      first.unmount();

      clearNodesSnapshot();
      const second = render(<Nodes />);
      const again = within(second.container);
      expect(
        await again.findByRole("button", { name: "自动选择", expanded: true }),
      ).toBeInTheDocument();
      expect(again.getByRole("button", { name: "节点选择", expanded: false })).toBeInTheDocument();
    });

    it("keeps expanded groups per subscription", async () => {
      useBackendNodes(STRUCTURED);
      const { container } = render(<Nodes />);
      const view = within(container);
      await expandGroup(view, "自动选择");

      // Another subscription with other groups starts collapsed.
      useBackendNodes([
        { tag: "Proxies", outbound_type: "selector", group_now: "us-1", group_all: ["us-1"] },
        { tag: "us-1", outbound_type: "vless", group_now: null, group_all: null },
      ]);
      await act(async () => {
        stateChangedHandler?.();
      });
      expect(
        await view.findByRole("button", { name: "Proxies", expanded: false }),
      ).toBeInTheDocument();

      // Switching back restores what was open there.
      useBackendNodes(STRUCTURED);
      await act(async () => {
        stateChangedHandler?.();
      });
      expect(
        await view.findByRole("button", { name: "自动选择", expanded: true }),
      ).toBeInTheDocument();
    });
  });

  describe("delay result expiry", () => {
    afterEach(() => {
      vi.useRealTimers();
    });

    it("clears a delay result three minutes after it was measured", async () => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
      useFlatProfile();
      const { container } = render(<Nodes />);
      const view = within(container);
      await view.findByRole("list", { name: t("nodes.listAria") });

      fireEvent.click(
        view.getByRole("button", { name: t("nodes.testNodeAria", { node: "node-a" }) }),
      );
      await waitFor(() => {
        expect(within(rowOf(view, "node-a")).getByText("42 ms")).toBeInTheDocument();
      });

      // Still shown just before the deadline.
      await act(async () => {
        vi.advanceTimersByTime(3 * 60_000 - 1_000);
      });
      expect(within(rowOf(view, "node-a")).getByText("42 ms")).toBeInTheDocument();

      await act(async () => {
        vi.advanceTimersByTime(2_000);
      });
      expect(within(rowOf(view, "node-a")).queryByText("42 ms")).toBeNull();
      // Back to the untested state: the card offers a fresh test.
      expect(
        view.getByRole("button", { name: t("nodes.testNodeAria", { node: "node-a" }) }),
      ).toBeEnabled();
    });

    it("still clears a result when the expiry timer fires early", async () => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
      useFlatProfile();
      const { container } = render(<Nodes />);
      const view = within(container);
      await view.findByRole("list", { name: t("nodes.listAria") });

      fireEvent.click(
        view.getByRole("button", { name: t("nodes.testNodeAria", { node: "node-a" }) }),
      );
      await waitFor(() => {
        expect(within(rowOf(view, "node-a")).getByText("42 ms")).toBeInTheDocument();
      });

      // The wall clock steps back (NTP, sleep/wake): the timer fires on time
      // but finds nothing old enough yet.
      vi.setSystemTime(Date.now() - 10_000);
      await act(async () => {
        vi.advanceTimersByTime(3 * 60_000 + 500);
      });
      expect(within(rowOf(view, "node-a")).getByText("42 ms")).toBeInTheDocument();

      // It re-arms instead of leaving the result up for good.
      await act(async () => {
        vi.advanceTimersByTime(11_000);
      });
      expect(within(rowOf(view, "node-a")).queryByText("42 ms")).toBeNull();
    });

    it("restarts the three minutes when a node is tested again", async () => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
      useFlatProfile();
      testNodeDelay.mockImplementationOnce(async (tag: string) => ({ tag, delay_ms: 42 }));
      testNodeDelay.mockImplementationOnce(async (tag: string) => ({ tag, delay_ms: 77 }));
      const { container } = render(<Nodes />);
      const view = within(container);
      await view.findByRole("list", { name: t("nodes.listAria") });
      const testButton = () =>
        view.getByRole("button", { name: t("nodes.testNodeAria", { node: "node-a" }) });

      fireEvent.click(testButton());
      await waitFor(() => {
        expect(within(rowOf(view, "node-a")).getByText("42 ms")).toBeInTheDocument();
      });
      await act(async () => {
        vi.advanceTimersByTime(2 * 60_000);
      });
      fireEvent.click(testButton());
      await waitFor(() => {
        expect(within(rowOf(view, "node-a")).getByText("77 ms")).toBeInTheDocument();
      });

      // Past the first result's deadline, the newer result stays.
      await act(async () => {
        vi.advanceTimersByTime(90_000);
      });
      expect(within(rowOf(view, "node-a")).getByText("77 ms")).toBeInTheDocument();
      await act(async () => {
        vi.advanceTimersByTime(91_000);
      });
      expect(within(rowOf(view, "node-a")).queryByText("77 ms")).toBeNull();
    });
  });

  describe("group delays follow the current exit", () => {
    const NESTED = [
      { tag: "节点选择", outbound_type: "selector", group_now: "自动选择",
        group_all: ["自动选择", "hk-1", "jp-1"] },
      { tag: "自动选择", outbound_type: "urltest", group_now: "jp-1",
        group_all: ["hk-1", "jp-1"] },
      { tag: "hk-1", outbound_type: "vless", group_now: null, group_all: null },
      { tag: "jp-1", outbound_type: "vless", group_now: null, group_all: null },
    ];

    it("shows the new exit's delay in the header right after a switch", async () => {
      testNodeDelay.mockImplementation(async (tag: string) => ({
        tag,
        delay_ms: tag === "node-a" ? 42 : 610,
      }));
      // The backend keeps the switch, so the follow-up refresh agrees with it.
      const backend = useBackendNodes(await listNodes());
      setGroupSelection.mockImplementation(async (group: string, member: string) => {
        for (const node of backend) if (node.tag === group) node.group_now = member;
        return { selected_tag: "node-a" };
      });
      const { container } = render(<Nodes />);
      const view = within(container);
      await expandGroup(view, "选择组");
      // Test both members; the header shows the current exit, node-a.
      await clickGroupDelayTest(view, "选择组", true);
      await waitFor(() => {
        expect(within(rowOf(view, "选择组")).getByText("42 ms")).toBeInTheDocument();
      });

      fireEvent.click(
        view.getByLabelText(t("nodes.setMemberAria", { member: "node-b", group: "选择组" })),
      );
      await waitFor(() => {
        expect(within(rowOf(view, "选择组")).getByText("610 ms")).toBeInTheDocument();
      });
      expect(within(rowOf(view, "选择组")).queryByText("42 ms")).toBeNull();
    });

    it("tests and shows the node a nested group leads to", async () => {
      useBackendNodes(NESTED);
      testNodeDelay.mockImplementation(async (tag: string) => ({ tag, delay_ms: 88 }));
      const { container } = render(<Nodes />);
      const view = within(container);
      await view.findByRole("button", { name: "节点选择", expanded: false });

      // The collapsed main group probes the node behind 自动选择, not the group.
      await clickGroupDelayTest(view, "节点选择", false);
      await waitFor(() => {
        expect(within(rowOf(view, "节点选择")).getByText("88 ms")).toBeInTheDocument();
      });
      expect(testNodeDelay.mock.calls.map(([tag]) => tag)).toEqual(["jp-1"]);
      // 自动选择 leads to the same node, so its header shows the same result.
      expect(within(rowOf(view, "自动选择")).getByText("88 ms")).toBeInTheDocument();

      await expandGroup(view, "节点选择");
      const members = view.getByLabelText(t("nodes.membersAria", { group: "节点选择" }));
      const nested = within(members)
        .getByRole("button", { name: t("nodes.memberCurrentAria", { member: "自动选择" }) })
        .closest(ROW) as HTMLElement;
      expect(within(nested).getByText("88 ms")).toBeInTheDocument();
    });
  });
});
