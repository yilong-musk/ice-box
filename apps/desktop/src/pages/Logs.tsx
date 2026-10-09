// SPDX-License-Identifier: GPL-3.0-or-later

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, formatInvokeError } from "../api/client";
import { useGenerationGuard } from "../lib/generationGuard";
import { ErrorAlert } from "../components/StatusAlert";
import { t, useLanguagePreference } from "../lib/i18n";

const POLL_MS = 2000;
const VIEW_LINES = 500;
const STICK_THRESHOLD_PX = 40;

function documentHidden(): boolean {
  return document.visibilityState === "hidden";
}

/** Color the whole line by its compact `LEVEL` prefix. INFO/DEBUG/TRACE stay default. */
function logLineClass(line: string): string | undefined {
  const space = line.indexOf(" ");
  const level = space === -1 ? line : line.slice(0, space);
  if (level === "WARN") return "text-warn";
  if (level === "ERROR" || level === "FATAL") return "text-destructive";
  return undefined;
}

export function Logs({ active = true }: { active?: boolean }) {
  useLanguagePreference();
  const { nextGeneration, isStale } = useGenerationGuard();
  const [lines, setLines] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [stickToBottom, setStickToBottom] = useState(true);
  const boxRef = useRef<HTMLPreElement | null>(null);
  const lastTextRef = useRef("");
  const activeRef = useRef(active);
  activeRef.current = active;
  const logActiveChainRef = useRef(Promise.resolve());

  const queueLogActive = (visible: boolean) => {
    if (typeof api.setLogViewActive !== "function") return;
    logActiveChainRef.current = logActiveChainRef.current.then(async () => {
      try {
        await api.setLogViewActive(visible);
      } catch {
        // The website demo has no parsed log cache.
      }
    });
  };

  const refresh = useCallback(async () => {
    if (!activeRef.current || documentHidden()) return;
    await logActiveChainRef.current;
    if (!activeRef.current || documentHidden()) return;
    const gen = nextGeneration();
    try {
      const tail = await api.getLogView(VIEW_LINES);
      if (isStale(gen) || !activeRef.current) return;
      setError(null);
      setLines((prev) => {
        const text = tail.join("\n");
        if (text === lastTextRef.current) return prev;
        lastTextRef.current = text;
        return tail;
      });
    } catch (e) {
      if (!isStale(gen) && activeRef.current) setError(formatInvokeError(e));
    }
  }, [isStale, nextGeneration]);

  useEffect(() => {
    if (!active) {
      nextGeneration();
      setLines([]);
      lastTextRef.current = "";
      setError(null);
    }
    queueLogActive(active);
  }, [active, nextGeneration]);

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    let timer = 0;
    nextGeneration();
    void (async () => {
      await logActiveChainRef.current;
      if (cancelled || !activeRef.current) return;
      void refresh();
      timer = window.setInterval(() => void refresh(), POLL_MS);
    })();
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
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
    <div
      className="logs-panel flex min-h-0 flex-1 flex-col overflow-hidden gap-3"
      data-testid="logs-panel"
    >
      {error && <ErrorAlert className="shrink-0">{error}</ErrorAlert>}
      <pre
        ref={boxRef}
        className="log-view min-h-0 flex-1 overflow-auto bg-card p-3 font-mono text-xs leading-relaxed text-foreground whitespace-pre-wrap break-all"
        data-testid="log-view"
        onScroll={handleScroll}
        aria-live="polite"
      >
        {lines.length === 0
          ? t("logs.empty")
          : lines.map((line, i) => (
              <div key={i} className={logLineClass(line)}>
                {line}
              </div>
            ))}
      </pre>
    </div>
  );
}
