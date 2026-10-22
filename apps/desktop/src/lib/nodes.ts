// SPDX-License-Identifier: GPL-3.0-or-later

import type { NodeInfo } from "../api/client";
import { t } from "./i18n";

export type DelayCell = number | "error" | "testing" | null;

/** Last successful node list, shared across Home and Nodes tab mounts. */
export type NodesSnapshot = {
  nodes: NodeInfo[];
  selectedTag: string;
  running: boolean;
};

type SnapshotListener = (snap: NodesSnapshot | undefined) => void;

let nodesSnapshot: NodesSnapshot | undefined;
let snapshotRevision = 0;
const snapshotListeners = new Set<SnapshotListener>();

export function readNodesSnapshot(): NodesSnapshot | undefined {
  return nodesSnapshot;
}

/** Monotonic selection revision; advances when the snapshot is cleared or
 * its selected tag changes. Content-only updates (favorites, group exits,
 * member lists, running state) notify subscribers without bumping it, so Home
 * does not mistake them for a node switch. */
export function nodesSnapshotRevision(): number {
  return snapshotRevision;
}

export function subscribeNodesSnapshot(listener: SnapshotListener): () => void {
  snapshotListeners.add(listener);
  return () => {
    snapshotListeners.delete(listener);
  };
}

function notifySnapshot(): void {
  const snap = nodesSnapshot;
  for (const listener of snapshotListeners) listener(snap);
}

function snapshotsEqual(
  left: NodesSnapshot | undefined,
  right: NodesSnapshot,
): boolean {
  return (
    left != null &&
    left.selectedTag === right.selectedTag &&
    left.running === right.running &&
    nodesEqual(left.nodes, right.nodes)
  );
}

export function writeNodesSnapshot(next: NodesSnapshot): void {
  if (snapshotsEqual(nodesSnapshot, next)) return;
  if (nodesSnapshot === undefined || nodesSnapshot.selectedTag !== next.selectedTag) {
    snapshotRevision += 1;
  }
  nodesSnapshot = next;
  notifySnapshot();
}

export function clearNodesSnapshot(): void {
  snapshotRevision += 1;
  nodesSnapshot = undefined;
  notifySnapshot();
}

/** Drop strategy-group member lists. Tags and the live exit stay. */
export function withoutGroupMembers(nodes: NodeInfo[]): NodeInfo[] {
  let changed = false;
  const next = nodes.map((node) => {
    if (node.group_all == null) return node;
    changed = true;
    return { ...node, group_all: null };
  });
  return changed ? next : nodes;
}

/**
 * Free member lists kept for the Nodes page. Does not bump the snapshot
 * revision: Home treats a revision change as a node switch.
 */
export function releaseNodeMemberLists(): void {
  const snap = nodesSnapshot;
  if (snap == null) return;
  const nodes = withoutGroupMembers(snap.nodes);
  if (nodes === snap.nodes) return;
  nodesSnapshot = { ...snap, nodes };
  notifySnapshot();
}

export function nodesEqual(a: NodeInfo[], b: NodeInfo[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    const left = a[i];
    const right = b[i];
    if (
      left.tag !== right.tag ||
      left.outbound_type !== right.outbound_type ||
      left.group_now !== right.group_now
    ) {
      return false;
    }
    if (isFavoriteNode(left) !== isFavoriteNode(right)) return false;
    if (
      isFavoriteNode(left) &&
      (left.favorite_rank ?? null) !== (right.favorite_rank ?? null)
    ) {
      return false;
    }
    const leftMembers = left.group_all;
    const rightMembers = right.group_all;
    if (leftMembers === rightMembers) continue;
    if (!leftMembers || !rightMembers || leftMembers.length !== rightMembers.length) {
      return false;
    }
    for (let j = 0; j < leftMembers.length; j++) {
      if (leftMembers[j] !== rightMembers[j]) return false;
    }
  }
  return true;
}

/** Favorites are optional: a missing flag with a rank still counts. */
export function isFavoriteNode(node: NodeInfo): boolean {
  return node.favorite === true || node.favorite_rank != null;
}

function favoriteRank(node: NodeInfo): number {
  return node.favorite_rank ?? Number.MAX_SAFE_INTEGER;
}

/**
 * Favorites first, newest first, then the provider order for everything else.
 * Returns the original array when nothing is favorited.
 */
export function orderNodesByFavorite(nodes: NodeInfo[]): NodeInfo[] {
  if (!nodes.some(isFavoriteNode)) return nodes;
  return nodes
    .map((node, index) => ({ node, index }))
    .sort((left, right) => {
      const leftFavorite = isFavoriteNode(left.node);
      const rightFavorite = isFavoriteNode(right.node);
      if (leftFavorite !== rightFavorite) return leftFavorite ? -1 : 1;
      if (!leftFavorite) return left.index - right.index;
      const rankDelta = favoriteRank(left.node) - favoriteRank(right.node);
      return rankDelta !== 0 ? rankDelta : left.index - right.index;
    })
    .map((entry) => entry.node);
}

/**
 * Optimistic favorite toggle. Favoriting puts the tag at rank 0 and shifts
 * the other favorites down; unfavoriting closes the rank gap. New node
 * objects are returned for changed rows, never mutated in place.
 */
export function applyFavoriteToNodes(
  nodes: NodeInfo[],
  tag: string,
  favorite: boolean,
): NodeInfo[] {
  if (!nodes.some((node) => node.tag === tag)) return nodes;

  const ordered = orderNodesByFavorite(nodes)
    .filter((node) => isFavoriteNode(node) && node.tag !== tag)
    .map((node) => node.tag);
  const nextFavorites = favorite ? [tag, ...ordered] : ordered;
  const ranks = new Map(nextFavorites.map((candidate, index) => [candidate, index]));
  let changed = false;
  const next = nodes.map((node) => {
    const rank = ranks.get(node.tag);
    if (node.tag === tag) {
      if (favorite) {
        const nextRank = rank ?? 0;
        if (node.favorite === true && node.favorite_rank === nextRank) return node;
        changed = true;
        return { ...node, favorite: true, favorite_rank: nextRank };
      }
      if (node.favorite !== true && node.favorite_rank == null) return node;
      changed = true;
      return { ...node, favorite: false, favorite_rank: null };
    }
    if (rank === undefined) return node;
    if (node.favorite === true && node.favorite_rank === rank) return node;
    changed = true;
    return { ...node, favorite: true, favorite_rank: rank };
  });
  return changed ? next : nodes;
}

export const STRATEGY_GROUP_TYPES = [
  "selector",
  "urltest",
  "fallback",
  "loadbalance",
] as const;

export function isGroupType(outboundType: string): boolean {
  return (STRATEGY_GROUP_TYPES as readonly string[]).includes(outboundType);
}

/** Mark matching selector members as the live exit after a leaf pick. */
export function applySelectedTagToNodes(
  nodes: NodeInfo[],
  tag: string,
): NodeInfo[] {
  let changed = false;
  const next = nodes.map((n) => {
    if (!isGroupType(n.outbound_type)) return n;
    if (!n.group_all?.includes(tag) || n.group_now === tag) return n;
    changed = true;
    return { ...n, group_now: tag };
  });
  return changed ? next : nodes;
}

/** Update one strategy group's live exit after a member pick. */
export function applyGroupNowToNodes(
  nodes: NodeInfo[],
  group: string,
  member: string,
): NodeInfo[] {
  let changed = false;
  const next = nodes.map((n) => {
    if (n.tag !== group || n.group_now === member) return n;
    changed = true;
    return { ...n, group_now: member };
  });
  return changed ? next : nodes;
}

export function formatDelay(v: DelayCell): string {
  if (v === null) return t("common.dash");
  if (v === "testing") return "…";
  if (v === "error") return t("delay.failed");
  return `${v} ms`;
}

export type DelayResultTone = "ok" | "warn" | "bad";

/** Color band for a numeric delay: <300 green, 300–999 yellow, ≥1000 red. */
export function delayResultTone(ms: number): DelayResultTone {
  if (ms < 300) return "ok";
  if (ms < 1000) return "warn";
  return "bad";
}

export type DelayProbeNode = {
  tag: string;
  outbound_type: string;
  group_now?: string | null;
  group_all?: string[] | null;
};

/** Outbound tags to probe for a strategy-group delay test. */
export function delayTestTagsForGroup(input: {
  expanded: boolean;
  groupNow: string | null | undefined;
  groupAll: string[] | null | undefined;
}): string[] {
  if (input.expanded) {
    return [...new Set(input.groupAll ?? [])];
  }
  return input.groupNow ? [input.groupNow] : [];
}

/** Leaf outbound tags to probe for a full-list delay test. */
export function delayTestTagsForList(
  nodes: DelayProbeNode[],
  expandedGroups: ReadonlySet<string>,
): string[] {
  const seen = new Set<string>();
  const tags: string[] = [];
  for (const n of nodes) {
    const next = isGroupType(n.outbound_type)
      ? delayTestTagsForGroup({
          expanded: expandedGroups.has(n.tag),
          groupNow: n.group_now,
          groupAll: n.group_all,
        })
      : [n.tag];
    for (const tag of next) {
      if (tag && !seen.has(tag)) {
        seen.add(tag);
        tags.push(tag);
      }
    }
  }
  return tags;
}

export function resolveSelectedTag(
  settingsTag: string | null | undefined,
  nodes: { tag: string }[],
): string {
  if (settingsTag && nodes.some((n) => n.tag === settingsTag)) {
    return settingsTag;
  }
  return nodes[0]?.tag ?? "";
}
