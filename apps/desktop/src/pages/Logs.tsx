import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, formatInvokeError } from "../api/tauri";
import { useGenerationGuard } from "../lib/generationGuard";
import { ErrorAlert } from "../components/StatusAlert";
import { t, useLanguagePreference } from "../lib/i18n";

const POLL_MS = 2000;
const VIEW_LINES = 500;
const STICK_THRESHOLD_PX = 40;

export function Logs({ active = true }: { active?: boolean }) {
  useLanguagePreference();
  const { nextGeneration, isStale } = useGenerationGuard();
  const [lines, setLines] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [stickToBottom, setStickToBottom] = useState(true);
  const boxRef = useRef<HTMLPreElement | null>(null);
  const lastTextRef = useRef("");

  const refresh = useCallback(async () => {
    if (!active || document.visibilityState === "hidden") return;
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
  }, [active, isStale, nextGeneration]);

  useEffect(() => {
    if (!active) return;
    nextGeneration();
    void refresh();
    const id = window.setInterval(() => void refresh(), POLL_MS);
    return () => window.clearInterval(id);
  }, [active, nextGeneration, refresh]);

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
      <pre
        ref={boxRef}
        className="log-view min-h-0 flex-1 overflow-auto bg-card p-3 font-mono text-xs leading-relaxed text-foreground whitespace-pre-wrap break-all"
        onScroll={handleScroll}
        aria-live="polite"
      >
        {lines.length === 0 ? t("logs.empty") : lines.join("\n")}
      </pre>
    </div>
  );
}
