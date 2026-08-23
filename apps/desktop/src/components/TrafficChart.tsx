import { useEffect, useMemo, useRef, useState } from "react";
import { api, formatInvokeError, type TrafficSample } from "../api/tauri";
import { formatRate } from "../lib/traffic";

const MAX_POINTS = 60;

type Point = TrafficSample & { t: number };

type Props = {
  running: boolean;
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

export function TrafficChart({ running }: Props) {
  const [points, setPoints] = useState<Point[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [latest, setLatest] = useState<TrafficSample | null>(null);
  const inFlightRef = useRef(false);

  useEffect(() => {
    if (!running) {
      setPoints([]);
      setLatest(null);
      setError(null);
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
        setLatest(sample);
        setError(null);
        setPoints((prev) => {
          const next = [...prev, { ...sample, t: Date.now() }];
          return next.length > MAX_POINTS ? next.slice(-MAX_POINTS) : next;
        });
      } catch (e) {
        if (!cancelled) setError(formatInvokeError(e));
      } finally {
        inFlightRef.current = false;
      }
    };

    void tick();
    const id = window.setInterval(() => void tick(), 1000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [running]);

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
      <div className="traffic-panel">
        <h3 className="traffic-title">流量</h3>
        <p className="muted">启动内核后显示实时上下行曲线（最近 {MAX_POINTS} 秒）。</p>
      </div>
    );
  }

  return (
    <div className="traffic-panel">
      <div className="traffic-head">
        <h3 className="traffic-title">流量</h3>
        <div className="traffic-legend">
          <span className="legend-down">
            ↓ {latest ? formatRate(latest.down) : "—"}
          </span>
          <span className="legend-up">
            ↑ {latest ? formatRate(latest.up) : "—"}
          </span>
        </div>
      </div>
      {error && <p className="error traffic-error">{error}</p>}
      <svg
        className="traffic-chart"
        viewBox="0 0 320 72"
        preserveAspectRatio="none"
        aria-label="上下行流量曲线"
      >
        <line x1="0" y1="72" x2="320" y2="72" className="traffic-axis" />
        {downPath && (
          <path d={downPath} className="traffic-line traffic-line-down" fill="none" />
        )}
        {upPath && (
          <path d={upPath} className="traffic-line traffic-line-up" fill="none" />
        )}
      </svg>
      <p className="muted traffic-hint">
        峰值刻度 {formatRate(maxVal)} · 每秒采样（Clash API /traffic）
      </p>
    </div>
  );
}
