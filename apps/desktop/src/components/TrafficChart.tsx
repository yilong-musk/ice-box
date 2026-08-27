import { useEffect, useMemo, useRef, useState } from "react";
import { Area, AreaChart, CartesianGrid } from "recharts";
import { api, formatInvokeError, type TrafficSample } from "../api/tauri";
import { formatRate } from "../lib/traffic";
import {
  type ChartConfig,
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
} from "@/components/ui/chart";
import { cn } from "@/lib/utils";

/** Visible window; matches the backend ring buffer (`TRAFFIC_WINDOW_MS`). */
const WINDOW_SECONDS = 60;
/** Consecutive snapshot failures before surfacing a stale/error hint. */
const FAILURE_THRESHOLD = 3;

type Point = TrafficSample & { t: number };

type Props = {
  running: boolean;
  /** Pause snapshot polling (e.g. while mode switch reloads Clash API). */
  paused?: boolean;
  className?: string;
};

const chartConfig = {
  down: {
    label: "下行",
    color: "var(--ok)",
  },
  up: {
    label: "上行",
    color: "var(--primary)",
  },
} satisfies ChartConfig;

export function TrafficChart({ running, paused = false, className }: Props) {
  const [points, setPoints] = useState<Point[]>([]);
  const [latest, setLatest] = useState<TrafficSample | null>(null);
  const [error, setError] = useState<string | null>(null);
  const inFlightRef = useRef(false);
  const failCountRef = useRef(0);

  useEffect(() => {
    if (!running) {
      setPoints([]);
      setLatest(null);
      setError(null);
      failCountRef.current = 0;
      inFlightRef.current = false;
      return;
    }

    // Drop any abandoned in-flight snapshot so unpause can resume.
    if (paused) {
      inFlightRef.current = false;
      return;
    }

    let cancelled = false;

    const tick = async () => {
      if (inFlightRef.current) return;
      inFlightRef.current = true;
      try {
        const snap = await api.getTrafficSnapshot();
        if (cancelled) return;
        failCountRef.current = 0;
        setError(null);
        setLatest(snap.latest);
        setPoints(snap.points);
      } catch (e) {
        if (cancelled) return;
        // Brief Clash API drops are skipped; only surface after sustained failure.
        failCountRef.current += 1;
        if (failCountRef.current >= FAILURE_THRESHOLD) {
          setError(formatInvokeError(e));
        }
      } finally {
        inFlightRef.current = false;
      }
    };

    void tick();
    const id = window.setInterval(() => void tick(), 1000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
      // Allow the next effect run to sample even if this invoke never settles.
      inFlightRef.current = false;
    };
  }, [running, paused]);

  const { chartData, maxVal } = useMemo(() => {
    const maxVal = Math.max(1, ...points.map((p) => Math.max(p.up, p.down)));
    return {
      chartData: points.map((p, index) => ({
        second: index,
        down: p.down,
        up: p.up,
      })),
      maxVal,
    };
  }, [points]);

  if (!running) {
    return (
      <div className={cn("flex min-h-0 flex-1 flex-col justify-center", className)}>
        <p className="muted text-sm">
          启动代理服务后显示实时上下行曲线（最近 {WINDOW_SECONDS} 秒）。
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
      {error && <p className="error shrink-0 text-sm">采样中断：{error}</p>}
      <ChartContainer
        config={chartConfig}
        className="aspect-auto min-h-24 w-full flex-1"
        aria-label="上下行流量曲线"
      >
        <AreaChart
          accessibilityLayer
          data={chartData}
          margin={{ top: 8, right: 8, left: 8, bottom: 0 }}
        >
          <CartesianGrid vertical={false} />
          <ChartTooltip
            cursor={false}
            content={<ChartTooltipContent hideLabel indicator="line" />}
          />
          <Area
            dataKey="down"
            type="linear"
            fill="var(--color-down)"
            fillOpacity={0.2}
            stroke="var(--color-down)"
            strokeWidth={1.5}
            isAnimationActive={false}
          />
          <Area
            dataKey="up"
            type="linear"
            fill="var(--color-up)"
            fillOpacity={0.2}
            stroke="var(--color-up)"
            strokeWidth={1.5}
            isAnimationActive={false}
          />
        </AreaChart>
      </ChartContainer>
      <p className="muted shrink-0 text-xs">
        峰值刻度 {formatRate(maxVal)} · 最近 {WINDOW_SECONDS} 秒（后台持续采样）
      </p>
    </div>
  );
}
