import type { NodeInfo } from "../api/tauri";
import { t } from "./i18n";

export type DelayCell = number | "error" | "testing" | null;

/** Last successful node list, shared across Home and Nodes tab mounts. */
export type NodesSnapshot = {
  nodes: NodeInfo[];
  selectedTag: string;
  running: boolean;
};

let nodesSnapshot: NodesSnapshot | undefined;

export function readNodesSnapshot(): NodesSnapshot | undefined {
  return nodesSnapshot;
}

export function writeNodesSnapshot(next: NodesSnapshot): void {
  nodesSnapshot = next;
}

export function clearNodesSnapshot(): void {
  nodesSnapshot = undefined;
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

export const STRATEGY_GROUP_TYPES = [
  "selector",
  "urltest",
  "fallback",
  "loadbalance",
] as const;

export function isGroupType(outboundType: string): boolean {
  return (STRATEGY_GROUP_TYPES as readonly string[]).includes(outboundType);
}

export function formatDelay(v: DelayCell): string {
  if (v === null) return "—";
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
