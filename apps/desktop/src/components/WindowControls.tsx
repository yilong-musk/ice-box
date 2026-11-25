// SPDX-License-Identifier: GPL-3.0-or-later

import { useEffect, useState, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { detectWindowChrome, runWindowCommand } from "@platform/windowChrome";
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

/** macOS light colours, with the darker glyph ink each light uses. */
const LIGHT_CLASS: Record<LightTone, string> = {
  close: "bg-[#ff5f57] text-[#4d0000]",
  minimize: "bg-[#febc2e] text-[#985700]",
  zoom: "bg-[#28c840] text-[#006500]",
};

/** Glyphs drawn on a 12px light, shown while the group is hovered. */
const LIGHT_GLYPH: Record<LightTone, ReactNode> = {
  close: <path d="M3.75 3.75l4.5 4.5M8.25 3.75l-4.5 4.5" />,
  minimize: <path d="M3.25 6h5.5" />,
  zoom: <path d="M6 3.25v5.5M3.25 6h5.5" />,
};

function TrafficLight({
  label,
  tone,
  showGlyph,
  dimmed,
  onCommand,
}: {
  label: string;
  tone: LightTone;
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
          {LIGHT_GLYPH[tone]}
        </svg>
      </span>
    </button>
  );
}

/**
 * Window buttons on Windows, drawn as macOS traffic lights (close, minimize,
 * zoom) at the same inset as the native lights on macOS (14px from the left,
 * centred in the 48px title bar), so the collapsed rail is the same width on
 * every desktop. macOS keeps its native lights and renders nothing here.
 */
export function WindowControls() {
  useLanguagePreference();
  const windows = detectWindowChrome() === "windows-custom";
  const resumeEpoch = useCaptionResumeEpoch(windows);
  const focused = useWindowFocused(windows);
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
      className="flex h-full shrink-0 items-center pl-2.5"
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
          label={t("window.maximize")}
          tone="zoom"
          showGlyph={hovered}
          dimmed={!focused && !hovered}
          onCommand={() => void runWindowCommand("toggleMaximize")}
        />
      </div>
    </div>
  );
}
