// SPDX-License-Identifier: GPL-3.0-or-later

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Check } from "lucide-react";
import { api, formatInvokeError } from "../api/client";
import { ErrorAlert } from "../components/StatusAlert";
import { useGenerationGuard } from "../lib/generationGuard";
import { t, useLanguagePreference } from "../lib/i18n";
import { copyText, readLogSelection, sameLogSelection, type LogSelection } from "../lib/logCopy";
import { alignLogRows, type LogRow } from "../lib/logRows";

const POLL_MS = 2000;
const VIEW_LINES = 500;
const STICK_THRESHOLD_PX = 40;
const COPY_HINT_MS = 1600;

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

function nextRowId(rows: readonly LogRow[]): number {
  let nextId = 1;
  for (const row of rows) {
    if (row.id >= nextId) nextId = row.id + 1;
  }
  return nextId;
}

/**
 * Chromium extends a selection inside a scroller when script sets `scrollTop`
 * (the same path as drag-select autoscroll). Pause stick-to-bottom while a
 * range is selected; the next tail update resumes it after the range is gone.
 */
function hasLogSelection(box: HTMLElement): boolean {
  const selection = window.getSelection();
  if (selection == null || selection.isCollapsed || selection.rangeCount === 0) return false;
  const anchor = selection.anchorNode;
  const focus = selection.focusNode;
  return (anchor != null && box.contains(anchor)) || (focus != null && box.contains(focus));
}

export function Logs({ active = true }: { active?: boolean }) {
  useLanguagePreference();
  const { nextGeneration, isStale } = useGenerationGuard();
  const [lines, setLines] = useState<LogRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [copyFailed, setCopyFailed] = useState(false);
  const [copyHintVisible, setCopyHintVisible] = useState(false);
  const [copyHintNonce, setCopyHintNonce] = useState(0);
  const [stickToBottom, setStickToBottom] = useState(true);
  const boxRef = useRef<HTMLPreElement | null>(null);
  const copyHintTimerRef = useRef<number | null>(null);
  const lastTextRef = useRef("");
  const stickToBottomRef = useRef(stickToBottom);
  stickToBottomRef.current = stickToBottom;
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
      const text = tail.join("\n");
      if (text === lastTextRef.current) return;
      lastTextRef.current = text;
      setLines((prev) => alignLogRows(prev, tail, nextRowId(prev)).rows);
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
      setCopyFailed(false);
      setCopyHintVisible(false);
      if (copyHintTimerRef.current !== null) {
        window.clearTimeout(copyHintTimerRef.current);
        copyHintTimerRef.current = null;
      }
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

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    let armed = false;
    let started: LogSelection | null = null;
    let generation = 0;

    const onMouseDown = (event: MouseEvent) => {
      if (event.button !== 0) return;
      const box = boxRef.current;
      const target = event.target;
      if (!box || !(target instanceof Node) || !box.contains(target)) {
        armed = false;
        return;
      }
      armed = true;
      started = readLogSelection(box);
    };

    const onMouseUp = (event: MouseEvent) => {
      if (event.button !== 0 || !armed) return;
      armed = false;
      const box = boxRef.current;
      if (!box) return;
      const selected = readLogSelection(box);
      // A click or scrollbar drag leaves the range alone. Copy only when this
      // gesture made a selection (drag, double-click, triple-click).
      if (selected == null) return;
      if (event.detail < 2 && sameLogSelection(started, selected)) return;
      const text = selected.text;
      // Start the write in this mouseup turn so the clipboard API still sees
      // the user gesture. A later selection supersedes an earlier failure.
      const gen = ++generation;
      void copyText(text).then(
        () => {
          if (cancelled || gen !== generation) return;
          if (copyHintTimerRef.current !== null) {
            window.clearTimeout(copyHintTimerRef.current);
          }
          setCopyFailed(false);
          setCopyHintNonce((nonce) => nonce + 1);
          setCopyHintVisible(true);
          copyHintTimerRef.current = window.setTimeout(() => {
            copyHintTimerRef.current = null;
            setCopyHintVisible(false);
          }, COPY_HINT_MS);
        },
        () => {
          if (cancelled || gen !== generation) return;
          if (copyHintTimerRef.current !== null) {
            window.clearTimeout(copyHintTimerRef.current);
            copyHintTimerRef.current = null;
          }
          setCopyHintVisible(false);
          setCopyFailed(true);
        },
      );
    };

    document.addEventListener("mousedown", onMouseDown);
    document.addEventListener("mouseup", onMouseUp);
    return () => {
      cancelled = true;
      if (copyHintTimerRef.current !== null) {
        window.clearTimeout(copyHintTimerRef.current);
        copyHintTimerRef.current = null;
      }
      document.removeEventListener("mousedown", onMouseDown);
      document.removeEventListener("mouseup", onMouseUp);
    };
  }, [active]);

  const scrollToBottom = useCallback(() => {
    const box = boxRef.current;
    if (!box || !stickToBottomRef.current || hasLogSelection(box)) return;
    box.scrollTop = box.scrollHeight;
  }, []);

  useLayoutEffect(() => {
    const raf = requestAnimationFrame(scrollToBottom);
    return () => cancelAnimationFrame(raf);
  }, [lines, stickToBottom, scrollToBottom]);

  const handleScroll = useCallback(() => {
    const box = boxRef.current;
    if (!box) return;
    const nearBottom =
      box.scrollHeight - box.scrollTop - box.clientHeight < STICK_THRESHOLD_PX;
    setStickToBottom((prev) => (prev === nearBottom ? prev : nearBottom));
  }, []);

  return (
    <div
      className="logs-panel relative flex min-h-0 flex-1 flex-col overflow-hidden gap-3"
      data-testid="logs-panel"
    >
      {error && <ErrorAlert className="shrink-0">{error}</ErrorAlert>}
      {copyFailed && <ErrorAlert className="shrink-0">{t("logs.copyFailed")}</ErrorAlert>}
      {copyHintVisible && (
        <div
          key={copyHintNonce}
          role="status"
          data-testid="log-copy-hint"
          className="pointer-events-none absolute bottom-4 left-1/2 z-10 flex -translate-x-1/2 animate-in fade-in-0 slide-in-from-bottom-2 items-center gap-1.5 rounded-full bg-foreground px-3 py-1 text-xs font-medium text-background shadow-lg duration-200"
        >
          <Check className="size-3" aria-hidden="true" />
          {t("logs.copied")}
        </div>
      )}
      {/* Anchoring would scroll when a row leaves the top, and Chromium would drag the selection with it. */}
      <pre
        ref={boxRef}
        className="log-view min-h-0 flex-1 overflow-auto [overflow-anchor:none] bg-card p-3 font-mono text-xs leading-relaxed text-foreground whitespace-pre-wrap break-all"
        data-testid="log-view"
        onScroll={handleScroll}
        aria-live="polite"
      >
        {lines.length === 0
          ? t("logs.empty")
          : lines.map((line) => (
              <div key={line.id} className={logLineClass(line.text)}>
                {line.text}
              </div>
            ))}
      </pre>
    </div>
  );
}
