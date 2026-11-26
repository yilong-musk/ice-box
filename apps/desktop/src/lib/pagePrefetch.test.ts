// SPDX-License-Identifier: GPL-3.0-or-later

import { afterEach, describe, expect, it, vi } from "vitest";
import {
  clearNodesSnapshot,
  readNodesSnapshot,
  writeNodesSnapshot,
} from "./nodes";
import {
  readRulesSnapshot,
  readSubscriptionsSnapshot,
  writeRulesSnapshot,
} from "./pageSnapshots";

const api = vi.hoisted(() => ({
  listNodes: vi.fn(),
  getSettings: vi.fn(),
  getStatus: vi.fn(),
  listSubscriptions: vi.fn(),
  getRuleOverview: vi.fn(),
  listRules: vi.fn(),
  releaseRuleSearch: vi.fn(),
}));

vi.mock("../api/client", () => ({ api }));

import {
  prefetchNodes,
  prefetchRules,
  prefetchSubscriptions,
  schedulePagePrefetch,
} from "./pagePrefetch";

const node = (tag: string) => ({
  tag,
  outbound_type: "vless",
  group_now: null,
  group_all: null,
  favorite: false,
  favorite_rank: null,
});

const overview = { total: 1, disabled: 0, custom: 0, rule_sets: 0, types: [] };
const list = { total: 1, offset: 0, limit: 100, items: [] };

afterEach(() => {
  clearNodesSnapshot();
  for (const fn of Object.values(api)) fn.mockReset();
});

describe("prefetchNodes", () => {
  it("fills the node snapshot with the saved selection and core state", async () => {
    api.listNodes.mockResolvedValue([node("a"), node("b")]);
    api.getSettings.mockResolvedValue({ selected_tag: "b" });
    api.getStatus.mockResolvedValue({ core: { status: "running" } });

    await prefetchNodes();

    expect(readNodesSnapshot()).toMatchObject({ selectedTag: "b", running: true });
    expect(readNodesSnapshot()?.nodes.map((n) => n.tag)).toEqual(["a", "b"]);
  });

  it("leaves an existing snapshot alone", async () => {
    writeNodesSnapshot({ nodes: [node("x")], selectedTag: "x", running: false });
    await prefetchNodes();
    expect(api.listNodes).not.toHaveBeenCalled();
  });

  it("drops a list read before the snapshot was cleared", async () => {
    let resolveNodes: (value: unknown) => void = () => {};
    api.listNodes.mockReturnValue(new Promise((resolve) => (resolveNodes = resolve)));
    api.getSettings.mockResolvedValue({ selected_tag: "a" });
    api.getStatus.mockResolvedValue({ core: { status: "running" } });

    const pending = prefetchNodes();
    // Another subscription went live while the list was being read.
    clearNodesSnapshot();
    resolveNodes([node("a")]);
    await pending;

    expect(readNodesSnapshot()).toBeUndefined();
  });
});

describe("prefetchSubscriptions", () => {
  it("fills the subscription snapshot", async () => {
    api.listSubscriptions.mockResolvedValue([{ id: "s1" }]);
    await prefetchSubscriptions();
    expect(readSubscriptionsSnapshot()).toEqual([{ id: "s1" }]);
  });
});

describe("prefetchRules", () => {
  it("fills the rules snapshot and releases the backend copy", async () => {
    api.getRuleOverview.mockResolvedValue(overview);
    api.listRules.mockResolvedValue(list);
    api.releaseRuleSearch.mockResolvedValue(undefined);

    await prefetchRules(() => false);

    expect(api.listRules).toHaveBeenCalledWith(
      expect.objectContaining({ offset: 0, limit: 100, keyword: null }),
    );
    expect(readRulesSnapshot()).toEqual({ overview, list });
    expect(api.releaseRuleSearch).toHaveBeenCalledTimes(1);
  });

  it("keeps the backend copy when the Rules page opened meanwhile", async () => {
    api.getRuleOverview.mockResolvedValue(overview);
    api.listRules.mockResolvedValue(list);

    await prefetchRules(() => true);

    expect(api.releaseRuleSearch).not.toHaveBeenCalled();
  });

  it("does not replace a snapshot the page wrote first", async () => {
    const own = { overview: { ...overview, total: 7 }, list };
    api.getRuleOverview.mockImplementation(async () => {
      writeRulesSnapshot(own);
      return overview;
    });
    api.listRules.mockResolvedValue(list);
    api.releaseRuleSearch.mockResolvedValue(undefined);

    await prefetchRules(() => false);

    expect(readRulesSnapshot()).toBe(own);
  });
});

describe("schedulePagePrefetch", () => {
  it("waits, runs the tasks in order past a failure, then reports done", async () => {
    vi.useFakeTimers();
    const order: string[] = [];
    const onDone = vi.fn();
    schedulePagePrefetch({
      delayMs: 1_000,
      tasks: [
        async () => {
          order.push("a");
        },
        async () => {
          order.push("b");
          throw new Error("offline");
        },
        async () => {
          order.push("c");
        },
      ],
      onDone,
    });

    await vi.advanceTimersByTimeAsync(999);
    expect(order).toEqual([]);
    await vi.advanceTimersByTimeAsync(1);
    expect(order).toEqual(["a", "b", "c"]);
    expect(onDone).toHaveBeenCalledTimes(1);
  });

  it("runs nothing once cancelled", async () => {
    vi.useFakeTimers();
    const task = vi.fn(async () => {});
    const onDone = vi.fn();
    const cancel = schedulePagePrefetch({ delayMs: 1_000, tasks: [task], onDone });
    cancel();
    await vi.advanceTimersByTimeAsync(2_000);
    expect(task).not.toHaveBeenCalled();
    expect(onDone).not.toHaveBeenCalled();
  });
});
