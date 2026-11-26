// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Session snapshots the Subscriptions and Rules pages start from, so opening
 * a page shows its content at once instead of the empty state while its first
 * read is in flight. The pages refresh them on every successful read; the
 * launch prefetch (`pagePrefetch.ts`) fills the subscription list ahead of
 * the first visit.
 *
 * Type imports only: the test setup resets these between tests, and a value
 * import of the API client there would load it before test files mock it.
 */

import type {
  ListRulesResponse,
  RuleOverview,
  SubscriptionMeta,
} from "../api/client";

/** Last subscription list read this session (metadata only, small). */
let subscriptionsSnapshot: SubscriptionMeta[] | undefined;
/** Moves on every write and clear, so a read started before either can tell
 * it has been overtaken. */
let subscriptionsRevision = 0;

export function readSubscriptionsSnapshot(): SubscriptionMeta[] | undefined {
  return subscriptionsSnapshot;
}

export function subscriptionsSnapshotRevision(): number {
  return subscriptionsRevision;
}

export function writeSubscriptionsSnapshot(items: SubscriptionMeta[]): void {
  subscriptionsRevision += 1;
  subscriptionsSnapshot = items;
}

export function clearSubscriptionsSnapshot(): void {
  subscriptionsRevision += 1;
  subscriptionsSnapshot = undefined;
}

/** Rules overview and first unfiltered page, as last read this session. */
export type RulesSnapshot = {
  overview: RuleOverview;
  list: ListRulesResponse;
};

/** One page of rows only; the backend's full rule copy is not kept for it. */
let rulesSnapshot: RulesSnapshot | undefined;
/** Counts clears. A clear means the live subscription moved or was updated,
 * so rows a mounted Rules page still holds from before it are out of date. */
let rulesClears = 0;
const rulesClearListeners = new Set<() => void>();

export function readRulesSnapshot(): RulesSnapshot | undefined {
  return rulesSnapshot;
}

export function writeRulesSnapshot(snapshot: RulesSnapshot): void {
  rulesSnapshot = snapshot;
}

export function clearRulesSnapshot(): void {
  rulesClears += 1;
  rulesSnapshot = undefined;
  for (const listener of rulesClearListeners) listener();
}

/** How many times the rules snapshot has been cleared this session. */
export function rulesSnapshotClears(): number {
  return rulesClears;
}

/** Subscribe to rules snapshot clears (for `useSyncExternalStore`). */
export function subscribeRulesSnapshotClears(listener: () => void): () => void {
  rulesClearListeners.add(listener);
  return () => {
    rulesClearListeners.delete(listener);
  };
}
