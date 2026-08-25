import { useEffect, useRef } from "react";
import {
  getDisplacementFilter,
  supportsBackdropFilterUrl,
} from "../lib/liquidGlass";

type Props = {
  proxyOn: boolean;
  busy: boolean;
  disabled: boolean;
  ariaLabel: string;
  title: string;
  subtitle: string;
  onClick: () => void;
};

const RADIUS = 22;
const DEPTH = 12;
const STRENGTH = 90;
const CHROMA = 2;

function setBackdrop(el: HTMLElement, value: string, webkit = false) {
  el.style.backdropFilter = value;
  if (webkit) {
    el.style.setProperty("-webkit-backdrop-filter", value);
  } else {
    el.style.removeProperty("-webkit-backdrop-filter");
  }
}

function applyLens(lens: HTMLElement, width: number, height: number) {
  const w = Math.max(Math.round(width), 2);
  const h = Math.max(Math.round(height), 2);
  const radius = Math.min(RADIUS, Math.floor(Math.min(w, h) / 2));
  const depth = Math.min(DEPTH, Math.floor(Math.min(w, h) / 6));

  if (supportsBackdropFilterUrl()) {
    const filterUrl = getDisplacementFilter({
      width: w,
      height: h,
      radius,
      depth,
      strength: STRENGTH,
      chromaticAberration: CHROMA,
    });
    // blur=0 — clear refraction, not frosted glass
    setBackdrop(lens, `url('${filterUrl}') brightness(1.08) saturate(1.35)`);
    lens.dataset.mode = "refract";
  } else {
    // WebKit: no SVG-in-backdrop; keep clear tint (avoid heavy frost)
    setBackdrop(lens, "saturate(1.25) brightness(1.06)", true);
    lens.dataset.mode = "clear";
  }
}

/** Home proxy toggle with nikdelvin-style liquid-glass lens. */
export function ProxyPowerButton({
  proxyOn,
  busy,
  disabled,
  ariaLabel,
  title,
  subtitle,
  onClick,
}: Props) {
  const rootRef = useRef<HTMLButtonElement>(null);
  const lensRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    const root = rootRef.current;
    const lens = lensRef.current;
    if (!root || !lens) return;

    const redraw = () => {
      const rect = root.getBoundingClientRect();
      applyLens(lens, rect.width, rect.height);
    };

    redraw();
    if (typeof ResizeObserver === "undefined") return;

    const ro = new ResizeObserver(redraw);
    ro.observe(root);
    return () => ro.disconnect();
  }, []);

  return (
    <button
      ref={rootRef}
      type="button"
      className={`proxy-power${proxyOn ? " on" : ""}${busy ? " busy" : ""}`}
      disabled={disabled}
      aria-pressed={proxyOn}
      aria-label={ariaLabel}
      onClick={onClick}
    >
      <span ref={lensRef} className="proxy-power-lens" aria-hidden="true" />
      <span className="proxy-power-sheen" aria-hidden="true" />
      <span className="proxy-power-core" aria-hidden="true">
        <span className="proxy-power-glyph" />
      </span>
      <span className="proxy-power-text">
        <span className="proxy-power-title">{title}</span>
        <span className="proxy-power-sub">{subtitle}</span>
      </span>
    </button>
  );
}
