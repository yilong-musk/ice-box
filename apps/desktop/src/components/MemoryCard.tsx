// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Home memory card: the total footprint, a stacked bar split into core and
 * app, and a legend with each part's figure.
 *
 * The bar is drawn against a scale that ends where the red band starts (100 MB,
 * or 120 MB on macOS), so how full it is shows how close usage is to that
 * band. Above it the scale grows to the next 50 MB step, so the red mark stays
 * readable and the bar shows how far into the red band usage has gone. The bar
 * and the total take the colour of the band the total falls in.
 */

import type { MemoryUsage } from "@/api/contracts";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { t } from "@/lib/i18n";
import {
  displayMemoryMb,
  formatMemory,
  formatMemoryPart,
  memoryBands,
  memoryLabel,
  memoryMb,
  memoryToneFor,
} from "@/lib/memory";
import { TONE_BG, TONE_TEXT, type Tone } from "@/lib/tone";
import { cn } from "@/lib/utils";

/** Past the red band's start the scale grows in these steps, never ending
 * just past that mark. */
const SCALE_STEP_MB = 50;

/** An inner label closer than this share of the bar to either end is dropped
 * (its gap marker stays), so it never runs into the end labels. */
const MIN_LABEL_GAP = 0.12;

/** Bar length in MB for a displayed total: the start of the red band
 * (`badMb`) until usage passes it. */
function memoryScaleMb(totalMb: number, badMb: number): number {
  return totalMb <= badMb
    ? badMb
    : Math.ceil(totalMb / SCALE_STEP_MB) * SCALE_STEP_MB;
}

/** Core segment: the full band colour. */
const TONE_CORE = TONE_BG;

/** App segment: the same colour, lighter, so the two parts read apart. */
const TONE_APP: Record<Tone, string> = {
  ok: "bg-ok/40",
  warn: "bg-warn/40",
  bad: "bg-destructive/40",
};

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
  const bands = memoryBands();
  const scale = memoryScaleMb(memory ? displayMemoryMb(memory) : 0, bands.badMb);
  // Scale labels: the start, and where the bar turns yellow and red. Past the
  // red mark the scale grows, and its end gets a label of its own.
  const baseTicks = [0, bands.warnMb, bands.badMb];
  const ticks = scale > bands.badMb ? [...baseTicks, scale] : baseTicks;
  const labels = ticks.filter(
    (mb) =>
      mb === 0 ||
      mb === scale ||
      (mb / scale >= MIN_LABEL_GAP && mb / scale <= 1 - MIN_LABEL_GAP),
  );
  const pct = (mb: number) => `${Math.round((mb / scale) * 10000) / 100}%`;

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
        <div className="flex flex-col gap-1.5">
          <div className="relative">
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
                style={{ width: pct(coreMb) }}
                data-testid="home-memory-bar-core"
              />
              <div
                className={cn(
                  "h-full transition-[width] duration-500",
                  tone ? TONE_APP[tone] : "bg-muted-foreground/40",
                )}
                style={{ width: pct(appMb) }}
                data-testid="home-memory-bar-app"
              />
            </div>
            {/* Band boundaries cut through the bar as thin gaps. */}
            {ticks.filter((mb) => mb > 0 && mb < scale).map((mb) => (
              <span
                key={mb}
                aria-hidden="true"
                className="absolute inset-y-0 w-0.5 -translate-x-1/2 bg-card"
                style={{ left: pct(mb) }}
              />
            ))}
          </div>
          <div
            aria-hidden="true"
            className="relative h-3.5 font-mono text-[10px] leading-none text-muted-foreground tabular-nums"
          >
            {labels.map((mb) => (
              <span
                key={mb}
                className={cn(
                  "absolute top-0 whitespace-nowrap",
                  mb === 0
                    ? ""
                    : mb === scale
                      ? "-translate-x-full"
                      : "-translate-x-1/2",
                )}
                style={{ left: pct(mb) }}
                data-testid={`home-memory-tick-${mb}`}
              >
                {mb === scale ? `${mb} MB` : mb}
              </span>
            ))}
          </div>
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
      <span className="truncate font-mono tabular-nums" data-testid={testId}>
        {value}
      </span>
    </span>
  );
}
