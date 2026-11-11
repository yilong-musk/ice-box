// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Home connection strip: how many connections the core holds right now and
 * the bytes it has relayed since it started, read from the same source as
 * the Connections page but without the rows (`getConnectionStats`).
 */

import { useEffect, useState } from "react";
import { ArrowDown, ArrowUp, Cable, type LucideIcon } from "lucide-react";
import { api, formatInvokeError } from "@/api/client";
import { Card, CardContent } from "@/components/ui/card";
import { t } from "@/lib/i18n";
import { formatBytes } from "@/lib/traffic";
import { cn } from "@/lib/utils";

const POLL_MS = 1000;
/** Consecutive failed polls before the figures are dropped as stale; matches
 * the traffic chart. */
const FAILURE_THRESHOLD = 3;

type Stats = { count: number; upload: number; download: number };

type Props = {
  running: boolean;
  /** When false the card keeps its last figures and stops polling. */
  active: boolean;
  /** Hold the last figures without polling, e.g. while the mode switch or a
   * start/stop is in flight; the traffic chart pauses on the same states. */
  paused?: boolean;
  className?: string;
};

function StatCell({
  icon: Icon,
  label,
  value,
  tone,
  testId,
  className,
}: {
  icon: LucideIcon;
  label: string;
  value: string;
  /** Chip and value colour; matches the Home traffic chart series. */
  tone: "neutral" | "down" | "up";
  testId: string;
  className?: string;
}) {
  return (
    <div className={cn("flex min-w-0 flex-1 items-center gap-2 px-3 first:pl-0 last:pr-0", className)}>
      <span
        aria-hidden="true"
        className={cn(
          "flex size-7 shrink-0 items-center justify-center rounded-md",
          TONE_CHIP[tone],
        )}
      >
        <Icon className="size-3.5" />
      </span>
      <div className="flex min-w-0 flex-col">
        <span className="truncate text-[11px] font-medium leading-4 text-muted-foreground">
          {label}
        </span>
        <span
          className={cn(
            "truncate font-mono text-[15px] font-semibold leading-6 tracking-tight tabular-nums",
            TONE_VALUE[tone],
          )}
          title={value}
          data-testid={testId}
        >
          {value}
        </span>
      </div>
    </div>
  );
}

const TONE_CHIP = {
  neutral: "bg-muted text-foreground",
  down: "bg-ok/10 text-ok",
  up: "bg-primary/10 text-primary",
} as const;

const TONE_VALUE = {
  neutral: "text-foreground",
  down: "text-ok",
  up: "text-primary",
} as const;

export function ConnectionStatsCard({ running, active, paused = false, className }: Props) {
  const [stats, setStats] = useState<Stats | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!running) {
      setStats(null);
      setError(null);
      return;
    }
    if (!active || paused) return;
    let cancelled = false;
    let timer = 0;
    let failures = 0;
    // Each poll is scheduled after the previous one settles, so a slow core
    // never has two reads in flight.
    const tick = async () => {
      if (document.visibilityState !== "hidden") {
        try {
          const next = await api.getConnectionStats();
          if (!cancelled) {
            failures = 0;
            setError(null);
            setStats(
              next.running
                ? {
                    count: next.count,
                    upload: next.upload_total,
                    download: next.download_total,
                  }
                : null,
            );
          }
        } catch (e) {
          // A single miss keeps the last figures; a run of them means the
          // core stopped answering, so stale numbers must not look live.
          failures += 1;
          if (!cancelled && failures >= FAILURE_THRESHOLD) {
            setStats(null);
            setError(formatInvokeError(e));
          }
        }
      }
      if (!cancelled) timer = window.setTimeout(() => void tick(), POLL_MS);
    };
    void tick();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [running, active, paused]);

  const dash = "—";
  return (
    <Card
      size="sm"
      className={cn("w-full min-w-0", className)}
      title={error ? t("traffic.samplingInterrupted", { error }) : undefined}
      data-testid="home-connection-stats"
    >
      <CardContent className="flex items-center divide-x divide-border/70">
        <StatCell
          icon={Cable}
          label={t("home.stats.connections")}
          value={stats ? String(stats.count) : dash}
          tone="neutral"
          testId="home-stats-connections"
          className="flex-none pr-4"
        />
        <StatCell
          icon={ArrowDown}
          label={t("home.stats.download")}
          value={stats ? formatBytes(stats.download) : dash}
          tone="down"
          testId="home-stats-download"
        />
        <StatCell
          icon={ArrowUp}
          label={t("home.stats.upload")}
          value={stats ? formatBytes(stats.upload) : dash}
          tone="up"
          testId="home-stats-upload"
        />
      </CardContent>
    </Card>
  );
}
