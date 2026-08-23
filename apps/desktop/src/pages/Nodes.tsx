import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  formatInvokeError,
  type NodeInfo,
} from "../api/tauri";
import { useGenerationGuard } from "../lib/generationGuard";
import {
  delaySortKey,
  formatDelay,
  resolveSelectedTag,
  type DelayCell,
} from "../lib/nodes";

const GROUP_TYPES = ["selector", "urltest", "fallback", "loadbalance"];

export function Nodes() {
  const { nextGeneration, isStale } = useGenerationGuard();
  const [nodes, setNodes] = useState<NodeInfo[]>([]);
  const [selectedTag, setSelectedTag] = useState<string>("");
  const [running, setRunning] = useState(false);
  const [delays, setDelays] = useState<Record<string, DelayCell>>({});
  const [sortByDelay, setSortByDelay] = useState(false);
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
    if (!GROUP_TYPES.includes(n.outbound_type)) {
      return <span className="muted">—</span>;
    }
    const members = n.group_all ?? [];
    if (n.outbound_type === "selector" && members.length > 0) {
      return (
        <select
          aria-label={`${n.tag} 出口`}
          className="group-select"
          value={n.group_now ?? members[0]}
          disabled={busy}
          title={
            running
              ? "切换策略组出口"
              : "内核未运行：选择会保存，启动后生效"
          }
          onChange={(e) => void onGroupSelect(n.tag, e.target.value)}
        >
          {members.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
      );
    }
    if (n.group_now) {
      return <span className="group-now">→ {n.group_now}</span>;
    }
    return <span className="muted">内核未运行</span>;
  }

  return (
    <section className="panel">
      <h2>节点</h2>
      {error && <p className="error">{error}</p>}

      {!running && nodes.length > 0 && (
        <p className="warn">
          内核未运行：测延迟不可用；切换出口会保存，启动后生效。
        </p>
      )}

      {nodes.length === 0 ? (
        <p className="muted">暂无节点，请先在「订阅」页导入。</p>
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
              const delay = delays[n.tag] ?? null;
              const isSelected = n.tag === selectedTag;
              return (
                <li
                  key={n.tag}
                  className={isSelected ? "node-table-row selected" : "node-table-row"}
                >
                  <span className="node-tag" title={n.tag}>
                    {isSelected && <span className="node-current">● </span>}
                    {n.tag}
                  </span>
                  <span className="muted">
                    {GROUP_TYPES.includes(n.outbound_type) ? (
                      <>
                        策略组 · {n.outbound_type}
                      </>
                    ) : (
                      n.outbound_type
                    )}
                  </span>
                  {groupExitCell(n)}
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
              );
            })}
          </ul>
        </>
      )}
    </section>
  );
}
