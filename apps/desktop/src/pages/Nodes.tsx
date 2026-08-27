import { useCallback, useEffect, useRef, useState } from "react";
import { ChevronRight } from "lucide-react";
import {
  api,
  formatInvokeError,
  type NodeInfo,
} from "../api/tauri";
import { EmptyState } from "../components/EmptyState";
import { ErrorAlert, WarnAlert } from "../components/StatusAlert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemSeparator,
  ItemTitle,
} from "@/components/ui/item";
import { Toggle } from "@/components/ui/toggle";
import { cn } from "@/lib/utils";
import { useGenerationGuard } from "../lib/generationGuard";
import {
  delaySortKey,
  formatDelay,
  resolveSelectedTag,
  type DelayCell,
} from "../lib/nodes";

const GROUP_TYPES = ["selector", "urltest", "fallback", "loadbalance"];

type Props = {
  onNavigate?: (tab: "subs") => void;
};

function isGroupType(outboundType: string): boolean {
  return GROUP_TYPES.includes(outboundType);
}

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
    return (
      <Badge variant="outline" className="font-mono tabular-nums">
        {formatDelay(delay)}
      </Badge>
    );
  }
  if (delay === "error") {
    return <Badge variant="destructive">{formatDelay(delay)}</Badge>;
  }
  return (
    <Badge variant="ghost" className="text-muted-foreground">
      {formatDelay(delay)}
    </Badge>
  );
}

export function Nodes({ onNavigate }: Props) {
  const { nextGeneration, isStale } = useGenerationGuard();
  const [nodes, setNodes] = useState<NodeInfo[]>([]);
  const [selectedTag, setSelectedTag] = useState<string>("");
  const [running, setRunning] = useState(false);
  const [delays, setDelays] = useState<Record<string, DelayCell>>({});
  const [sortByDelay, setSortByDelay] = useState(false);
  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(
    () => new Set(),
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [batchProgress, setBatchProgress] = useState<string | null>(null);
  const cancelRef = useRef(false);
  const mountedRef = useRef(true);

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
      if (isStale(gen)) return;
      setNodes(n);
      setSelectedTag(resolveSelectedTag(settings.selected_tag, n));
      setRunning(status.core.status === "running");
      setError(null);
    } catch (e) {
      if (!isStale(gen)) setError(formatInvokeError(e));
    }
  }, [isStale, nextGeneration]);

  useEffect(() => {
    void refresh();
    const id = window.setInterval(() => void refresh(), 3000);
    return () => window.clearInterval(id);
  }, [refresh]);

  const displayNodes = sortByDelay
    ? [...nodes].sort(
        (a, b) => delaySortKey(delays[a.tag]) - delaySortKey(delays[b.tag]),
      )
    : nodes;

  function setGroupOpen(tag: string, open: boolean) {
    setExpandedGroups((prev) => {
      const next = new Set(prev);
      if (open) next.add(tag);
      else next.delete(tag);
      return next;
    });
  }

  async function testOne(tag: string) {
    nextGeneration();
    setBusy(true);
    setError(null);
    setDelays((prev) => ({ ...prev, [tag]: "testing" }));
    try {
      const r = await api.testNodeDelay(tag);
      if (mountedRef.current) {
        setDelays((prev) => ({ ...prev, [tag]: r.delay_ms }));
      }
    } catch (e) {
      if (mountedRef.current) {
        setDelays((prev) => ({ ...prev, [tag]: "error" }));
        setError(formatInvokeError(e));
      }
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }

  async function onBatchTest() {
    if (!running || nodes.length === 0) return;
    cancelRef.current = false;
    setBusy(true);
    setError(null);
    setSortByDelay(false);

    const tags = nodes.map((n) => n.tag);
    for (let i = 0; i < tags.length; i++) {
      if (cancelRef.current || !mountedRef.current) break;
      const tag = tags[i];
      setBatchProgress(`${i + 1} / ${tags.length}`);
      setDelays((prev) => ({ ...prev, [tag]: "testing" }));
      try {
        const r = await api.testNodeDelay(tag);
        if (cancelRef.current || !mountedRef.current) break;
        setDelays((prev) => ({ ...prev, [tag]: r.delay_ms }));
      } catch {
        if (cancelRef.current || !mountedRef.current) break;
        setDelays((prev) => ({ ...prev, [tag]: "error" }));
      }
    }

    if (!mountedRef.current) return;
    setBatchProgress(null);
    setBusy(false);
    if (!cancelRef.current) {
      setSortByDelay(true);
    }
  }

  function onCancelBatch() {
    cancelRef.current = true;
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

  function groupExitBadge(n: NodeInfo) {
    if (!isGroupType(n.outbound_type)) return null;
    if (n.group_now) {
      return <Badge variant="outline">→ {n.group_now}</Badge>;
    }
    return (
      <Badge variant="ghost" className="text-muted-foreground">
        代理服务未运行
      </Badge>
    );
  }

  function renderNodeItem(n: NodeInfo) {
    const isSelected = n.tag === selectedTag;
    const members = n.group_all ?? [];
    const expandable = isGroupType(n.outbound_type) && members.length > 0;
    const expanded = expandable && expandedGroups.has(n.tag);
    const membersId = groupMembersDomId(n.tag);

    return (
      <Item
        size="sm"
        variant={isSelected ? "muted" : "default"}
        className="px-0"
      >
        <ItemContent className="min-w-0">
          {expandable ? (
            <ItemTitle className="max-w-full">
              <CollapsibleTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-auto min-w-0 max-w-full justify-start px-0"
                  aria-label={n.tag}
                  aria-controls={membersId}
                  title={expanded ? "收起成员" : "展开成员"}
                >
                  <ChevronRight
                    className={cn(
                      "transition-transform",
                      expanded && "rotate-90",
                    )}
                  />
                  <span className="truncate">{n.tag}</span>
                </Button>
              </CollapsibleTrigger>
              {isSelected ? <Badge>选用中</Badge> : null}
            </ItemTitle>
          ) : (
            <ItemTitle title={n.tag}>
              <span className="truncate">{n.tag}</span>
              {isSelected ? <Badge>选用中</Badge> : null}
            </ItemTitle>
          )}
          <ItemDescription>{nodeTypeLabel(n)}</ItemDescription>
        </ItemContent>
        <ItemActions className="flex-wrap">
          {groupExitBadge(n)}
          {delayBadge(delays[n.tag] ?? null)}
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={!running || busy}
            onClick={() => void testOne(n.tag)}
          >
            测速
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={busy || isSelected}
            onClick={() => void onSelect(n.tag)}
          >
            选用
          </Button>
        </ItemActions>
      </Item>
    );
  }

  function renderMembers(n: NodeInfo) {
    const members = n.group_all ?? [];
    const selectable = n.outbound_type === "selector";
    const membersId = groupMembersDomId(n.tag);

    return (
      <ItemGroup
        id={membersId}
        aria-label={`${n.tag} 成员`}
        className="gap-0 pl-6"
      >
        {members.map((member) => {
          const isExit = member === n.group_now;
          return (
            <Item
              key={`${n.tag}::${member}`}
              size="xs"
              variant={isExit ? "muted" : "default"}
              className="px-0"
            >
              <ItemContent className="min-w-0">
                {selectable ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="h-auto min-w-0 max-w-full justify-start px-0"
                    disabled={busy || isExit}
                    aria-current={isExit ? "true" : undefined}
                    aria-label={
                      isExit
                        ? `${member}（当前出口）`
                        : `将 ${member} 设为 ${n.tag} 出口`
                    }
                    title={
                      isExit
                        ? "当前出口"
                        : running
                          ? "设为出口"
                          : "设为出口（保存后启动生效）"
                    }
                    onClick={() => void onGroupSelect(n.tag, member)}
                  >
                    <span className="truncate">{member}</span>
                  </Button>
                ) : (
                  <ItemTitle title={isExit ? "当前出口" : member}>
                    <span className="truncate">{member}</span>
                  </ItemTitle>
                )}
              </ItemContent>
              <ItemActions>
                {isExit ? <Badge>当前</Badge> : null}
                {delayBadge(delays[member] ?? null)}
              </ItemActions>
            </Item>
          );
        })}
      </ItemGroup>
    );
  }

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
            <Toggle
              size="sm"
              variant="outline"
              pressed={sortByDelay}
              onPressedChange={setSortByDelay}
              disabled={busy || nodes.length === 0}
              aria-label="按延迟排序"
            >
              按延迟排序
            </Toggle>
          </CardAction>
        </CardHeader>
        <CardContent className="flex min-h-0 flex-1 flex-col overflow-auto">
          {nodes.length === 0 ? (
            <EmptyState
              framed={false}
              className="my-auto"
              title="暂无节点"
              description="未导入任何订阅节点。导入订阅后即可在此查看节点、测速并切换出口。"
              actionLabel="前往订阅页导入"
              onAction={() => onNavigate?.("subs")}
            />
          ) : (
            <ItemGroup aria-label="节点列表" className="gap-0">
              {displayNodes.map((n, index) => {
                const members = n.group_all ?? [];
                const expandable =
                  isGroupType(n.outbound_type) && members.length > 0;
                const expanded =
                  expandable && expandedGroups.has(n.tag);

                if (!expandable) {
                  return (
                    <div key={n.tag}>
                      {index > 0 ? <ItemSeparator className="my-0" /> : null}
                      {renderNodeItem(n)}
                    </div>
                  );
                }

                return (
                  <div key={n.tag}>
                    {index > 0 ? <ItemSeparator className="my-0" /> : null}
                    <Collapsible
                      open={expanded}
                      onOpenChange={(open) => setGroupOpen(n.tag, open)}
                    >
                      {renderNodeItem(n)}
                      <CollapsibleContent>{renderMembers(n)}</CollapsibleContent>
                    </Collapsible>
                  </div>
                );
              })}
            </ItemGroup>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
