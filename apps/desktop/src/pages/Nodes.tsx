import { memo, useCallback, useEffect, useRef, useState } from "react";
import { ChevronRight } from "lucide-react";
import {
  api,
  formatInvokeError,
  type NodeInfo,
} from "../api/tauri";
import { EmptyState } from "../components/EmptyState";
import { ErrorAlert, WarnAlert } from "../components/StatusAlert";
import { badgeVariants } from "@/components/ui/badge";
import { Button, buttonVariants } from "@/components/ui/button";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemTitle,
} from "@/components/ui/item";
import { Label } from "@/components/ui/label";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";
import { useGenerationGuard } from "../lib/generationGuard";
import {
  delayTestTagsForGroup,
  delayTestTagsForList,
  delayResultTone,
  formatDelay,
  isGroupType,
  nodesEqual,
  readNodesSnapshot,
  resolveSelectedTag,
  writeNodesSnapshot,
  type DelayCell,
} from "../lib/nodes";

type Props = {
  onNavigate?: (tab: "subs") => void;
  /** When false the page stays mounted but stops polling. */
  active?: boolean;
};

/** HTML id-safe token derived from a possibly spaced group tag. */
function groupMembersDomId(tag: string): string {
  return `group-members-${encodeURIComponent(tag).replace(/%/g, "_")}`;
}

function nodeTypeLabel(n: NodeInfo): string {
  return isGroupType(n.outbound_type)
    ? `策略组 · ${n.outbound_type}`
    : n.outbound_type;
}

function delayBadge(delay: DelayCell) {
  if (typeof delay === "number") {
    const tone = delayResultTone(delay);
    return (
      <span
        className={cn(
          "font-mono text-xs tabular-nums",
          tone === "ok" && "text-ok",
          tone === "warn" && "text-warn",
          tone === "bad" && "text-destructive",
        )}
      >
        {formatDelay(delay)}
      </span>
    );
  }
  if (delay === "error") {
    return <span className="text-xs text-destructive">{formatDelay(delay)}</span>;
  }
  return (
    <span className="text-xs text-muted-foreground">{formatDelay(delay)}</span>
  );
}

const EMPTY_DELAYS: Record<string, DelayCell> = {};
/** First commit only. Strategy groups sit at the front and are expensive. */
const FIRST_PAINT = 8;
const REVEAL_BATCH = 16;
const MEMBER_FIRST_PAINT = 24;
const MEMBER_BATCH = 32;

function firstPaintCount(total: number): number {
  return Math.min(total, FIRST_PAINT);
}

function scheduleIdle(fn: () => void): () => void {
  const idle = window.requestIdleCallback?.bind(window);
  if (typeof idle === "function" && !import.meta.env.VITEST) {
    const id = idle(fn, { timeout: 200 });
    return () => window.cancelIdleCallback?.(id);
  }
  const id = window.setTimeout(fn, 0);
  return () => window.clearTimeout(id);
}

type GroupMembersProps = {
  groupTag: string;
  members: string[];
  groupNow: string | null;
  selectable: boolean;
  running: boolean;
  busy: boolean;
  delays: Record<string, DelayCell>;
  onGroupSelect: (group: string, member: string) => void;
};

const GroupMembers = memo(function GroupMembers({
  groupTag,
  members,
  groupNow,
  selectable,
  running,
  busy,
  delays,
  onGroupSelect,
}: GroupMembersProps) {
  const [shown, setShown] = useState(() =>
    Math.min(MEMBER_FIRST_PAINT, members.length),
  );
  const shownRef = useRef(shown);
  shownRef.current = shown;
  const membersRef = useRef(members);
  membersRef.current = members;

  useEffect(() => {
    const first = Math.min(MEMBER_FIRST_PAINT, members.length);
    shownRef.current = first;
    setShown(first);
    if (first >= members.length) return;
    let cancelled = false;
    let raf = 0;
    const pump = () => {
      if (cancelled) return;
      const total = membersRef.current.length;
      if (shownRef.current >= total) return;
      const next = Math.min(total, shownRef.current + MEMBER_BATCH);
      shownRef.current = next;
      setShown(next);
      if (next < total) raf = window.requestAnimationFrame(pump);
    };
    raf = window.requestAnimationFrame(pump);
    return () => {
      cancelled = true;
      window.cancelAnimationFrame(raf);
    };
  }, [members]);

  return (
    <div
      id={groupMembersDomId(groupTag)}
      aria-label={`${groupTag} 成员`}
      className="flex flex-col pl-6"
    >
      {members.slice(0, shown).map((member) => {
        const isExit = member === groupNow;
        return (
          <div
            key={`${groupTag}::${member}`}
            className={cn(
              "flex h-8 items-center gap-2 overflow-hidden px-0",
              isExit && "bg-muted/50",
            )}
          >
            {selectable ? (
              <button
                type="button"
                className={cn(
                  buttonVariants({ variant: "ghost", size: "sm" }),
                  "h-auto min-w-0 flex-1 justify-start px-0",
                )}
                disabled={busy || isExit}
                aria-current={isExit ? "true" : undefined}
                aria-label={
                  isExit
                    ? `${member}（当前出口）`
                    : `将 ${member} 设为 ${groupTag} 出口`
                }
                title={
                  isExit
                    ? "当前出口"
                    : running
                      ? "设为出口"
                      : "设为出口（保存后启动生效）"
                }
                onClick={() => onGroupSelect(groupTag, member)}
              >
                <span className="truncate">{member}</span>
              </button>
            ) : (
              <span className="min-w-0 flex-1 truncate text-xs" title={member}>
                {member}
              </span>
            )}
            {isExit ? <span className={badgeVariants()}>当前</span> : null}
            {delayBadge(delays[member] ?? null)}
          </div>
        );
      })}
    </div>
  );
});

type NodeRowProps = {
  node: NodeInfo;
  selected: boolean;
  expanded: boolean;
  running: boolean;
  busy: boolean;
  delay: DelayCell;
  delays: Record<string, DelayCell>;
  onToggleGroup: (tag: string, open: boolean) => void;
  onTest: (node: NodeInfo) => void;
  onSelect: (tag: string) => void;
  onGroupSelect: (group: string, member: string) => void;
};

const NodeRow = memo(function NodeRow({
  node,
  selected,
  expanded,
  running,
  busy,
  delay,
  delays,
  onToggleGroup,
  onTest,
  onSelect,
  onGroupSelect,
}: NodeRowProps) {
  const expandable =
    isGroupType(node.outbound_type) && (node.group_all?.length ?? 0) > 0;
  const membersId = groupMembersDomId(node.tag);
  const typeLabel = nodeTypeLabel(node);

  return (
    <div>
      <Item
        size="sm"
        variant={selected ? "muted" : "default"}
        className={cn(
          "box-border h-14 flex-nowrap overflow-hidden px-0",
          expandable && "cursor-pointer",
        )}
      >
        {expandable ? (
          <>
            <div
              role="button"
              tabIndex={0}
              aria-label={node.tag}
              aria-expanded={expanded}
              aria-controls={membersId}
              title={expanded ? "收起成员" : "展开成员"}
              className="flex min-w-0 flex-1 cursor-pointer items-center gap-2"
              onClick={() => onToggleGroup(node.tag, !expanded)}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  onToggleGroup(node.tag, !expanded);
                }
              }}
            >
              <ItemContent className="min-w-0">
                <ItemTitle className="w-full max-w-full" title={node.tag}>
                  <span className="inline-flex min-w-0 max-w-full items-center gap-2">
                    <ChevronRight
                      className={cn(
                        "size-4 shrink-0",
                        expanded && "rotate-90",
                      )}
                    />
                    <span className="truncate">{node.tag}</span>
                  </span>
                  {selected ? (
                    <Label className="shrink-0 text-ok">
                      选用中
                    </Label>
                  ) : null}
                </ItemTitle>
                <ItemDescription>{typeLabel}</ItemDescription>
              </ItemContent>
              {node.group_now ? (
                <span className={badgeVariants({ variant: "outline" })}>
                  → {node.group_now}
                </span>
              ) : (
                <span
                  className={cn(
                    badgeVariants({ variant: "ghost" }),
                    "text-muted-foreground",
                  )}
                >
                  代理服务未运行
                </span>
              )}
              {delayBadge(delay)}
            </div>
            <ItemActions className="flex-nowrap">
              <button
                type="button"
                className={buttonVariants({ size: "sm", variant: "outline" })}
                disabled={!running || busy}
                title={expanded ? "测全部成员延迟" : "测当前出口延迟"}
                onClick={() => onTest(node)}
              >
                测速
              </button>
              <button
                type="button"
                className={buttonVariants({ size: "sm" })}
                disabled={busy || selected}
                onClick={() => onSelect(node.tag)}
              >
                选用
              </button>
            </ItemActions>
          </>
        ) : (
          <>
            <ItemContent className="min-w-0">
              <ItemTitle title={node.tag}>
                <span className="truncate">{node.tag}</span>
                {selected ? (
                  <Label className="shrink-0 text-ok">
                    选用中
                  </Label>
                ) : null}
              </ItemTitle>
              <ItemDescription>{typeLabel}</ItemDescription>
            </ItemContent>
            <ItemActions className="flex-nowrap">
              {delayBadge(delay)}
              <button
                type="button"
                className={buttonVariants({ size: "sm", variant: "outline" })}
                disabled={!running || busy}
                onClick={() => onTest(node)}
              >
                测速
              </button>
              <button
                type="button"
                className={buttonVariants({ size: "sm" })}
                disabled={busy || selected}
                onClick={() => onSelect(node.tag)}
              >
                选用
              </button>
            </ItemActions>
          </>
        )}
      </Item>
      {expandable && expanded && node.group_all ? (
        <GroupMembers
          groupTag={node.tag}
          members={node.group_all}
          groupNow={node.group_now}
          selectable={node.outbound_type === "selector"}
          running={running}
          busy={busy}
          delays={delays}
          onGroupSelect={onGroupSelect}
        />
      ) : null}
    </div>
  );
});

export function Nodes({ onNavigate, active = true }: Props) {
  const { nextGeneration, isStale } = useGenerationGuard();
  const [nodes, setNodes] = useState<NodeInfo[]>(
    () => readNodesSnapshot()?.nodes ?? [],
  );
  const [selectedTag, setSelectedTag] = useState(
    () => readNodesSnapshot()?.selectedTag ?? "",
  );
  const [running, setRunning] = useState(
    () => readNodesSnapshot()?.running ?? false,
  );
  const [listReady, setListReady] = useState(
    () => (readNodesSnapshot()?.nodes.length ?? 0) > 0,
  );
  const [delays, setDelays] = useState<Record<string, DelayCell>>({});
  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(
    () => new Set(),
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [batchProgress, setBatchProgress] = useState<string | null>(null);
  const cancelRef = useRef(false);
  const mountedRef = useRef(true);
  const activeRef = useRef(active);
  const delayRunRef = useRef(0);
  const nodesRef = useRef(nodes);
  nodesRef.current = nodes;
  const expandedRef = useRef(expandedGroups);
  expandedRef.current = expandedGroups;
  const [revealCount, setRevealCount] = useState(() => {
    const total = readNodesSnapshot()?.nodes.length ?? 0;
    return firstPaintCount(total);
  });
  const revealRef = useRef(revealCount);
  revealRef.current = revealCount;

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      cancelRef.current = true;
    };
  }, []);

  const refresh = useCallback(async () => {
    const gen = nextGeneration();
    try {
      const [n, settings, status] = await Promise.all([
        api.listNodes(),
        api.getSettings(),
        api.getStatus(),
      ]);
      if (isStale(gen) || !activeRef.current) return;
      const selected = resolveSelectedTag(settings.selected_tag, n);
      const runningNow = status.core.status === "running";
      setNodes((prev) => (nodesEqual(prev, n) ? prev : n));
      setSelectedTag((prev) => (prev === selected ? prev : selected));
      setRunning((prev) => (prev === runningNow ? prev : runningNow));
      writeNodesSnapshot({
        nodes: n,
        selectedTag: selected,
        running: runningNow,
      });
      setListReady(true);
      setError(null);
    } catch (e) {
      if (!isStale(gen) && activeRef.current) {
        setError(formatInvokeError(e));
        setListReady(true);
      }
    }
  }, [isStale, nextGeneration]);

  useEffect(() => {
    activeRef.current = active;
    if (!active) {
      nextGeneration();
      return;
    }
    // Keep the snapshot visible while immediately reconciling it with the backend.
    void refresh();
    const id = window.setInterval(() => void refresh(), 5000);
    return () => {
      activeRef.current = false;
      nextGeneration();
      window.clearInterval(id);
    };
  }, [active, nextGeneration, refresh]);

  useEffect(() => {
    if (!listReady || nodes.length === 0) return;
    let cancelled = false;
    let cancelSched = () => {};
    const pump = () => {
      if (cancelled) return;
      const total = nodesRef.current.length;
      if (total === 0) {
        if (revealRef.current !== 0) {
          revealRef.current = 0;
          setRevealCount(0);
        }
        return;
      }
      if (revealRef.current >= total) return;
      const next = Math.min(total, revealRef.current + REVEAL_BATCH);
      revealRef.current = next;
      setRevealCount(next);
      if (next < total) cancelSched = scheduleIdle(pump);
    };
    cancelSched = scheduleIdle(pump);
    return () => {
      cancelled = true;
      cancelSched();
    };
  }, [listReady, nodes]);

  const setGroupOpen = useCallback((tag: string, open: boolean) => {
    setExpandedGroups((prev) => {
      const next = new Set(prev);
      if (open) next.add(tag);
      else next.delete(tag);
      return next;
    });
  }, []);

  function writeDelay(tag: string, value: DelayCell) {
    const list = nodesRef.current;
    setDelays((prev) => {
      const next = { ...prev, [tag]: value };
      for (const n of list) {
        if (isGroupType(n.outbound_type) && n.group_now === tag) {
          next[n.tag] = value;
        }
      }
      return next;
    });
  }

  function clearTestingDelays() {
    setDelays((prev) => {
      const next = { ...prev };
      let changed = false;
      for (const [tag, value] of Object.entries(next)) {
        if (value === "testing") {
          next[tag] = null;
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }

  async function testTags(tags: string[]) {
    if (tags.length === 0) return;
    const run = ++delayRunRef.current;
    nextGeneration();
    cancelRef.current = false;
    setBusy(true);
    setError(null);
    const multi = tags.length > 1;
    const isCurrent = () =>
      run === delayRunRef.current &&
      !cancelRef.current &&
      mountedRef.current;

    for (let i = 0; i < tags.length; i++) {
      if (!isCurrent()) break;
      const tag = tags[i];
      if (multi) setBatchProgress(`${i + 1} / ${tags.length}`);
      writeDelay(tag, "testing");
      try {
        const r = await api.testNodeDelay(tag);
        if (!isCurrent()) break;
        writeDelay(tag, r.delay_ms);
      } catch (e) {
        if (!isCurrent()) break;
        writeDelay(tag, "error");
        if (!multi) {
          setError(formatInvokeError(e));
          break;
        }
      }
    }

    if (!mountedRef.current || run !== delayRunRef.current) return;
    if (multi) setBatchProgress(null);
    setBusy(false);
  }

  const onTest = useCallback((node: NodeInfo) => {
    void (async () => {
      if (!isGroupType(node.outbound_type)) {
        await testTags([node.tag]);
        return;
      }
      const tags = delayTestTagsForGroup({
        expanded: expandedRef.current.has(node.tag),
        groupNow: node.group_now,
        groupAll: node.group_all,
      });
      if (tags.length === 0) {
        setError("当前策略组没有可测的出口");
        return;
      }
      await testTags(tags);
    })();
  }, []);

  const handleSelect = useCallback((tag: string) => {
    void onSelect(tag);
  }, []);

  const handleGroupSelect = useCallback((group: string, member: string) => {
    void onGroupSelect(group, member);
  }, []);

  async function onBatchTest() {
    if (!running || nodes.length === 0) return;
    const tags = delayTestTagsForList(nodes, expandedGroups);
    if (tags.length === 0) {
      setError("当前没有可测的出口");
      return;
    }
    await testTags(tags);
  }

  function onCancelBatch() {
    cancelRef.current = true;
    delayRunRef.current += 1;
    clearTestingDelays();
    setBatchProgress(null);
    setBusy(false);
  }

  async function onSelect(tag: string) {
    nextGeneration();
    setBusy(true);
    setError(null);
    try {
      await api.setSelectedNode(tag);
      if (mountedRef.current) setSelectedTag(tag);
    } catch (e) {
      if (mountedRef.current) setError(formatInvokeError(e));
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }

  async function onGroupSelect(group: string, member: string) {
    nextGeneration();
    setBusy(true);
    setError(null);
    try {
      await api.setGroupSelection(group, member);
      if (mountedRef.current) await refresh();
    } catch (e) {
      if (mountedRef.current) setError(formatInvokeError(e));
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }

  const visibleCount =
    nodes.length === 0
      ? 0
      : revealCount > 0
        ? Math.min(revealCount, nodes.length)
        : firstPaintCount(nodes.length);
  const visibleNodes = nodes.slice(0, visibleCount);

  return (
    <div className="nodes-panel flex min-h-0 flex-1 flex-col gap-3">
      {error && <ErrorAlert className="shrink-0">{error}</ErrorAlert>}

      {!running && nodes.length > 0 && (
        <WarnAlert className="shrink-0">
          代理服务未运行：测延迟不可用；切换出口会保存，启动后生效。
        </WarnAlert>
      )}

      <Card size="sm" className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <CardHeader className="shrink-0">
          <CardTitle>节点</CardTitle>
          <CardDescription>
            {batchProgress
              ? `正在测延迟 ${batchProgress}`
              : "测延迟并切换出口"}
          </CardDescription>
          <CardAction className="flex flex-wrap items-center justify-end gap-2">
            <Button
              type="button"
              size="sm"
              disabled={!running || busy || nodes.length === 0}
              onClick={() => void onBatchTest()}
            >
              批量测延迟
            </Button>
            {busy && batchProgress ? (
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={onCancelBatch}
              >
                取消
              </Button>
            ) : null}
          </CardAction>
        </CardHeader>
        <CardContent className="flex min-h-0 flex-1 flex-col overflow-hidden">
          {!listReady ? (
            <p className="my-auto text-sm text-muted-foreground">加载节点列表…</p>
          ) : nodes.length === 0 ? (
            <EmptyState
              framed={false}
              className="my-auto"
              title="暂无节点"
              description="未导入任何订阅节点。导入订阅后即可在此查看节点、测速并切换出口。"
              actionLabel="前往订阅页导入"
              onAction={() => onNavigate?.("subs")}
            />
          ) : (
            <ScrollArea
              type="scroll"
              scrollHideDelay={600}
              className="min-h-0 flex-1 overflow-hidden"
            >
              <div role="list" aria-label="节点列表" className="flex w-full flex-col">
                {visibleNodes.map((n) => {
                  const expanded =
                    isGroupType(n.outbound_type) &&
                    Boolean(n.group_all?.length) &&
                    expandedGroups.has(n.tag);
                  return (
                    <NodeRow
                      key={n.tag}
                      node={n}
                      selected={n.tag === selectedTag}
                      expanded={expanded}
                      running={running}
                      busy={busy}
                      delay={delays[n.tag] ?? null}
                      delays={expanded ? delays : EMPTY_DELAYS}
                      onToggleGroup={setGroupOpen}
                      onTest={onTest}
                      onSelect={handleSelect}
                      onGroupSelect={handleGroupSelect}
                    />
                  );
                })}
              </div>
            </ScrollArea>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
