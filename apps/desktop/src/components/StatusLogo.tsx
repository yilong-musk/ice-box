// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * App logo that mirrors the core state, replacing the Home "core status" row.
 *
 * stopped: the plain logo; starting/stopping: amber breathing glow;
 * running: green; error: red. Visuals live in `index.css` under
 * `.status-logo`, keyed on `data-core`.
 *
 * Desktop shows the label on hover. The phone shell has no hover, so a tap
 * toggles it and it stays up for a fixed time, whatever the pointer does.
 */

import { useEffect, useState } from "react";
import { isPhoneShell } from "@platform/shell";

import type { CoreStatus, StatusResponse } from "@/api/contracts";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { t, useLanguagePreference } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export type LogoCore = CoreStatus | "unknown";

/** `unknown` until the first status arrives, so boot does not flash grey. */
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
  // Re-render on language change so the label follows the UI language.
  useLanguagePreference();
  const core = logoCore(status);
  const label = logoLabel(core);
  const phone = isPhoneShell();
  const [tapOpen, setTapOpen] = useState(false);

  useEffect(() => {
    if (!tapOpen) return;
    const id = window.setTimeout(() => setTapOpen(false), TAP_LABEL_MS);
    return () => window.clearTimeout(id);
  }, [tapOpen]);

  // Phone: taps and the timer own the label. Radix's own open/close requests
  // (pointer leave, focus, outside press) are ignored, so a pointer moving
  // away does not cut the fixed display time short.
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
        <span
          role="img"
          aria-label={label}
          className={cn("status-logo", className)}
          data-core={core}
          data-testid="status-logo"
        >
          <img src={src} alt="" draggable={false} />
        </span>
      </TooltipTrigger>
      {/* The phone logo sits at the top edge, so its label opens below; the
          padding keeps it off the screen edge. */}
      <TooltipContent side={phone ? "bottom" : "top"} collisionPadding={12}>
        {label}
      </TooltipContent>
    </Tooltip>
  );
}
