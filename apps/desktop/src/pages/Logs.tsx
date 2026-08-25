import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, formatInvokeError } from "../api/tauri";
import { useGenerationGuard } from "../lib/generationGuard";

const POLL_MS = 2000;
const VIEW_LINES = 500;
const STICK_THRESHOLD_PX = 40;

export function Logs() {
  const { nextGeneration, isStale } = useGenerationGuard();
  const [lines, setLines] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [stickToBottom, setStickToBottom] = useState(true);
  const boxRef = useRef<HTMLPreElement | null>(null);
  const lastTextRef = useRef("");

  const refresh = useCallback(async () => {
    if (document.visibilityState === "hidden") return;
    const gen = nextGeneration();
    try {
      const tail = await api.getLogView(VIEW_LINES);
      if (isStale(gen)) return;
      setError(null);
      setLines((prev) => {
        const text = tail.join("\n");
        if (text === lastTextRef.current) return prev;
        lastTextRef.current = text;
        return tail;
      });
    } catch (e) {
      if (!isStale(gen)) setError(formatInvokeError(e));
    }
  }, [isStale, nextGeneration]);

  useEffect(() => {
    nextGeneration();
    void refresh();
    const id = window.setInterval(() => void refresh(), POLL_MS);
    return () => window.clearInterval(id);
  }, [nextGeneration, refresh]);

  useLayoutEffect(() => {
    const box = boxRef.current;
    if (!box || !stickToBottom) return;
    const raf = requestAnimationFrame(() => {
      box.scrollTop = box.scrollHeight;
    });
    return () => cancelAnimationFrame(raf);
  }, [lines, stickToBottom]);

  const handleScroll = useCallback(() => {
    const box = boxRef.current;
    if (!box) return;
    const nearBottom =
      box.scrollHeight - box.scrollTop - box.clientHeight < STICK_THRESHOLD_PX;
    setStickToBottom((prev) => (prev === nearBottom ? prev : nearBottom));
  }, []);

  return (
    <section className="panel">
      <div className="actions">
        <span className="hint">
          自动刷新，显示警告 / 错误、关键事件与每连接出站节点；完整日志见数据目录
          logs/ 下的 ice-box.log 与 sing-box.log
        </span>
        <button type="button" onClick={() => void refresh()}>
          刷新
        </button>
      </div>
      {error && <p className="error">{error}</p>}
      <pre
        ref={boxRef}
        className="log-view"
        onScroll={handleScroll}
        aria-live="polite"
      >
        {lines.length === 0 ? "（空）" : lines.join("\n")}
      </pre>
    </section>
  );
}