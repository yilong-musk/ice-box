import { useEffect, useMemo, useRef, useState } from "react";
import { api, formatInvokeError, type TrafficSample } from "../api/tauri";
import { formatRate } from "../lib/traffic";

const MAX_POINTS = 60;
/** Consecutive sample failures before surfacing a stale/error hint. */
const FAILURE_THRESHOLD = 3;

type Point = TrafficSample & { t: number };

type Props = {
  running: boolean;
  /** Pause sampling (e.g. while mode switch reloads Clash API). */
  paused?: boolean;
};

function buildPath(values: number[], width: number, height: number, max: number): string {
  if (values.length === 0) return "";
  const step = values.length <= 1 ? 0 : width / (values.length - 1);
  return values
    .map((v, i) => {
      const x = i * step;
      const y = height - (max > 0 ? (v / max) * height : 0);
      return `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}

export function TrafficChart({ running, paused = false }: Props) {
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

    // Drop any abandoned in-flight sample so unpause can resume.
    if (paused) {
      inFlightRef.current = false;
      return;
    }

    let cancelled = false;

    const tick = async () => {
      if (inFlightRef.current) return;
      inFlightRef.current = true;
      try {
        const sample = await api.getTrafficSample();
        if (cancelled) return;
        failCountRef.current = 0;
        setError(null);
        setLatest(sample);
        setPoints((prev) => {
          const next = [...prev, { ...sample, t: Date.now() }];
          return next.length > MAX_POINTS ? next.slice(-MAX_POINTS) : next;
        });
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

  const { upPath, downPath, maxVal } = useMemo(() => {
    const ups = points.map((p) => p.up);
    const downs = points.map((p) => p.down);
    const maxVal = Math.max(1, ...ups, ...downs);
    const w = 320;
    const h = 72;
    return {
      upPath: buildPath(ups, w, h, maxVal),
      downPath: buildPath(downs, w, h, maxVal),
      maxVal,
    };
  }, [points]);

  if (!running) {
    return (
      <div className="space-y-2">
        <h3 className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">
          流量
        </h3>
        <p className="muted text-sm">
          启动代理服务后显示实时上下行曲线（最近 {MAX_POINTS} 秒）。
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h3 className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">
          流量
        </h3>
        <div className="flex gap-3 font-mono text-xs">
          <span className="text-ok">
            ↓ {latest ? formatRate(latest.down) : "—"}
          </span>
          <span className="text-primary">
            ↑ {latest ? formatRate(latest.up) : "—"}
          </span>
        </div>
      </div>
      {error && <p className="error text-sm">采样中断：{error}</p>}
      <svg
        className="block h-16 w-full rounded-lg bg-muted/40"
        viewBox="0 0 320 72"
        preserveAspectRatio="none"
        aria-label="上下行流量曲线"
      >
        <line
          x1="0"
          y1="72"
          x2="320"
          y2="72"
          className="stroke-border"
          strokeWidth="1"
        />
        {downPath && (
          <path
            d={downPath}
            className="fill-none stroke-ok"
            strokeWidth="1.5"
            vectorEffect="non-scaling-stroke"
          />
        )}
        {upPath && (
          <path
            d={upPath}
            className="fill-none stroke-primary"
            strokeWidth="1.5"
            vectorEffect="non-scaling-stroke"
          />
        )}
      </svg>
      <p className="muted text-xs">
        峰值刻度 {formatRate(maxVal)} · 每秒采样（Clash API /traffic）
      </p>
    </div>
  );
}
