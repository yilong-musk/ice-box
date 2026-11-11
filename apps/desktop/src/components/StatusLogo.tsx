// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * App logo that mirrors the core state, replacing the Home "core status" row.
 *
 * The logo is the pixel switch (docs/brand/switch-logo): its port lights show
 * the core state, each state as a 3-second loop of 45 frames in one horizontal
 * strip. unknown looks like stopped. The loop plays only while the app is in
 * the foreground; otherwise the state's still frame is shown. Visuals live in
 * `index.css` under `.status-logo`, keyed on `data-core` and `data-active`.
 *
 * Desktop shows the label on hover. The phone shell has no hover, so a tap
 * toggles it and it stays up for a fixed time, whatever the pointer does.
 */

import { type CSSProperties, useEffect, useState, useSyncExternalStore } from "react";
import { isPhoneShell } from "@platform/shell";

import type { CoreStatus, StatusResponse } from "@/api/contracts";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { t } from "@/lib/i18n";
import { cn } from "@/lib/utils";

import errorStill from "@/assets/switch-logo/error-still.png";
import errorStrip from "@/assets/switch-logo/error.png";
import runningStill from "@/assets/switch-logo/running-still.png";
import runningStrip from "@/assets/switch-logo/running.png";
import startingStill from "@/assets/switch-logo/starting-still.png";
import startingStrip from "@/assets/switch-logo/starting.png";
import stoppedStill from "@/assets/switch-logo/stopped-still.png";
import stoppedStrip from "@/assets/switch-logo/stopped.png";
import stoppingStill from "@/assets/switch-logo/stopping-still.png";
import stoppingStrip from "@/assets/switch-logo/stopping.png";

export type LogoCore = CoreStatus | "unknown";

/**
 * `unknown` until the first status arrives. It looks exactly like `stopped`;
 * only the label differs ("—" instead of "Stopped").
 */
export function logoCore(status: StatusResponse | null): LogoCore {
  return status?.core.status ?? "unknown";
}

/** Tooltip text and accessible name, e.g. "Core status: Running". */
export function logoLabel(core: LogoCore): string {
  return t("app.logo.core", {
    state:
      core === "unknown" ? t("common.dash") : t(`home.coreStatus.${core}`),
  });
}

/** The loop strip and still frame for each core state. */
const SPRITES: Record<CoreStatus, { strip: string; still: string }> = {
  stopped: { strip: stoppedStrip, still: stoppedStill },
  starting: { strip: startingStrip, still: startingStill },
  running: { strip: runningStrip, still: runningStill },
  stopping: { strip: stoppingStrip, still: stoppingStill },
  error: { strip: errorStrip, still: errorStill },
};

/** True while the page is visible and its window has focus. */
function isForeground(): boolean {
  return document.visibilityState === "visible" && document.hasFocus();
}

function subscribeForeground(onChange: () => void): () => void {
  window.addEventListener("focus", onChange);
  window.addEventListener("blur", onChange);
  document.addEventListener("visibilitychange", onChange);
  return () => {
    window.removeEventListener("focus", onChange);
    window.removeEventListener("blur", onChange);
    document.removeEventListener("visibilitychange", onChange);
  };
}

/** Whether the app is open and in the foreground, so animations may run. */
function useForeground(): boolean {
  return useSyncExternalStore(subscribeForeground, isForeground, () => false);
}

/** How long a tapped label stays up on the phone. */
const TAP_LABEL_MS = 3000;

export function StatusLogo({
  status,
  className,
}: {
  status: StatusResponse | null;
  className?: string;
}) {
  const core = logoCore(status);
  const label = logoLabel(core);
  const sprite = SPRITES[core === "unknown" ? "stopped" : core];
  const phone = isPhoneShell();
  const active = useForeground();
  const [tapOpen, setTapOpen] = useState(false);

  useEffect(() => {
    if (!tapOpen) return;
    const id = window.setTimeout(() => setTapOpen(false), TAP_LABEL_MS);
    return () => window.clearTimeout(id);
  }, [tapOpen]);

  // Phone: the label closes only on a second tap or when the timer runs out.
  // Every Radix open/close request (focus, pointer leave, outside press,
  // Escape, scroll) is ignored, so nothing else opens it or cuts it short.
  const rootProps = phone
    ? { open: tapOpen, onOpenChange: () => {} }
    : {};

  return (
    <Tooltip {...rootProps}>
      <TooltipTrigger
        asChild
        onClick={
          phone
            ? (event) => {
                // Skip Radix's close-on-click so the tap can open the label.
                event.preventDefault();
                setTapOpen((open) => !open);
              }
            : undefined
        }
      >
        {/* A button, so keyboard users reach it and open the label on
            desktop, and a tap toggles it on the phone. */}
        <button
          type="button"
          aria-label={label}
          className={cn("status-logo", className)}
          data-core={core}
          data-active={active ? "" : undefined}
          data-testid="status-logo"
        >
          <span
            aria-hidden="true"
            className="status-logo-art"
            style={
              {
                "--logo-strip": `url("${sprite.strip}")`,
                "--logo-still": `url("${sprite.still}")`,
              } as CSSProperties
            }
          />
        </button>
      </TooltipTrigger>
      {/* The phone logo sits at the top edge, so its label opens below; the
          padding keeps it off the screen edge. */}
      <TooltipContent side={phone ? "bottom" : "top"} collisionPadding={12}>
        {label}
      </TooltipContent>
    </Tooltip>
  );
}
