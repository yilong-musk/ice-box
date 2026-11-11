// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Outbound mode picker for Home. The phone draws a horizontal segmented
 * control with a one-line hint under it; the desktop draws a vertical list
 * whose selected row is tinted and checked, with the hint on hover. Both
 * share the modes, icons, hints and the value guard.
 */

import { Check, Globe, MoveRight, Split, type LucideIcon } from "lucide-react";
import type { ProxyMode } from "@/api/contracts";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { t, type MessageKey } from "@/lib/i18n";
import { cn } from "@/lib/utils";

const PROXY_MODES: ReadonlyArray<{
  mode: ProxyMode;
  labelKey: MessageKey;
  hintKey: MessageKey;
  icon: LucideIcon;
}> = [
  { mode: "rule", labelKey: "home.mode.rule", hintKey: "home.modeHint.rule", icon: Split },
  { mode: "global", labelKey: "home.mode.global", hintKey: "home.modeHint.global", icon: Globe },
  { mode: "direct", labelKey: "home.mode.direct", hintKey: "home.modeHint.direct", icon: MoveRight },
];

function isProxyMode(value: string): value is ProxyMode {
  return PROXY_MODES.some((entry) => entry.mode === value);
}

type Props = {
  variant: "segmented" | "list";
  value: ProxyMode;
  disabled?: boolean;
  onChange: (mode: ProxyMode) => void;
  className?: string;
};

export function ProxyModePicker({ variant, value, disabled, onChange, className }: Props) {
  // Settings come from the backend unchecked; a mode this build does not
  // know selects nothing and shows no hint rather than breaking the page.
  const current = PROXY_MODES.find((entry) => entry.mode === value);
  const segmented = variant === "segmented";

  const group = (
    <ToggleGroup
      type="single"
      spacing={1}
      orientation={segmented ? "horizontal" : "vertical"}
      value={current?.mode ?? ""}
      onValueChange={(next) => {
        if (isProxyMode(next)) onChange(next);
      }}
      disabled={disabled}
      className={cn(
        "flex w-full",
        segmented ? "rounded-xl bg-muted p-1" : "flex-1 flex-col",
        !segmented && className,
      )}
      aria-label={t("home.modeAria")}
    >
      {PROXY_MODES.map(({ mode, labelKey, hintKey, icon: Icon }) =>
        segmented ? (
          <ToggleGroupItem
            key={mode}
            value={mode}
            className="h-9 min-w-0 flex-1 gap-1.5 rounded-lg text-muted-foreground hover:bg-transparent hover:text-foreground data-[state=on]:bg-background data-[state=on]:font-semibold data-[state=on]:text-foreground data-[state=on]:shadow-sm"
          >
            <Icon aria-hidden="true" />
            {t(labelKey)}
          </ToggleGroupItem>
        ) : (
          <ToggleGroupItem
            key={mode}
            value={mode}
            title={t(hintKey)}
            className="group/mode h-auto w-full flex-1 justify-start gap-2.5 rounded-lg px-2.5 text-muted-foreground hover:bg-muted/60 hover:text-foreground data-[state=on]:bg-ok/12 data-[state=on]:font-semibold data-[state=on]:text-foreground hover:data-[state=on]:bg-ok/15 [&>svg:first-child]:group-data-[state=on]/mode:text-ok"
          >
            <Icon aria-hidden="true" />
            <span className="flex-1 text-left">{t(labelKey)}</span>
            <Check
              aria-hidden="true"
              className="text-ok opacity-0 transition-opacity group-data-[state=on]/mode:opacity-100"
            />
          </ToggleGroupItem>
        ),
      )}
    </ToggleGroup>
  );

  if (!segmented) return group;
  return (
    <div className={cn("flex flex-col gap-2", className)}>
      {group}
      {current && (
        <p className="px-1 text-xs text-muted-foreground" data-testid="home-mode-hint">
          {t(current.hintKey)}
        </p>
      )}
    </div>
  );
}
