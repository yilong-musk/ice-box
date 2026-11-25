// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Full-size map behind the landing probe card's expand button.
 *
 * This dialog draws no map of its own. Its map box is a slot
 * (`data-landing-slot`): the card's globe canvas (LandingMap) flies out of
 * the card into it, unrolling into a flat map on the way, and flies back on
 * close. So the dots you see in here are the very same element as on Home.
 *
 * An SVG overlay in the flat map's coordinate space (one unit per degree,
 * centred on the meridian the globe was facing) carries the two markers —
 * the user's own region and the probed exit — and the arc between them.
 *
 * The user's region is a local guess (system time zone, then the UI
 * language's region tag); nothing leaves the machine to find it. A region
 * missing from the centroid table simply gets no marker.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Dialog as DialogPrimitive } from "radix-ui";
import { XIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { formatLocale, t, useLanguagePreference } from "../lib/i18n";
import { countryFlagEmoji } from "../lib/flagEmoji";
import {
  LANDING_FLAT,
  getLandingGlobeView,
  landingFlatCenter,
  landingMapCentroid,
  setLandingFlatCenter,
  wrapLon,
} from "./LandingMap";

const { latTop: LAT_TOP, width: VIEW_W, height: VIEW_H } = LANDING_FLAT;

/** System time zone → ISO alpha-2, for the zones people commonly run. */
const TZ_COUNTRY: Readonly<Record<string, string>> = {
  "Asia/Shanghai": "CN", "Asia/Chongqing": "CN", "Asia/Chungking": "CN",
  "Asia/Harbin": "CN", "Asia/Urumqi": "CN", "Asia/Kashgar": "CN", PRC: "CN",
  "Asia/Hong_Kong": "HK", "Asia/Macau": "MO", "Asia/Taipei": "TW",
  "Asia/Tokyo": "JP", "Asia/Seoul": "KR", "Asia/Pyongyang": "KP",
  "Asia/Singapore": "SG", "Asia/Kuala_Lumpur": "MY", "Asia/Bangkok": "TH",
  "Asia/Ho_Chi_Minh": "VN", "Asia/Saigon": "VN", "Asia/Manila": "PH",
  "Asia/Jakarta": "ID", "Asia/Kolkata": "IN", "Asia/Calcutta": "IN",
  "Asia/Dubai": "AE", "Asia/Karachi": "PK", "Asia/Dhaka": "BD",
  "Asia/Kathmandu": "NP", "Asia/Ulaanbaatar": "MN", "Asia/Almaty": "KZ",
  "Asia/Jerusalem": "IL", "Asia/Riyadh": "SA", "Asia/Tehran": "IR",
  "Europe/London": "GB", "Europe/Dublin": "IE", "Europe/Paris": "FR",
  "Europe/Berlin": "DE", "Europe/Amsterdam": "NL", "Europe/Brussels": "BE",
  "Europe/Zurich": "CH", "Europe/Vienna": "AT", "Europe/Rome": "IT",
  "Europe/Madrid": "ES", "Europe/Lisbon": "PT", "Europe/Stockholm": "SE",
  "Europe/Oslo": "NO", "Europe/Helsinki": "FI", "Europe/Copenhagen": "DK",
  "Europe/Warsaw": "PL", "Europe/Prague": "CZ", "Europe/Budapest": "HU",
  "Europe/Bucharest": "RO", "Europe/Athens": "GR", "Europe/Kiev": "UA",
  "Europe/Kyiv": "UA", "Europe/Moscow": "RU", "Europe/Istanbul": "TR",
  "America/New_York": "US", "America/Chicago": "US", "America/Denver": "US",
  "America/Los_Angeles": "US", "America/Phoenix": "US",
  "America/Anchorage": "US", "Pacific/Honolulu": "US",
  "America/Toronto": "CA", "America/Vancouver": "CA",
  "America/Mexico_City": "MX", "America/Sao_Paulo": "BR",
  "America/Argentina/Buenos_Aires": "AR", "America/Santiago": "CL",
  "America/Bogota": "CO", "America/Lima": "PE",
  "Australia/Sydney": "AU", "Australia/Melbourne": "AU",
  "Australia/Brisbane": "AU", "Australia/Perth": "AU",
  "Australia/Adelaide": "AU", "Pacific/Auckland": "NZ",
  "Africa/Johannesburg": "ZA", "Africa/Cairo": "EG", "Africa/Lagos": "NG",
  "Africa/Nairobi": "KE", "Africa/Casablanca": "MA",
};

/** Best local guess at the user's own country, or null. */
export function guessUserCountry(): string | null {
  try {
    const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    const fromZone = zone ? TZ_COUNTRY[zone] : undefined;
    if (fromZone) return fromZone;
  } catch {
    // No Intl time zone support: fall through to the language tag.
  }
  const languages =
    typeof navigator !== "undefined"
      ? navigator.languages?.length
        ? navigator.languages
        : [navigator.language]
      : [];
  for (const tag of languages) {
    const region = tag?.split(/[-_]/)[1]?.toUpperCase();
    if (region && region.length === 2 && landingMapCentroid(region)) return region;
  }
  return null;
}

function regionName(code: string, locale: string): string {
  try {
    return new Intl.DisplayNames([locale], { type: "region" }).of(code) ?? code;
  } catch {
    return code;
  }
}

/** [lat, lon] → SVG coordinates, with longitude `lonC` at the centre. */
const project = ([lat, lon]: readonly [number, number], lonC: number) =>
  [wrapLon(lon - lonC) + 180, LAT_TOP - lat] as const;

/** Arc from `a` to `b`, bowed upward (toward the north) by a share of its length. */
function arcPath(a: readonly [number, number], b: readonly [number, number]): string {
  const [x1, y1] = a;
  const [x2, y2] = b;
  const d = Math.hypot(x2 - x1, y2 - y1);
  const lift = Math.max(3, d * 0.32);
  const cx = (x1 + x2) / 2;
  const cy = Math.max(1, (y1 + y2) / 2 - lift);
  return `M ${x1} ${y1} Q ${cx} ${cy} ${x2} ${y2}`;
}

/** Card radius of the small map (`rounded-lg`) and the dialog (`rounded-xl`). */
const ORIGIN_RADIUS = 8;
const DIALOG_RADIUS = 12;

/**
 * Sets the CSS variables the open/close keyframes start from (or end at), so
 * the dialog's frame grows out of the card's map box and shrinks back into
 * it, underneath the canvas that flies the same path (LandingMap).
 */
function syncZoomOrigin(node: HTMLElement) {
  const origin = document.querySelector<HTMLElement>('[data-testid="landing-map"]');
  const map = node.querySelector<HTMLElement>("[data-landing-slot]");
  const o = origin?.getBoundingClientRect();
  const cw = node.offsetWidth;
  const ch = node.offsetHeight;
  if (!map || !o || o.width <= 0 || o.height <= 0 || cw <= 0 || ch <= 0) {
    node.style.setProperty("--lm-dx", "0px");
    node.style.setProperty("--lm-dy", "24px");
    node.style.setProperty("--lm-s", "0.94");
    node.style.setProperty("--lm-clip", `inset(0px 0px 0px 0px round ${DIALOG_RADIUS}px)`);
    return;
  }
  const mw = map.offsetWidth;
  const mh = map.offsetHeight;
  const ml = map.offsetLeft;
  const mt = map.offsetTop;
  // Scale so the slot's width matches the card's; clip to the card's aspect.
  const s = o.width / mw;
  const visH = Math.min(mh, o.height / s);
  const mcx = ml + mw / 2 - cw / 2;
  const mcy = mt + mh / 2 - ch / 2;
  const dx = o.left + o.width / 2 - window.innerWidth / 2 - s * mcx;
  const dy = o.top + o.height / 2 - window.innerHeight / 2 - s * mcy;
  const top = mt + (mh - visH) / 2;
  const bottom = ch - top - visH;
  node.style.setProperty("--lm-dx", `${dx}px`);
  node.style.setProperty("--lm-dy", `${dy}px`);
  node.style.setProperty("--lm-s", `${s}`);
  node.style.setProperty(
    "--lm-clip",
    `inset(${top}px ${cw - ml - mw}px ${bottom}px ${ml}px round ${ORIGIN_RADIUS / s}px)`,
  );
}

export function LandingMapDialog({
  open,
  onOpenChange,
  country,
  ip,
  originCountry,
  originIp,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** ISO alpha-2 of the probed exit, or null while there is none. */
  country: string | null | undefined;
  ip: string | null | undefined;
  /** Measured origin (direct probe); null falls back to a local guess. */
  originCountry?: string | null;
  originIp?: string | null;
}) {
  // Measured origin when the direct probe has landed, else the local guess.
  const measuredOrigin = originIp && originCountry ? originCountry.toUpperCase() : null;
  const resolveUser = () => measuredOrigin ?? guessUserCountry();
  const { resolved: lang } = useLanguagePreference();
  const locale = formatLocale(lang);
  const [reduceMotion, setReduceMotion] = useState(false);
  useEffect(() => {
    if (typeof matchMedia !== "function") return;
    const query = matchMedia("(prefers-reduced-motion: reduce)");
    setReduceMotion(query.matches);
    const onChange = () => setReduceMotion(query.matches);
    query.addEventListener?.("change", onChange);
    return () => query.removeEventListener?.("change", onChange);
  }, []);

  // Central meridian: the midpoint between the user and the exit (shortest
  // way round), so both sit symmetrically around the middle of the map; the
  // globe's own facing when either is unknown. Picked once per open and kept
  // through the close animation so neither pins nor legend jump while closing.
  const [prevOpen, setPrevOpen] = useState(false);
  const [lonC, setLonC] = useState(() => landingFlatCenter());
  const [sessionUserCode, setSessionUserCode] = useState<string | null>(null);
  if (open !== prevOpen) {
    setPrevOpen(open);
    if (open) {
      const nextUser = resolveUser();
      setSessionUserCode(nextUser);
      const u = landingMapCentroid(nextUser);
      const x = ip ? landingMapCentroid(country) : null;
      setLonC(
        u && x
          ? wrapLon(u[1] + wrapLon(x[1] - u[1]) / 2)
          : wrapLon(getLandingGlobeView().lon),
      );
    }
  }
  if (open && measuredOrigin && sessionUserCode !== measuredOrigin) {
    setSessionUserCode(measuredOrigin);
  }
  // Publish the chosen meridian before paint and before LandingMap's
  // useEffect reads it to begin unrolling, without mutating globals in render.
  useLayoutEffect(() => {
    if (open) setLandingFlatCenter(lonC);
  }, [open, lonC]);

  // Measure the card's map when the dialog mounts (before its first paint)
  // and again as it starts closing, since Home may have scrolled or resized.
  const contentRef = useRef<HTMLDivElement | null>(null);
  const setContentRef = useCallback((node: HTMLDivElement | null) => {
    contentRef.current = node;
    if (node) syncZoomOrigin(node);
  }, []);
  useLayoutEffect(() => {
    if (!open && contentRef.current) syncZoomOrigin(contentRef.current);
  }, [open]);

  const userCode = sessionUserCode;
  const userInferred = userCode != null && measuredOrigin == null;
  const exitCode = country ? country.toUpperCase() : null;
  const userCentroid = landingMapCentroid(userCode);
  const exitCentroid = ip ? landingMapCentroid(exitCode) : null;
  const userPoint = userCentroid ? project(userCentroid, lonC) : null;
  const exitPoint = exitCentroid ? project(exitCentroid, lonC) : null;
  const sameSpot =
    userPoint && exitPoint && Math.hypot(userPoint[0] - exitPoint[0], userPoint[1] - exitPoint[1]) < 0.5;
  const arc = userPoint && exitPoint && !sameSpot ? arcPath(userPoint, exitPoint) : null;

  const userLabel = userCode
    ? `${countryFlagEmoji(userCode) ?? ""} ${regionName(userCode, locale)}${
        userInferred ? ` (${t("home.landingMap.inferred")})` : ""
      }`.trim()
    : t("home.landingMap.unknown");
  const exitLabel = ip
    ? exitCode
      ? `${countryFlagEmoji(exitCode) ?? ""} ${regionName(exitCode, locale)}`.trim()
      : t("home.landingMap.unknown")
    : t("home.landingMap.noExit");

  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay
          data-slot="dialog-overlay"
          className="landing-map-overlay fixed inset-0 z-50 bg-black/40 supports-backdrop-filter:backdrop-blur-sm"
        />
        <DialogPrimitive.Content
          ref={setContentRef}
          data-slot="dialog-content"
          data-testid="landing-map-dialog"
          className="landing-map-dialog fixed left-1/2 top-1/2 z-50 grid max-h-[90vh] w-[calc(100%-2rem)] max-w-5xl gap-3 rounded-xl border border-border bg-background p-5 shadow-lg outline-none"
        >
          <div data-lm-chrome className="flex items-start justify-between gap-3">
            <div className="grid gap-1">
              <DialogPrimitive.Title className="text-base font-semibold">
                {t("home.landingMap.title")}
              </DialogPrimitive.Title>
              <DialogPrimitive.Description className="text-sm text-muted-foreground">
                {t("home.landingMap.desc")}
              </DialogPrimitive.Description>
            </div>
            <DialogPrimitive.Close asChild>
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                aria-label={t("home.landingMap.close")}
              >
                <XIcon />
              </Button>
            </DialogPrimitive.Close>
          </div>

          {/* The card's globe canvas is moved in here (first child) while
              the dialog is open; see LandingMap. */}
          <div
            data-testid="landing-map-full"
            data-landing-slot
            className="landing-map-full relative aspect-[360/142] w-full select-none overflow-hidden rounded-lg"
          >
            <svg
              viewBox={`0 0 ${VIEW_W} ${VIEW_H}`}
              preserveAspectRatio="xMidYMid meet"
              className="absolute inset-0 size-full overflow-visible"
              aria-hidden="true"
            >
              {arc ? (
                <g data-testid="landing-map-arc">
                  <path
                    id="landing-map-arc-path"
                    d={arc}
                    className="landing-arc-base"
                    pathLength={100}
                  />
                  {/* No pathLength here: the dashes stay a fixed size in map
                      units, so a long arc gets more dashes, not longer gaps. */}
                  <path d={arc} className="landing-arc-flow" />
                  {reduceMotion ? null : (
                    <circle r={1.1} className="landing-arc-comet">
                      <animateMotion dur="2.4s" repeatCount="indefinite" rotate="auto">
                        <mpath href="#landing-map-arc-path" />
                      </animateMotion>
                    </circle>
                  )}
                </g>
              ) : null}
              {userPoint ? (
                <g
                  data-testid="landing-map-user"
                  data-country={userCode ?? undefined}
                  transform={`translate(${userPoint[0]} ${userPoint[1]})`}
                >
                  <circle r={1.6} className="landing-pin-user" />
                </g>
              ) : null}
              {exitPoint ? (
                <g
                  data-testid="landing-map-exit"
                  data-country={exitCode ?? undefined}
                  transform={`translate(${exitPoint[0]} ${exitPoint[1]})`}
                >
                  <circle r={1.6} className="landing-pin-ping" />
                  <circle r={1.6} className="landing-pin-exit" />
                </g>
              ) : null}
            </svg>
          </div>

          <div data-lm-chrome className="grid gap-2 text-sm sm:grid-cols-2">
            <div className="flex min-w-0 items-center gap-2">
              <span className="landing-legend-user size-2.5 shrink-0 rounded-full" />
              <span className="shrink-0 text-muted-foreground">{t("home.landingMap.you")}</span>
              <span className="min-w-0 truncate" data-testid="landing-map-user-label">
                {userLabel}
              </span>
            </div>
            <div className="flex min-w-0 items-center gap-2 sm:justify-end">
              <span className="size-2.5 shrink-0 rounded-full bg-primary" />
              <span className="shrink-0 text-muted-foreground">{t("home.landingMap.exit")}</span>
              <span
                className={cn("min-w-0 truncate", !ip && "text-muted-foreground")}
                data-testid="landing-map-exit-label"
              >
                {exitLabel}
                {ip ? (
                  <span
                    className="ml-2 font-mono tabular-nums text-muted-foreground"
                    data-selectable=""
                  >
                    {ip}
                  </span>
                ) : null}
              </span>
            </div>
          </div>
          <p data-lm-chrome className="text-xs text-muted-foreground">
            {t("home.landingMap.hint")}
          </p>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
