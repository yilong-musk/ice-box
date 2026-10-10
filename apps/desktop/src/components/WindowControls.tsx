// SPDX-License-Identifier: GPL-3.0-or-later

import { useEffect, useState, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { Minus, Square, X } from "lucide-react";
import { detectWindowChrome, runWindowCommand } from "@platform/windowChrome";
import { api } from "@/api/client";
import { t, useLanguagePreference } from "@/lib/i18n";
import { cn } from "@/lib/utils";

const BUTTON_CLASS =
  "flex h-12 w-11 items-center justify-center text-muted-foreground transition-colors";

/**
 * WebView2 keeps :hover and :active after the window is hidden on click, so the
 * close button is still red when the tray shows it again. Pointer state is
 * dropped when the window blurs, hides, or is shown.
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

function CaptionButton({
  label,
  tone,
  resumeEpoch,
  onCommand,
  children,
}: {
  label: string;
  tone: "neutral" | "close";
  resumeEpoch: number;
  onCommand: () => void;
  children: ReactNode;
}) {
  const [hot, setHot] = useState(false);
  const [pointerEpoch, setPointerEpoch] = useState(resumeEpoch);
  const pressed = hot && pointerEpoch === resumeEpoch;

  function setPressed(next: boolean) {
    setPointerEpoch(resumeEpoch);
    setHot(next);
  }

  return (
    <button
      type="button"
      className={cn(
        BUTTON_CLASS,
        pressed && tone === "neutral" && "bg-muted text-foreground",
        pressed && tone === "close" && "bg-destructive text-white",
      )}
      aria-label={label}
      onPointerEnter={() => setPressed(true)}
      onPointerLeave={() => setPressed(false)}
      onPointerCancel={() => setPressed(false)}
      onClick={(event) => {
        if (tone === "close") {
          // Commit the released paint before hide freezes the webview.
          event.currentTarget.blur();
          flushSync(() => setPressed(false));
        }
        onCommand();
      }}
    >
      {children}
    </button>
  );
}

/** Windows caption buttons; macOS keeps native traffic lights in the overlay bar. */
export function WindowControls() {
  useLanguagePreference();
  const windows = detectWindowChrome() === "windows-custom";
  const resumeEpoch = useCaptionResumeEpoch(windows);
  if (!windows) return null;

  return (
    <div className="flex h-full shrink-0">
      <CaptionButton
        label={t("window.minimize")}
        tone="neutral"
        resumeEpoch={resumeEpoch}
        onCommand={() => void runWindowCommand("minimize")}
      >
        <Minus className="size-3.5" />
      </CaptionButton>
      <CaptionButton
        label={t("window.maximize")}
        tone="neutral"
        resumeEpoch={resumeEpoch}
        onCommand={() => void runWindowCommand("toggleMaximize")}
      >
        <Square className="size-3" />
      </CaptionButton>
      <CaptionButton
        label={t("window.close")}
        tone="close"
        resumeEpoch={resumeEpoch}
        onCommand={() => void runWindowCommand("close")}
      >
        <X className="size-3.5" />
      </CaptionButton>
    </div>
  );
}
