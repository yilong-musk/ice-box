// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * App logo that mirrors the core state, replacing the Home "core status" row.
 *
 * unknown/stopped: the plain logo; starting/stopping: amber; running: green;
 * error: red. The glow breathes only while the app is in the foreground and
 * holds still otherwise. Visuals live in `index.css` under `.status-logo`,
 * keyed on `data-core` and `data-active`.
 *
 * Desktop shows the label on hover. The phone shell has no hover, so a tap
 * toggles it and it stays up for a fixed time, whatever the pointer does.
 */

import { useEffect, useState, useSyncExternalStore } from "react";
import { isPhoneShell } from "@platform/shell";

import type { CoreStatus, StatusResponse } from "@/api/contracts";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { t } from "@/lib/i18n";
import { cn } from "@/lib/utils";

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
  src,
  status,
  className,
}: {
  src: string;
  status: StatusResponse | null;
  className?: string;
}) {
  const core = logoCore(status);
  const label = logoLabel(core);
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
          <img src={src} alt="" draggable={false} />
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
