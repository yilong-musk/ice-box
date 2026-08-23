export type DelayCell = number | "error" | "testing" | null;

export function delaySortKey(v: DelayCell): number {
  if (typeof v === "number") return v;
  return Number.POSITIVE_INFINITY;
}

export function formatDelay(v: DelayCell): string {
  if (v === null) return "—";
  if (v === "testing") return "…";
  if (v === "error") return "失败";
  return `${v} ms`;
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
