// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Warm the pages people open first (Nodes, Subscriptions, Rules) once the app
 * has settled after launch. Each page starts from a session snapshot when one
 * exists, so reading the data here ahead of the first visit lets that visit
 * show content at once instead of an empty card while its first reads are in
 * flight. The pages still refresh on their own when they open.
 */

import { api } from "../api/client";
import {
  nodesSnapshotRevision,
  readNodesSnapshot,
  resolveSelectedTag,
  writeNodesSnapshot,
} from "./nodes";
import { defaultRulesRequest } from "./rules";
import {
  readRulesSnapshot,
  readSubscriptionsSnapshot,
  writeRulesSnapshot,
  writeSubscriptionsSnapshot,
} from "./pageSnapshots";

/** Fill the node snapshot the Nodes page mounts from, unless it has one. */
export async function prefetchNodes(): Promise<void> {
  if (readNodesSnapshot()) return;
  // A subscription switch clears the snapshot and moves the revision; a list
  // read before that must not be published after it.
  const revision = nodesSnapshotRevision();
  const [nodes, settings, status] = await Promise.all([
    api.listNodes(),
    api.getSettings(),
    api.getStatus(),
  ]);
  if (readNodesSnapshot() || nodesSnapshotRevision() !== revision) return;
  writeNodesSnapshot({
    nodes,
    selectedTag: resolveSelectedTag(settings.selected_tag, nodes),
    running: status?.core.status === "running",
  });
}

/** Fill the subscription list the Subscriptions page mounts from. */
export async function prefetchSubscriptions(): Promise<void> {
  if (readSubscriptionsSnapshot()) return;
  const items = await api.listSubscriptions();
  // The page may have read a newer list meanwhile.
  if (!readSubscriptionsSnapshot()) writeSubscriptionsSnapshot(items);
}

/**
 * Fill the Rules overview and first page. Reading them makes the backend load
 * every subscription rule; that copy is meant to live only while the Rules
 * page is open, so it is released again unless the page opened meanwhile.
 */
export async function prefetchRules(rulesOpen: () => boolean): Promise<void> {
  if (readRulesSnapshot()) return;
  try {
    const [overview, list] = await Promise.all([
      api.getRuleOverview(),
      api.listRules(defaultRulesRequest()),
    ]);
    if (!readRulesSnapshot()) writeRulesSnapshot({ overview, list });
  } finally {
    if (!rulesOpen() && typeof api.releaseRuleSearch === "function") {
      void api.releaseRuleSearch().catch(() => {});
    }
  }
}

type IdleWindow = Window & {
  requestIdleCallback?: (cb: () => void, opts?: { timeout: number }) => number;
  cancelIdleCallback?: (id: number) => void;
};

/**
 * After `delayMs`, once the main thread is idle, run `tasks` one at a time (so
 * they do not compete with each other for the backend's worker threads), then
 * call `onDone`. A failed task is skipped. Returns a cancel function.
 */
export function schedulePagePrefetch({
  delayMs,
  tasks,
  onDone,
}: {
  delayMs: number;
  tasks: ReadonlyArray<() => Promise<void>>;
  onDone?: () => void;
}): () => void {
  const win = window as IdleWindow;
  let cancelled = false;
  let idleId: number | null = null;
  const run = async () => {
    for (const task of tasks) {
      if (cancelled) return;
      try {
        await task();
      } catch {
        // Best-effort: the page reads the data itself when it opens.
      }
    }
    if (!cancelled) onDone?.();
  };
  const timer = window.setTimeout(() => {
    if (typeof win.requestIdleCallback === "function") {
      idleId = win.requestIdleCallback(() => void run(), { timeout: 2_000 });
    } else {
      void run();
    }
  }, delayMs);
  return () => {
    cancelled = true;
    window.clearTimeout(timer);
    if (idleId !== null) win.cancelIdleCallback?.(idleId);
  };
}
