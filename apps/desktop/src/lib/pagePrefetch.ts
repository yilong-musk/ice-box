// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Warm the pages people open first (Nodes, Subscriptions) once the app has
 * settled after launch. Each page starts from a session snapshot when one
 * exists, so reading the data here ahead of the first visit lets that visit
 * show content at once instead of an empty card while its first reads are in
 * flight. The pages still refresh on their own when they open.
 *
 * Rules is not prefetched: its first read makes the backend parse the whole
 * profile, which is not worth paying on every launch for a page that may not
 * be opened. It shows a loading line until that read lands.
 */

import { api } from "../api/client";
import {
  nodesSnapshotRevision,
  readNodesSnapshot,
  resolveSelectedTag,
  writeNodesSnapshot,
} from "./nodes";
import {
  readSubscriptionsSnapshot,
  subscriptionsSnapshotRevision,
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
  const revision = subscriptionsSnapshotRevision();
  const items = await api.listSubscriptions();
  // The page may have read a newer list meanwhile.
  if (subscriptionsSnapshotRevision() === revision) writeSubscriptionsSnapshot(items);
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
