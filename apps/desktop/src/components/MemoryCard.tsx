// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Home memory card: the total footprint, a bar split into core and app, and
 * a legend with each part's figure.
 *
 * The bar is the current total, not a scale with empty room past it. Core and
 * app each take their share of that total, so the two segments fill the bar.
 * Both segments, and the total, take the colour of the band the total falls in.
 */

import type { MemoryUsage } from "@/api/contracts";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { t } from "@/lib/i18n";
import {
  formatMemory,
  formatMemoryPart,
  memoryLabel,
  memoryMb,
  memoryToneFor,
} from "@/lib/memory";
import { TONE_BG, TONE_TEXT, type Tone } from "@/lib/tone";
import { cn } from "@/lib/utils";

/** Core segment: the full band colour. */
const TONE_CORE = TONE_BG;

/** App segment: the same colour, lighter, so the two parts read apart. */
const TONE_APP: Record<Tone, string> = {
  ok: "bg-ok/40",
  warn: "bg-warn/40",
  bad: "bg-destructive/40",
};

/** Percent of the bar, rounded to 0.01. The two parts share one total, so the
 * app segment is whatever the core segment does not take. */
function sharePct(partMb: number, totalMb: number): number {
  if (totalMb <= 0) return 0;
  return Math.round((partMb / totalMb) * 10000) / 100;
}

function widthOf(share: number): string {
  return `${share}%`;
}

/** Hover text for the total: the breakdown, or why the core is missing. */
function memoryTip(memory: MemoryUsage | undefined, coreRunning: boolean) {
  if (!memory) return undefined;
  if (memory.core_bytes != null) {
    return t("home.memory.tooltip", {
      core: formatMemory(memory.core_bytes),
      app: formatMemoryPart(memory.app_bytes),
    });
  }
  return coreRunning
    ? t("home.memory.tooltipUnreadable", { app: formatMemoryPart(memory.app_bytes) })
    : t("home.memory.tooltipAppOnly", { app: formatMemoryPart(memory.app_bytes) });
}

export function MemoryCard({
  memory,
  coreRunning,
}: {
  memory: MemoryUsage | undefined;
  coreRunning: boolean;
}) {
  const tone = memoryToneFor(memory);
  const label = memoryLabel(memory);
  const tip = memoryTip(memory, coreRunning);

  // Rounded per-part figures, the same ones the legend prints.
  const coreMb = memory?.core_bytes != null ? memoryMb(memory.core_bytes) : 0;
  const appMb = memory?.app_bytes != null ? memoryMb(memory.app_bytes) : 0;
  const totalMb = coreMb + appMb;
  const coreShare = sharePct(coreMb, totalMb);
  const appShare = totalMb <= 0 ? 0 : Math.round((100 - coreShare) * 100) / 100;

  const coreValue =
    memory?.core_bytes != null
      ? formatMemory(memory.core_bytes)
      : coreRunning
        ? t("home.memory.coreUnreadable")
        : t("home.memory.coreStopped");
  const appValue = formatMemoryPart(memory?.app_bytes ?? null);

  return (
    <Card size="sm" className="flex w-full min-w-0 flex-col">
      <CardHeader className="flex flex-col items-start gap-1">
        <CardTitle className="text-xs font-medium text-muted-foreground">
          {t("home.memoryTitle")}
        </CardTitle>
        <span
          dir="auto"
          className={cn(
            "font-mono text-xl font-semibold tabular-nums",
            tone && TONE_TEXT[tone],
          )}
          title={tip ?? label}
          data-testid="home-health-memory"
        >
          {label}
        </span>
      </CardHeader>
      <CardContent className="flex flex-1 flex-col justify-center gap-4">
        <div
          role="img"
          aria-label={tip ?? label}
          className="flex h-3 w-full overflow-hidden rounded-full bg-muted"
          data-testid="home-memory-bar"
        >
          <div
            className={cn(
              "h-full transition-[width] duration-500",
              tone ? TONE_CORE[tone] : "bg-muted-foreground",
            )}
            style={{ width: widthOf(coreShare) }}
            data-testid="home-memory-bar-core"
          />
          <div
            className={cn(
              "h-full transition-[width] duration-500",
              tone ? TONE_APP[tone] : "bg-muted-foreground/40",
            )}
            style={{ width: widthOf(appShare) }}
            data-testid="home-memory-bar-app"
          />
        </div>
        <div className="flex flex-col gap-1.5 text-xs">
          <LegendItem
            swatch={tone ? TONE_CORE[tone] : "bg-muted-foreground"}
            label={t("home.memory.core")}
            value={coreValue}
            testId="home-memory-core"
          />
          <LegendItem
            swatch={tone ? TONE_APP[tone] : "bg-muted-foreground/40"}
            label={t("home.memory.app")}
            value={appValue}
            testId="home-memory-app"
          />
        </div>
      </CardContent>
    </Card>
  );
}

function LegendItem({
  swatch,
  label,
  value,
  testId,
}: {
  swatch: string;
  label: string;
  value: string;
  testId: string;
}) {
  return (
    <span className="flex min-w-0 items-center gap-1.5">
      <span aria-hidden="true" className={cn("size-2 shrink-0 rounded-full", swatch)} />
      <span className="flex-1 text-muted-foreground">{label}</span>
      <span
        dir="auto"
        className="truncate font-mono tabular-nums"
        data-testid={testId}
      >
        {value}
      </span>
    </span>
  );
}
