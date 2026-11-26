// SPDX-License-Identifier: GPL-3.0-or-later

import { useEffect, useState, type ReactNode } from "react";
import { flushSync } from "react-dom";
import {
  controlsFollowWindowFocus,
  detectWindowChrome,
  runWindowCommand,
  subscribeWindowMaximized,
} from "@platform/windowChrome";
import { api } from "@/api/client";
import { t, useLanguagePreference } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/**
 * WebView2 keeps :hover and :active after the window is hidden on click, so the
 * close light still shows its glyph when the tray shows the window again.
 * Pointer state is dropped when the window blurs, hides, or is shown.
 */
function useCaptionResumeEpoch(enabled: boolean): number {
  const [epoch, setEpoch] = useState(0);
  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    const bump = () => {
      if (!disposed) setEpoch((n) => n + 1);
    };
    const onVisibility = () => {
      if (document.visibilityState === "hidden") bump();
    };
    window.addEventListener("blur", bump);
    document.addEventListener("visibilitychange", onVisibility);
    void (async () => {
      try {
        const hidden = await api.listenWindowHidden(bump);
        const shown = await api.listenWindowShown(bump);
        if (disposed) {
          hidden();
          shown();
          return;
        }
        unlisteners.push(hidden, shown);
      } catch {
        // Browser preview and unit tests are not inside Tauri.
      }
    })();
    return () => {
      disposed = true;
      window.removeEventListener("blur", bump);
      document.removeEventListener("visibilitychange", onVisibility);
      for (const off of unlisteners) off();
    };
  }, [enabled]);
  return epoch;
}

/** Whether the window has focus: an unfocused macOS window greys its lights. */
function useWindowFocused(enabled: boolean): boolean {
  const [focused, setFocused] = useState(
    () => typeof document === "undefined" || document.hasFocus(),
  );
  useEffect(() => {
    if (!enabled) return;
    const onFocus = () => setFocused(true);
    const onBlur = () => setFocused(false);
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    setFocused(document.hasFocus());
    return () => {
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
    };
  }, [enabled]);
  return focused;
}

type LightTone = "close" | "minimize" | "zoom";
type LightGlyph = LightTone | "restore";

/** macOS light colours, with the darker glyph ink each light uses. */
const LIGHT_CLASS: Record<LightTone, string> = {
  close: "bg-[#ff5f57] text-[#4d0000]",
  minimize: "bg-[#febc2e] text-[#985700]",
  zoom: "bg-[#28c840] text-[#006500]",
};

/** Two filled triangles, as the macOS green light draws its arrows. */
function TriangleGlyph({ d }: { d: string }) {
  return <path d={d} fill="currentColor" strokeWidth={0.5} strokeLinejoin="round" />;
}

/**
 * Glyphs drawn on a 12px light, shown while the group is hovered. The zoom
 * light uses the macOS full-screen pair in both states (triangles pointing
 * out to maximize, pointing in to restore), on the same top-left to
 * bottom-right diagonal; the plus is the Option-key zoom glyph on macOS and
 * would mix two conventions.
 */
const LIGHT_GLYPH: Record<LightGlyph, ReactNode> = {
  close: <path d="M3.75 3.75l4.5 4.5M8.25 3.75l-4.5 4.5" />,
  minimize: <path d="M3.25 6h5.5" />,
  zoom: <TriangleGlyph d="M3.25 3.25h4.25L3.25 7.5zM8.75 8.75H4.5l4.25-4.25z" />,
  restore: <TriangleGlyph d="M5.4 2.65v2.75H2.65zM6.6 9.35V6.6h2.75z" />,
};

/** Whether the window is maximized, so the zoom light can offer to restore. */
function useWindowMaximized(enabled: boolean): boolean {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    if (!enabled) return;
    return subscribeWindowMaximized(setMaximized);
  }, [enabled]);
  return maximized;
}

function TrafficLight({
  label,
  tone,
  glyph = tone,
  showGlyph,
  dimmed,
  onCommand,
}: {
  label: string;
  tone: LightTone;
  glyph?: LightGlyph;
  showGlyph: boolean;
  dimmed: boolean;
  onCommand: (button: HTMLButtonElement) => void;
}) {
  return (
    // A 20px hit target around the 12px light; three of them sit 20px apart,
    // the macOS spacing.
    <button
      type="button"
      className="group/light flex size-5 items-center justify-center rounded-full outline-none"
      aria-label={label}
      data-tone={tone}
      data-glyph={glyph}
      onClick={(event) => onCommand(event.currentTarget)}
    >
      <span
        className={cn(
          "flex size-3 items-center justify-center rounded-full shadow-[inset_0_0_0_0.5px_rgb(0_0_0/0.18)] group-active/light:brightness-[0.82] group-focus-visible/light:ring-2 group-focus-visible/light:ring-ring",
          dimmed ? "bg-foreground/15 text-transparent" : LIGHT_CLASS[tone],
        )}
      >
        <svg
          viewBox="0 0 12 12"
          className={cn("size-3", showGlyph ? "opacity-100" : "opacity-0")}
          fill="none"
          stroke="currentColor"
          strokeWidth={1.25}
          strokeLinecap="round"
          aria-hidden="true"
        >
          {LIGHT_GLYPH[glyph]}
        </svg>
      </span>
    </button>
  );
}

/**
 * Window buttons on Windows, drawn as macOS traffic lights (close, minimize,
 * zoom) 14px in from the window's top-left corner, about where the native
 * lights sit on macOS, so the collapsed rail is the same width on every
 * desktop. macOS keeps its native lights and renders nothing here. The
 * website's Live Demo draws these too, so the two always match.
 */
export function WindowControls() {
  useLanguagePreference();
  const windows = detectWindowChrome() === "windows-custom";
  const resumeEpoch = useCaptionResumeEpoch(windows);
  const followFocus = windows && controlsFollowWindowFocus();
  const windowFocused = useWindowFocused(followFocus);
  const focused = !followFocus || windowFocused;
  const maximized = useWindowMaximized(windows);
  const [hot, setHot] = useState(false);
  const [pointerEpoch, setPointerEpoch] = useState(resumeEpoch);
  if (!windows) return null;

  // As on macOS, hovering any light shows the glyphs on all three.
  const hovered = hot && pointerEpoch === resumeEpoch;

  function setHovered(next: boolean) {
    setPointerEpoch(resumeEpoch);
    setHot(next);
  }

  return (
    <div
      // 14px from the top as from the left: the 20px hit target starts 10px
      // down, so the 12px light sits 14px below the window's top edge.
      className="flex h-full shrink-0 items-start pl-2.5 pt-2.5"
      data-testid="window-controls"
      data-hovered={hovered ? "true" : undefined}
    >
      <div
        className="flex items-center"
        onPointerEnter={() => setHovered(true)}
        onPointerLeave={() => setHovered(false)}
        onPointerCancel={() => setHovered(false)}
      >
        <TrafficLight
          label={t("window.close")}
          tone="close"
          showGlyph={hovered}
          dimmed={!focused && !hovered}
          onCommand={(button) => {
            // Commit the released paint before hide freezes the webview.
            button.blur();
            flushSync(() => setHovered(false));
            void runWindowCommand("close");
          }}
        />
        <TrafficLight
          label={t("window.minimize")}
          tone="minimize"
          showGlyph={hovered}
          dimmed={!focused && !hovered}
          onCommand={() => void runWindowCommand("minimize")}
        />
        <TrafficLight
          label={maximized ? t("window.restore") : t("window.maximize")}
          tone="zoom"
          glyph={maximized ? "restore" : "zoom"}
          showGlyph={hovered}
          dimmed={!focused && !hovered}
          onCommand={() => void runWindowCommand("toggleMaximize")}
        />
      </div>
    </div>
  );
}
