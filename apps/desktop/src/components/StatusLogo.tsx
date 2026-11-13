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
import errorStill32 from "@/assets/switch-logo/32/error-still.png";
import errorStrip32 from "@/assets/switch-logo/32/error.png";
import runningStill32 from "@/assets/switch-logo/32/running-still.png";
import runningStrip32 from "@/assets/switch-logo/32/running.png";
import startingStill32 from "@/assets/switch-logo/32/starting-still.png";
import startingStrip32 from "@/assets/switch-logo/32/starting.png";
import stoppedStill32 from "@/assets/switch-logo/32/stopped-still.png";
import stoppedStrip32 from "@/assets/switch-logo/32/stopped.png";
import stoppingStill32 from "@/assets/switch-logo/32/stopping-still.png";
import stoppingStrip32 from "@/assets/switch-logo/32/stopping.png";

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

/** The 48 px loop strip and still frame for each core state. */
const SPRITES_48: Record<CoreStatus, { strip: string; still: string }> = {
  stopped: { strip: stoppedStrip, still: stoppedStill },
  starting: { strip: startingStrip, still: startingStill },
  running: { strip: runningStrip, still: runningStill },
  stopping: { strip: stoppingStrip, still: stoppingStill },
  error: { strip: errorStrip, still: errorStill },
};

/** The 32 px set, redrawn for the phone header rather than downscaled. */
const SPRITES_32: Record<CoreStatus, { strip: string; still: string }> = {
  stopped: { strip: stoppedStrip32, still: stoppedStill32 },
  starting: { strip: startingStrip32, still: startingStill32 },
  running: { strip: runningStrip32, still: runningStill32 },
  stopping: { strip: stoppingStrip32, still: stoppingStill32 },
  error: { strip: errorStrip32, still: errorStill32 },
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
  size = 48,
  className,
}: {
  status: StatusResponse | null;
  /** 48 on desktop, 32 in the phone header; each has its own redrawn strips. */
  size?: 32 | 48;
  className?: string;
}) {
  const core = logoCore(status);
  const label = logoLabel(core);
  const sprite = (size === 32 ? SPRITES_32 : SPRITES_48)[
    core === "unknown" ? "stopped" : core
  ];
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
          className={cn("status-logo", size === 32 ? "size-8" : "size-12", className)}
          data-core={core}
          data-size={size}
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
