import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, formatInvokeError } from "../api/tauri";
import { useGenerationGuard } from "../lib/generationGuard";
import { ErrorAlert } from "../components/StatusAlert";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";

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
    <div className="logs-panel flex min-h-0 flex-1 flex-col overflow-hidden gap-3">
      {error && <ErrorAlert className="shrink-0">{error}</ErrorAlert>}
      <Card size="sm" className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <CardHeader className="shrink-0">
          <CardTitle>日志</CardTitle>
          <CardDescription>
            自动刷新，显示警告 / 错误、关键事件与每连接出站节点；完整日志见数据目录
            logs/ 下的 ice-box.log 与 sing-box.log
          </CardDescription>
          <CardAction>
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() => void refresh()}
            >
              刷新
            </Button>
          </CardAction>
        </CardHeader>
        <CardContent className="flex min-h-0 flex-1 flex-col">
          <pre
            ref={boxRef}
            className="log-view min-h-0 flex-1 overflow-auto bg-muted/40 p-3 font-mono text-xs leading-relaxed text-muted-foreground whitespace-pre-wrap break-all"
            onScroll={handleScroll}
            aria-live="polite"
          >
            {lines.length === 0 ? "（空）" : lines.join("\n")}
          </pre>
        </CardContent>
      </Card>
    </div>
  );
}
