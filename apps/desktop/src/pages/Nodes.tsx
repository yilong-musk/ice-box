import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  formatInvokeError,
  type NodeInfo,
} from "../api/tauri";
import { EmptyState } from "../components/EmptyState";
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

  function toggleGroup(tag: string) {
    setExpandedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(tag)) next.delete(tag);
      else next.add(tag);
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

  function groupExitCell(n: NodeInfo) {
    if (!isGroupType(n.outbound_type)) {
      return <span className="muted">—</span>;
    }
    if (n.group_now) {
      return <span className="group-now">→ {n.group_now}</span>;
    }
    return <span className="muted">代理服务未运行</span>;
  }

  function delayCell(tag: string) {
    const delay = delays[tag] ?? null;
    return (
      <span
        className={
          typeof delay === "number"
            ? "delay-badge"
            : delay === "error"
              ? "delay-error"
              : "muted"
        }
      >
        {formatDelay(delay)}
      </span>
    );
  }

  return (
    <section className="panel">
      {error && <p className="error">{error}</p>}

      {!running && nodes.length > 0 && (
        <p className="warn">
          代理服务未运行：测延迟不可用；切换出口会保存，启动后生效。
        </p>
      )}

      {nodes.length === 0 ? (
        <EmptyState
          title="暂无节点"
          description="未导入任何订阅节点。导入订阅后即可在此查看节点、测速并切换出口。"
          actionLabel="前往订阅页导入"
          onAction={() => onNavigate?.("subs")}
        />
      ) : (
        <>
          <div className="node-toolbar">
            <button
              type="button"
              disabled={!running || busy}
              onClick={() => void onBatchTest()}
            >
              批量测延迟
            </button>
            {busy && batchProgress && (
              <>
                <span className="muted batch-progress">{batchProgress}</span>
                <button type="button" onClick={onCancelBatch}>
                  取消
                </button>
              </>
            )}
            <button
              type="button"
              disabled={busy}
              className={sortByDelay ? "tab active" : "tab"}
              onClick={() => setSortByDelay((v) => !v)}
            >
              {sortByDelay ? "按延迟排序 ✓" : "按延迟排序"}
            </button>
          </div>

          <ul className="node-table" aria-label="节点列表">
            <li className="node-table-head">
              <span>名称</span>
              <span>类型</span>
              <span>出口</span>
              <span>延迟</span>
              <span>操作</span>
            </li>
            {displayNodes.map((n) => {
              const isSelected = n.tag === selectedTag;
              const members = n.group_all ?? [];
              const expandable = isGroupType(n.outbound_type) && members.length > 0;
              const expanded = expandable && expandedGroups.has(n.tag);
              const selectable = n.outbound_type === "selector";
              const membersId = groupMembersDomId(n.tag);

              return (
                <Fragment key={n.tag}>
                  <li
                    className={
                      isSelected ? "node-table-row selected" : "node-table-row"
                    }
                  >
                    {expandable ? (
                      <button
                        type="button"
                        className="group-toggle"
                        aria-label={n.tag}
                        aria-expanded={expanded}
                        aria-controls={membersId}
                        title={expanded ? "收起成员" : "展开成员"}
                        onClick={() => toggleGroup(n.tag)}
                      >
                        <span className="group-chevron" aria-hidden="true">
                          {expanded ? "▾" : "▸"}
                        </span>
                        {isSelected && (
                          <span className="node-current">● </span>
                        )}
                        <span className="node-tag" title={n.tag}>
                          {n.tag}
                        </span>
                      </button>
                    ) : (
                      <span className="node-tag" title={n.tag}>
                        {isSelected && (
                          <span className="node-current">● </span>
                        )}
                        {n.tag}
                      </span>
                    )}
                    <span className="muted">
                      {isGroupType(n.outbound_type) ? (
                        <>策略组 · {n.outbound_type}</>
                      ) : (
                        n.outbound_type
                      )}
                    </span>
                    {groupExitCell(n)}
                    {delayCell(n.tag)}
                    <span className="node-row-actions">
                      <button
                        type="button"
                        disabled={!running || busy}
                        onClick={() => void testOne(n.tag)}
                      >
                        测速
                      </button>
                      <button
                        type="button"
                        disabled={busy || isSelected}
                        onClick={() => void onSelect(n.tag)}
                      >
                        选用
                      </button>
                    </span>
                  </li>
                  {expanded && (
                    <li className="group-members-wrap" id={membersId}>
                      <ul
                        className="group-members"
                        aria-label={`${n.tag} 成员`}
                      >
                        {members.map((member) => {
                          const isExit = member === n.group_now;
                          const memberDelay = delays[member] ?? null;
                          return (
                            <li
                              key={`${n.tag}::${member}`}
                              className={
                                isExit
                                  ? "group-member-row exit"
                                  : "group-member-row"
                              }
                            >
                              {selectable ? (
                                <button
                                  type="button"
                                  className={
                                    isExit
                                      ? "group-member-select exit"
                                      : "group-member-select"
                                  }
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
                                  onClick={() =>
                                    void onGroupSelect(n.tag, member)
                                  }
                                >
                                  <span
                                    className="group-member-mark"
                                    aria-hidden="true"
                                  >
                                    {isExit ? "●" : "○"}
                                  </span>
                                  <span className="node-tag" title={member}>
                                    {member}
                                  </span>
                                </button>
                              ) : (
                                <span
                                  className={
                                    isExit
                                      ? "group-member-label exit"
                                      : "group-member-label"
                                  }
                                  title={isExit ? "当前出口" : undefined}
                                >
                                  <span
                                    className="group-member-mark"
                                    aria-hidden="true"
                                  >
                                    {isExit ? "●" : "○"}
                                  </span>
                                  <span className="node-tag" title={member}>
                                    {member}
                                  </span>
                                </span>
                              )}
                              {isExit && (
                                <span className="group-member-exit-badge">
                                  当前
                                </span>
                              )}
                              <span
                                className={
                                  typeof memberDelay === "number"
                                    ? "delay-badge"
                                    : memberDelay === "error"
                                      ? "delay-error"
                                      : "muted"
                                }
                              >
                                {formatDelay(memberDelay)}
                              </span>
                            </li>
                          );
                        })}
                      </ul>
                    </li>
                  )}
                </Fragment>
              );
            })}
          </ul>
        </>
      )}
    </section>
  );
}
