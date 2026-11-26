// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Session snapshots the Subscriptions and Rules pages start from, so opening
 * a page shows its content at once instead of the empty state while its first
 * read is in flight. The pages refresh them on every successful read and the
 * launch prefetch (`pagePrefetch.ts`) fills them ahead of the first visit.
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

export function readSubscriptionsSnapshot(): SubscriptionMeta[] | undefined {
  return subscriptionsSnapshot;
}

export function writeSubscriptionsSnapshot(items: SubscriptionMeta[]): void {
  subscriptionsSnapshot = items;
}

export function clearSubscriptionsSnapshot(): void {
  subscriptionsSnapshot = undefined;
}

/** Rules overview and first unfiltered page, as last read this session. */
export type RulesSnapshot = {
  overview: RuleOverview;
  list: ListRulesResponse;
};

/** One page of rows only; the backend's full rule copy is not kept for it. */
let rulesSnapshot: RulesSnapshot | undefined;

export function readRulesSnapshot(): RulesSnapshot | undefined {
  return rulesSnapshot;
}

export function writeRulesSnapshot(snapshot: RulesSnapshot): void {
  rulesSnapshot = snapshot;
}

export function clearRulesSnapshot(): void {
  rulesSnapshot = undefined;
}
