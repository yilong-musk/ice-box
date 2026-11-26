// SPDX-License-Identifier: GPL-3.0-or-later

import { afterEach, describe, expect, it, vi } from "vitest";
import type { SubscriptionMeta } from "../api/client";
import {
  clearNodesSnapshot,
  readNodesSnapshot,
  writeNodesSnapshot,
} from "./nodes";
import {
  readSubscriptionsSnapshot,
  writeSubscriptionsSnapshot,
} from "./pageSnapshots";

const api = vi.hoisted(() => ({
  listNodes: vi.fn(),
  getSettings: vi.fn(),
  getStatus: vi.fn(),
  listSubscriptions: vi.fn(),
}));

vi.mock("../api/client", () => ({ api }));

import {
  prefetchNodes,
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

  it("does not replace a list the page read meanwhile", async () => {
    const own = [{ id: "fresh" }] as unknown as SubscriptionMeta[];
    api.listSubscriptions.mockImplementation(async () => {
      writeSubscriptionsSnapshot(own);
      return [{ id: "old" }];
    });
    await prefetchSubscriptions();
    expect(readSubscriptionsSnapshot()).toBe(own);
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
