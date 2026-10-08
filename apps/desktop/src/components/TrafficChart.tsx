// SPDX-License-Identifier: GPL-3.0-or-later

import { memo, useEffect, useMemo, useRef, useState } from "react";
import { Area, AreaChart, CartesianGrid, XAxis, YAxis } from "recharts";
import { api, formatInvokeError, type TrafficSample } from "../api/client";
import { formatRate } from "../lib/traffic";
import {
  type ChartConfig,
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
} from "@/components/ui/chart";
import { t, useLanguagePreference } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** Visible window; matches the backend ring buffer (`TRAFFIC_WINDOW_MS`). */
const WINDOW_SECONDS = 60;
const WINDOW_MS = WINDOW_SECONDS * 1000;
/** Consecutive snapshot failures before surfacing a stale/error hint. */
const FAILURE_THRESHOLD = 3;
/**
 * Catch-up / health poll. Live points arrive on `traffic://sample`; this
 * interval hydrates, fills gaps after the tab was hidden, resets on
 * generation change, and counts failures. A snapshot that matches the
 * current series does not render.
 */
const HEALTH_POLL_MS = 1000;

type Point = TrafficSample & { t: number };

type ChartRow = {
  t: number;
  down: number;
  up: number;
};

type Props = {
  running: boolean;
  /** Pause sampling (e.g. while mode switch reloads Clash API). */
  paused?: boolean;
  className?: string;
};

function formatClockPart(value: number): string {
  return value < 10 ? `0${value}` : String(value);
}

/** Local `HH:MM:SS` without `Intl` / `toLocaleTimeString`. */
function formatChartTime(value: unknown): string {
  const ms =
    typeof value === "number"
      ? value
      : typeof value === "string" && value !== ""
        ? Number(value)
        : NaN;
  if (!Number.isFinite(ms)) return "";
  const date = new Date(ms);
  if (Number.isNaN(date.getTime())) return "";
  return `${formatClockPart(date.getHours())}:${formatClockPart(date.getMinutes())}:${formatClockPart(date.getSeconds())}`;
}

function formatTooltipValue(value: unknown): string {
  return formatRate(Number(value));
}

const tooltipContent = (
  <ChartTooltipContent
    labelFormatter={(value) => formatChartTime(value)}
    indicator="dot"
    formatter={formatTooltipValue}
  />
);

function samplesEqual(
  a: TrafficSample | null,
  b: TrafficSample | null,
): boolean {
  if (a === b) return true;
  if (a == null || b == null) return false;
  return a.up === b.up && a.down === b.down;
}

function pointsEqual(a: Point[], b: Point[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    if (a[i].t !== b[i].t || a[i].up !== b[i].up || a[i].down !== b[i].down) {
      return false;
    }
  }
  return true;
}

function mergePoints(prev: Point[], incoming: Point[]): Point[] {
  // Expire only samples we were already holding. Incoming points are the
  // backend window (or the frozen demo series) and stay even when their
  // timestamps are not within the last 60s of this machine's clock.
  const cutoff = Date.now() - WINDOW_MS;
  const byTime = new Map<number, Point>();
  for (const p of prev) {
    if (p.t >= cutoff) byTime.set(p.t, p);
  }
  for (const p of incoming) byTime.set(p.t, p);
  return [...byTime.values()].sort((a, b) => a.t - b.t);
}

function raisePeak(
  current: TrafficSample | null,
  sample: TrafficSample,
): TrafficSample {
  const up = Math.max(current?.up ?? 0, sample.up);
  const down = Math.max(current?.down ?? 0, sample.down);
  if (current && current.up === up && current.down === down) return current;
  return { up, down };
}

function maxPeak(
  a: TrafficSample | null,
  b: TrafficSample | null,
): TrafficSample | null {
  if (a == null) return b;
  if (b == null) return a;
  const up = Math.max(a.up, b.up);
  const down = Math.max(a.down, b.down);
  if (a.up === up && a.down === down) return a;
  if (b.up === up && b.down === down) return b;
  return { up, down };
}

const TrafficPlot = memo(function TrafficPlot({
  chartData,
  maxVal,
  config,
  ariaLabel,
}: {
  chartData: ChartRow[];
  maxVal: number;
  config: ChartConfig;
  ariaLabel: string;
}) {
  return (
    <ChartContainer
      config={config}
      className="aspect-auto min-h-24 w-full flex-1"
      aria-label={ariaLabel}
    >
      <AreaChart
        accessibilityLayer
        data={chartData}
        margin={{ left: 12, right: 12 }}
      >
        <defs>
          <linearGradient id="fillTrafficDown" x1="0" y1="0" x2="0" y2="1">
            <stop
              offset="5%"
              stopColor="var(--color-down)"
              stopOpacity={0.6}
            />
            <stop
              offset="95%"
              stopColor="var(--color-down)"
              stopOpacity={0.1}
            />
          </linearGradient>
          <linearGradient id="fillTrafficUp" x1="0" y1="0" x2="0" y2="1">
            <stop offset="5%" stopColor="var(--color-up)" stopOpacity={0.45} />
            <stop
              offset="95%"
              stopColor="var(--color-up)"
              stopOpacity={0.1}
            />
          </linearGradient>
        </defs>
        <CartesianGrid vertical={false} />
        <YAxis hide domain={[0, maxVal]} />
        <XAxis
          dataKey="t"
          tickLine={false}
          axisLine={false}
          tickMargin={8}
          minTickGap={32}
          tickFormatter={formatChartTime}
        />
        <ChartTooltip cursor={false} content={tooltipContent} />
        <Area
          dataKey="down"
          type="monotone"
          fill="url(#fillTrafficDown)"
          stroke="var(--color-down)"
          isAnimationActive={false}
        />
        <Area
          dataKey="up"
          type="monotone"
          fill="url(#fillTrafficUp)"
          stroke="var(--color-up)"
          isAnimationActive={false}
        />
      </AreaChart>
    </ChartContainer>
  );
});

export function TrafficChart({ running, paused = false, className }: Props) {
  const { resolved: language } = useLanguagePreference();
  const chartConfig = useMemo<ChartConfig>(
    () => ({
      down: {
        label: t("traffic.down"),
        color: "var(--ok)",
      },
      up: {
        label: t("traffic.up"),
        color: "var(--primary)",
      },
    }),
    [language],
  );
  const ariaLabel = t("traffic.chartAria");
  const [points, setPoints] = useState<Point[]>([]);
  const [latest, setLatest] = useState<TrafficSample | null>(null);
  const [peak, setPeak] = useState<TrafficSample | null>(null);
  const [error, setError] = useState<string | null>(null);
  const pointsRef = useRef<Point[]>([]);
  const latestRef = useRef<TrafficSample | null>(null);
  const peakRef = useRef<TrafficSample | null>(null);
  /** Timestamp of the newest sample applied locally. */
  const latestTRef = useRef<number | null>(null);
  const inFlightRef = useRef(false);
  const failCountRef = useRef(0);
  const errorRef = useRef<string | null>(null);
  const cursorRef = useRef<number | null>(null);
  const generationRef = useRef<number | null>(null);

  const publish = (
    nextPoints: Point[],
    nextLatest: TrafficSample | null,
    nextPeak: TrafficSample | null,
  ) => {
    const pointsChanged = !pointsEqual(pointsRef.current, nextPoints);
    const latestChanged = !samplesEqual(latestRef.current, nextLatest);
    const peakChanged = !samplesEqual(peakRef.current, nextPeak);
    if (!pointsChanged && !latestChanged && !peakChanged) return;
    if (latestChanged) latestRef.current = nextLatest;
    if (peakChanged) peakRef.current = nextPeak;
    if (pointsChanged) {
      pointsRef.current = nextPoints;
      setPoints(nextPoints);
    }
    if (latestChanged) setLatest(nextLatest);
    if (peakChanged) setPeak(nextPeak);
  };

  useEffect(() => {
    if (!running) {
      pointsRef.current = [];
      latestRef.current = null;
      peakRef.current = null;
      latestTRef.current = null;
      failCountRef.current = 0;
      inFlightRef.current = false;
      cursorRef.current = null;
      generationRef.current = null;
      errorRef.current = null;
      setPoints([]);
      setLatest(null);
      setPeak(null);
      setError(null);
      return;
    }

    // Drop any abandoned in-flight snapshot so unpause can resume.
    if (paused) {
      inFlightRef.current = false;
      return;
    }

    let cancelled = false;
    let flightGen = 0;
    let unsubscribe = () => {};

    const noteHealthy = () => {
      failCountRef.current = 0;
      if (errorRef.current == null) return;
      errorRef.current = null;
      setError(null);
    };

    const snapshotIsBehind = (cursor: number | null | undefined) =>
      typeof latestTRef.current === "number" &&
      (typeof cursor !== "number" || latestTRef.current > cursor);

    const applyLiveSample = (sample: Point) => {
      if (cancelled || document.visibilityState === "hidden") return;
      noteHealthy();
      const prev = pointsRef.current;
      const last = prev[prev.length - 1];
      if (
        last &&
        last.t === sample.t &&
        last.up === sample.up &&
        last.down === sample.down
      ) {
        return;
      }
      if (cursorRef.current == null || sample.t > cursorRef.current) {
        cursorRef.current = sample.t;
      }
      const isNewest =
        latestTRef.current == null || sample.t >= latestTRef.current;
      if (isNewest) latestTRef.current = sample.t;
      publish(
        mergePoints(prev, [sample]),
        isNewest ? { up: sample.up, down: sample.down } : latestRef.current,
        raisePeak(peakRef.current, sample),
      );
    };

    const applySnapshot = (snap: {
      generation?: number;
      cursor?: number | null;
      points?: Point[];
      latest?: TrafficSample | null;
      peak?: TrafficSample | null;
    }) => {
      const incoming = snap.points ?? [];
      const nextGeneration =
        typeof snap.generation === "number" ? snap.generation : null;
      const generationChanged =
        nextGeneration !== null &&
        generationRef.current !== null &&
        generationRef.current !== nextGeneration;
      if (nextGeneration !== null) generationRef.current = nextGeneration;

      // Retarget clears history. The delta is the new window; drop local points.
      if (generationChanged) {
        cursorRef.current = typeof snap.cursor === "number" ? snap.cursor : null;
        latestTRef.current = cursorRef.current;
        publish(incoming, snap.latest ?? null, snap.peak ?? null);
        return;
      }

      const hydrating = cursorRef.current == null;
      const nextPoints =
        incoming.length > 0
          ? mergePoints(pointsRef.current, incoming)
          : pointsRef.current;
      const lastT =
        nextPoints.length > 0 ? nextPoints[nextPoints.length - 1].t : null;
      const candidate =
        typeof snap.cursor === "number" && (lastT == null || snap.cursor >= lastT)
          ? snap.cursor
          : lastT;
      if (
        typeof candidate === "number" &&
        (cursorRef.current == null || candidate > cursorRef.current)
      ) {
        cursorRef.current = candidate;
      }

      // A snapshot taken before a live sample must not rewind the label or scale.
      if (snapshotIsBehind(snap.cursor)) {
        publish(
          nextPoints,
          latestRef.current,
          maxPeak(snap.peak ?? null, peakRef.current),
        );
        return;
      }

      if (typeof snap.cursor === "number") latestTRef.current = snap.cursor;
      else if (hydrating) latestTRef.current = cursorRef.current;
      publish(nextPoints, snap.latest ?? null, snap.peak ?? null);
    };

    const tick = async () => {
      if (cancelled || inFlightRef.current || document.visibilityState === "hidden") {
        return;
      }
      const flight = flightGen;
      inFlightRef.current = true;
      try {
        const snap = await api.getTrafficSince(cursorRef.current);
        if (cancelled || flight !== flightGen) return;
        noteHealthy();
        applySnapshot(snap);
      } catch (e) {
        if (cancelled || flight !== flightGen) return;
        failCountRef.current += 1;
        if (failCountRef.current >= FAILURE_THRESHOLD) {
          const message = formatInvokeError(e);
          if (errorRef.current !== message) {
            errorRef.current = message;
            setError(message);
          }
        }
      } finally {
        if (flight === flightGen) inFlightRef.current = false;
      }
    };

    const onVisibility = () => {
      if (document.visibilityState === "visible") void tick();
    };

    void tick();
    const id = window.setInterval(() => {
      void tick();
    }, HEALTH_POLL_MS);
    document.addEventListener("visibilitychange", onVisibility);

    if (typeof api.listenTrafficSample === "function") {
      const pending = api.listenTrafficSample((sample) => {
        applyLiveSample(sample);
      });
      void Promise.resolve(pending).then(
        (unlisten) => {
          if (cancelled) unlisten();
          else unsubscribe = unlisten;
        },
        () => {
          // Event channel unavailable; the health poll still catches up.
        },
      );
    }

    return () => {
      cancelled = true;
      flightGen += 1;
      window.clearInterval(id);
      document.removeEventListener("visibilitychange", onVisibility);
      unsubscribe();
      inFlightRef.current = false;
    };
  }, [running, paused]);

  const chartData = useMemo<ChartRow[]>(
    () =>
      points.map((p) => ({
        t: p.t,
        down: p.down > 0 ? p.down : 0,
        up: p.up > 0 ? p.up : 0,
      })),
    [points],
  );
  const maxVal = Math.max(1, peak?.up ?? 0, peak?.down ?? 0);

  if (!running) {
    return (
      <div className={cn("flex min-h-0 flex-1 flex-col justify-center", className)}>
        <p className="muted text-sm">
          {t("traffic.idleHint", { n: WINDOW_SECONDS })}
        </p>
      </div>
    );
  }

  return (
    <div className={cn("flex min-h-0 flex-1 flex-col gap-2", className)}>
      <div className="flex shrink-0 justify-end gap-3 font-mono text-xs">
        <span className="text-ok">
          ↓ {latest ? formatRate(latest.down) : "—"}
        </span>
        <span className="text-primary">
          ↑ {latest ? formatRate(latest.up) : "—"}
        </span>
      </div>
      {error && (
        <p className="error shrink-0 text-sm">
          {t("traffic.samplingInterrupted", { error })}
        </p>
      )}
      <TrafficPlot
        chartData={chartData}
        maxVal={maxVal}
        config={chartConfig}
        ariaLabel={ariaLabel}
      />
      <p className="muted shrink-0 text-xs">
        {t("traffic.peak", {
          rate: formatRate(maxVal),
          n: WINDOW_SECONDS,
        })}
      </p>
    </div>
  );
}
