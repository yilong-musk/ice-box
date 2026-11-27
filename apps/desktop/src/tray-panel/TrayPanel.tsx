// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Windows custom tray menu panel.
 *
 * Cross-platform parity: keep item order, labels, separators, icons, delay
 * badges, submenu hierarchy, and visual styling in sync with the macOS native
 * tray menu (`apps/desktop/src-tauri/src/tray.rs`, `tray_delay_view.rs`).
 * Any change to one platform's tray menu must be mirrored to the other in the
 * same change.
 */

import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { listen } from "@tauri-apps/api/event";
import { ArrowUp, Check, ChevronRight } from "lucide-react";
import {
  CHANGED_EVENT,
  HIDE_EVENT,
  SHOW_EVENT,
  trayPanelApi,
  type CardRect,
  type PanelDelayBadge,
  type PanelModel,
  type PanelNodeEntry,
  type PanelPlacement,
} from "./api";
import { aimingAt, type Point } from "./menuAim";
import { usePanelTheme } from "./usePanelTheme";

type SubmenuKind = "mode" | "nodes" | "subs";

const CANVAS_MAX_HEIGHT = 760;
/** Matches `.tp-card` vertical padding so submenu rows line up with their parent row. */
const CARD_PADDING_Y = 5;
const HOVER_OPEN_MS = 45;
const AIM_RECHECK_MS = 120;
/** Wait briefly on first cold open so the initial reveal already carries live items. */
const COLD_DATA_WAIT_MS = 120;
/** Short wait on warm reopen so fresh state lands before reveal without stalling. */
const WARM_DATA_WAIT_MS = 50;
const OTHER_ITEM = "";

function clearDelayBadges(prev: PanelModel): PanelModel {
  return {
    ...prev,
    nodeEntries: prev.nodeEntries.map((entry) =>
      entry.kind === "item"
        ? { ...entry, delay: null }
        : {
            ...entry,
            delay: null,
            members: entry.members.map((member) => ({
              ...member,
              delay: null,
            })),
          },
    ),
  };
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), Math.max(min, max));
}

function escapeAttrValue(value: string): string {
  if (typeof CSS !== "undefined" && typeof CSS.escape === "function") {
    return CSS.escape(value);
  }
  return value.replace(/["\\]/g, "\\$&");
}

function DelaySuffix({ delay }: { delay: PanelDelayBadge | null }) {
  if (!delay) return null;
  return (
    <span className="tp-delay" data-tone={delay.tone ?? undefined}>
      {" · "}
      {delay.text}
    </span>
  );
}

export function TrayPanel() {
  const [nativePlacement, setNativePlacement] = useState<PanelPlacement | null>(
    null,
  );
  const [pixelRatio, setPixelRatio] = useState(
    () => window.devicePixelRatio || 1,
  );
  // Convert CSS pixels back into native logical pixels when Windows text scale
  // or WebView zoom diverges from the monitor scale factor.
  const coordinateScale = nativePlacement
    ? pixelRatio / nativePlacement.scaleFactor
    : 1;
  const placement = useMemo(
    () =>
      nativePlacement && {
        ...nativePlacement,
        boundsTop: nativePlacement.boundsTop / coordinateScale,
        boundsBottom: nativePlacement.boundsBottom / coordinateScale,
        mainWidth: nativePlacement.mainWidth / coordinateScale,
        modeWidth: nativePlacement.modeWidth / coordinateScale,
        subsWidth: nativePlacement.subsWidth / coordinateScale,
        subWidth: nativePlacement.subWidth / coordinateScale,
        leafWidth: nativePlacement.leafWidth / coordinateScale,
        gap: nativePlacement.gap / coordinateScale,
        radius: nativePlacement.radius / coordinateScale,
      },
    [nativePlacement, coordinateScale],
  );

  const [model, setModel] = useState<PanelModel | null>(null);
  const [openSub, setOpenSub] = useState<SubmenuKind | null>(null);
  const [openGroupTag, setOpenGroupTag] = useState<string | null>(null);
  const [shownCount, setShownCount] = useState(0);
  const theme = usePanelTheme(shownCount);

  const mainRef = useRef<HTMLDivElement>(null);
  const subRef = useRef<HTMLDivElement>(null);
  const leafRef = useRef<HTMLDivElement>(null);

  const subTimer = useRef<number | undefined>(undefined);
  const leafTimer = useRef<number | undefined>(undefined);
  const trail = useRef<Point[]>([]);
  const subAimedAt = useRef<Point | null>(null);
  const leafAimedAt = useRef<Point | null>(null);
  const mainHovered = useRef<string | null>(null);
  const subHovered = useRef<string | null>(null);

  const lastModelKey = useRef("");
  const hasModelRef = useRef(false);
  const lastLayoutKey = useRef("");
  const visibleRef = useRef(false);
  const layoutFrame = useRef<number | undefined>(undefined);
  const pendingLayoutRef = useRef<Parameters<typeof trayPanelApi.layout>[0] | null>(
    null,
  );
  const shownSeq = useRef(0);
  const sessionRef = useRef(0);

  const placementRef = useRef(placement);
  placementRef.current = placement;
  const openSubRef = useRef(openSub);
  openSubRef.current = openSub;
  const openGroupRef = useRef(openGroupTag);
  openGroupRef.current = openGroupTag;

  useEffect(() => {
    const update = () => setPixelRatio(window.devicePixelRatio || 1);
    const media = window.matchMedia?.(`(resolution: ${pixelRatio}dppx)`);
    media?.addEventListener("change", update);
    window.addEventListener("resize", update);
    update();
    return () => {
      media?.removeEventListener("change", update);
      window.removeEventListener("resize", update);
    };
  }, [pixelRatio]);

  useEffect(() => {
    const onMove = (event: MouseEvent) => {
      const points = trail.current;
      points.push({ x: event.clientX, y: event.clientY });
      if (points.length > 3) points.shift();
    };
    window.addEventListener("mousemove", onMove);
    return () => window.removeEventListener("mousemove", onMove);
  }, []);

  const load = useCallback(async (): Promise<boolean> => {
    try {
      const next = await trayPanelApi.state();
      hasModelRef.current = true;
      const key = JSON.stringify(next);
      if (key === lastModelKey.current) return true;
      lastModelKey.current = key;
      setModel(next);
      return true;
    } catch (error) {
      console.error("[tray-panel] failed to fetch tray panel state", error);
      return false;
    }
  }, []);

  const clearTimers = useCallback(() => {
    window.clearTimeout(subTimer.current);
    window.clearTimeout(leafTimer.current);
    subTimer.current = undefined;
    leafTimer.current = undefined;
  }, []);

  const cancelPendingLayout = useCallback(() => {
    if (layoutFrame.current !== undefined) {
      window.cancelAnimationFrame(layoutFrame.current);
      layoutFrame.current = undefined;
    }
    pendingLayoutRef.current = null;
  }, []);

  const begin = useCallback(
    async (next: PanelPlacement) => {
      if (next.seq <= shownSeq.current) return;
      shownSeq.current = next.seq;
      const session = ++sessionRef.current;
      clearTimers();
      cancelPendingLayout();
      setOpenSub(null);
      setOpenGroupTag(null);
      const cold = !hasModelRef.current;
      const loading = load();
      const outcome = await Promise.race([
        loading.then((ok) => (ok ? ("ok" as const) : ("error" as const))),
        new Promise<"timeout">((resolve) =>
          window.setTimeout(
            () => resolve("timeout"),
            cold ? COLD_DATA_WAIT_MS : WARM_DATA_WAIT_MS,
          ),
        ),
      ]);
      // Ignore completion if `HIDE_EVENT` or a newer `SHOW_EVENT` superseded
      // this open while waiting for state.
      if (sessionRef.current !== session) return;
      if (cold && outcome === "error" && !hasModelRef.current) {
        void trayPanelApi.hide();
        return;
      }
      if (cold && outcome === "timeout") {
        // Release backend `PENDING_REVEAL` if a slow cold fetch later fails.
        void loading.then((ok) => {
          if (!ok && !hasModelRef.current && sessionRef.current === session) {
            void trayPanelApi.hide();
          }
        });
      }
      lastLayoutKey.current = "";
      visibleRef.current = true;
      setShownCount((count) => count + 1);
      setPixelRatio(window.devicePixelRatio || 1);
      setNativePlacement(next);
    },
    [cancelPendingLayout, clearTimers, load],
  );

  useEffect(() => {
    let disposed = false;
    const offs: Array<() => void> = [];
    void (async () => {
      const offShow = await listen<PanelPlacement>(SHOW_EVENT, (event) => {
        void begin(event.payload);
      });
      const offHide = await listen(HIDE_EVENT, () => {
        sessionRef.current += 1;
        visibleRef.current = false;
        clearTimers();
        cancelPendingLayout();
        setOpenSub(null);
        setOpenGroupTag(null);
        // Drop delay badges cached from the dismissed session so a fast warm
        // reopen never flashes numbers that `tray_delay::cancel_and_drop` threw
        // away while hidden.
        lastModelKey.current = "";
        setModel((prev) => (prev ? clearDelayBadges(prev) : null));
      });
      const offChanged = await listen(CHANGED_EVENT, () => {
        void load();
      });
      if (disposed) {
        offShow();
        offHide();
        offChanged();
        return;
      }
      offs.push(offShow, offHide, offChanged);
      const pending = await trayPanelApi.placement();
      if (pending && !disposed) {
        void begin(pending);
      }
    })();
    return () => {
      disposed = true;
      clearTimers();
      cancelPendingLayout();
      offs.forEach((off) => off());
    };
  }, [begin, cancelPendingLayout, clearTimers, load]);

  // Resolve currently open strategy group (if still present after a live reload).
  const openGroup = useMemo(() => {
    if (openSub !== "nodes" || !openGroupTag || !model) return null;
    const found = model.nodeEntries.find(
      (entry): entry is Extract<PanelNodeEntry, { kind: "group" }> =>
        entry.kind === "group" && entry.tag === openGroupTag,
    );
    return found ?? null;
  }, [model, openGroupTag, openSub]);

  // Drop open submenus if they become disabled after a live model refresh.
  useEffect(() => {
    if (!model) return;
    if (openSub === "nodes" && !model.nodes.enabled) {
      setOpenSub(null);
      setOpenGroupTag(null);
    } else if (openSub === "subs" && !model.subs.enabled) {
      setOpenSub(null);
      setOpenGroupTag(null);
    }
  }, [model, openSub]);

  const activeSubWidth = useMemo(() => {
    if (!placement || !openSub) return 0;
    if (openSub === "mode") return placement.modeWidth;
    if (openSub === "subs") return placement.subsWidth;
    return placement.subWidth;
  }, [openSub, placement]);

  const canvasMetrics = useMemo(() => {
    if (!placement) return null;
    const maxHeight = CANVAS_MAX_HEIGHT / coordinateScale;
    const top =
      placement.edge === "top"
        ? 0
        : Math.max(placement.boundsTop, -maxHeight);
    const bottom =
      placement.edge === "top"
        ? Math.min(placement.boundsBottom, maxHeight)
        : 0;
    const height = Math.max(0, bottom - top);
    return { top, bottom, height };
  }, [coordinateScale, placement]);

  const relayout = useCallback(() => {
    if (!visibleRef.current) return;
    const p = placementRef.current;
    const main = mainRef.current;
    if (!p || !main || !canvasMetrics) return;
    const sub = subRef.current;
    const leaf = leafRef.current;

    const mainHeight = main.getBoundingClientRect().height;
    if (mainHeight <= 0) return;

    const { top, bottom } = canvasMetrics;
    const canvasWidth =
      p.mainWidth + p.gap + p.subWidth + p.gap + p.leafWidth;
    const mainX =
      p.side === "left" ? p.leafWidth + p.gap + p.subWidth + p.gap : 0;
    const mainTop = p.edge === "top" ? 0 : -mainHeight;

    main.style.left = `${mainX}px`;
    main.style.top = `${mainTop - top}px`;

    const cards: CardRect[] = [
      { x: mainX, y: mainTop - top, width: p.mainWidth, height: mainHeight },
    ];

    let subX = 0;
    let subTop = mainTop;
    if (sub && openSubRef.current) {
      const subWidth = activeSubWidth || p.subWidth;
      const subHeight = sub.getBoundingClientRect().height;
      subX =
        p.side === "left"
          ? mainX - p.gap - subWidth
          : mainX + p.mainWidth + p.gap;
      const trigger = main.querySelector<HTMLElement>(
        `[data-sub-trigger="${openSubRef.current}"]`,
      );
      const triggerOffset = trigger
        ? trigger.getBoundingClientRect().top -
          main.getBoundingClientRect().top
        : 0;
      subTop = clamp(
        mainTop + triggerOffset - CARD_PADDING_Y,
        top,
        bottom - subHeight,
      );
      sub.style.left = `${subX}px`;
      sub.style.top = `${subTop - top}px`;
      if (subHeight > 0) {
        cards.push({
          x: subX,
          y: subTop - top,
          width: subWidth,
          height: subHeight,
        });
      }
    }

    if (leaf && sub && openGroupRef.current) {
      const leafHeight = leaf.getBoundingClientRect().height;
      const subWidth = activeSubWidth || p.subWidth;
      const leafX =
        p.side === "left"
          ? subX - p.gap - p.leafWidth
          : subX + subWidth + p.gap;
      const groupRow = sub.querySelector<HTMLElement>(
        `[data-group-trigger="${escapeAttrValue(openGroupRef.current)}"]`,
      );
      const groupOffset = groupRow
        ? groupRow.getBoundingClientRect().top -
          sub.getBoundingClientRect().top
        : 0;
      const leafTop = clamp(
        subTop + groupOffset - CARD_PADDING_Y,
        top,
        bottom - leafHeight,
      );
      leaf.style.left = `${leafX}px`;
      leaf.style.top = `${leafTop - top}px`;
      if (leafHeight > 0) {
        cards.push({
          x: leafX,
          y: leafTop - top,
          width: p.leafWidth,
          height: leafHeight,
        });
      }
    }

    const layout = {
      seq: p.seq,
      width: canvasWidth * coordinateScale,
      height: Math.ceil((bottom - top) * coordinateScale),
      offsetX: mainX * coordinateScale,
      offsetY: top * coordinateScale,
      cards: cards.map((card) => ({
        x: card.x * coordinateScale,
        y: card.y * coordinateScale,
        width: card.width * coordinateScale,
        height: card.height * coordinateScale,
      })),
      dark: theme.explicit,
    };
    const key = JSON.stringify(layout);
    if (key === lastLayoutKey.current) return;
    const firstLayout = lastLayoutKey.current === "";
    lastLayoutKey.current = key;
    pendingLayoutRef.current = layout;
    // Coalesce rapid layout changes (e.g. continuous wheel scrolling) onto the
    // already-scheduled animation frame instead of cancelling and starving
    // `SetWindowRgn` updates.
    if (layoutFrame.current !== undefined) return;
    let scheduled = true;
    const flush = () => {
      scheduled = false;
      layoutFrame.current = undefined;
      const latest = pendingLayoutRef.current;
      pendingLayoutRef.current = null;
      if (visibleRef.current && latest) {
        void trayPanelApi.layout(latest);
      }
    };
    const frameId = firstLayout
      ? // Wait one frame so WebView2 commits the new session's DOM/theme before
        // Win32 reveals the window.
        window.requestAnimationFrame(flush)
      : // Wait two frames so WebView2 paints newly opened or moved cards onto
        // the fixed canvas before Win32 updates SetWindowRgn.
        window.requestAnimationFrame(() => {
          const secondId = window.requestAnimationFrame(flush);
          if (scheduled) {
            layoutFrame.current = secondId;
          }
        });
    if (scheduled) {
      layoutFrame.current = frameId;
    }
  }, [activeSubWidth, canvasMetrics, coordinateScale, theme.explicit]);

  useEffect(() => cancelPendingLayout, [cancelPendingLayout]);

  useLayoutEffect(() => {
    relayout();
  });

  useEffect(() => {
    const observer = new ResizeObserver(() => relayout());
    if (mainRef.current) observer.observe(mainRef.current);
    if (subRef.current) observer.observe(subRef.current);
    if (leafRef.current) observer.observe(leafRef.current);
    return () => observer.disconnect();
  }, [relayout, openSub, openGroup, placement]);

  const setActiveSub = useCallback((next: SubmenuKind | null) => {
    window.clearTimeout(subTimer.current);
    window.clearTimeout(leafTimer.current);
    setOpenSub(next);
    if (next !== "nodes") {
      setOpenGroupTag(null);
    }
  }, []);

  const setActiveGroup = useCallback((next: string | null) => {
    window.clearTimeout(leafTimer.current);
    setOpenGroupTag(next);
  }, []);

  const switchSubWithAim = useCallback(
    (target: SubmenuKind | null) => {
      const expected = target ?? OTHER_ITEM;
      if (mainHovered.current !== expected) return;
      const points = trail.current;
      const current = points[points.length - 1];
      const subBox = subRef.current?.getBoundingClientRect();
      const side = placementRef.current?.side ?? "left";
      const stopped =
        subAimedAt.current !== null &&
        current !== undefined &&
        subAimedAt.current.x === current.x &&
        subAimedAt.current.y === current.y;
      if (
        !stopped &&
        subBox &&
        points.length >= 2 &&
        aimingAt(points[0], current, subBox, side)
      ) {
        subAimedAt.current = current;
        subTimer.current = window.setTimeout(
          () => switchSubWithAim(target),
          AIM_RECHECK_MS,
        );
        return;
      }
      subAimedAt.current = null;
      setActiveSub(target);
    },
    [setActiveSub],
  );

  const hoverMainSubmenu = useCallback(
    (target: SubmenuKind, enabled: boolean) => {
      window.clearTimeout(subTimer.current);
      mainHovered.current = enabled ? target : OTHER_ITEM;
      subAimedAt.current = null;
      if (!enabled) {
        if (openSubRef.current) switchSubWithAim(null);
        return;
      }
      if (openSubRef.current === target) return;
      if (openSubRef.current) {
        switchSubWithAim(target);
        return;
      }
      subTimer.current = window.setTimeout(
        () => setActiveSub(target),
        HOVER_OPEN_MS,
      );
    },
    [setActiveSub, switchSubWithAim],
  );

  const hoverMainPlain = useCallback(() => {
    if (!openSubRef.current || mainHovered.current === OTHER_ITEM) return;
    window.clearTimeout(subTimer.current);
    mainHovered.current = OTHER_ITEM;
    subAimedAt.current = null;
    switchSubWithAim(null);
  }, [switchSubWithAim]);

  const leaveMainRow = useCallback(() => {
    window.clearTimeout(subTimer.current);
    mainHovered.current = null;
  }, []);

  const switchGroupWithAim = useCallback(
    (target: string | null) => {
      const expected = target ?? OTHER_ITEM;
      if (subHovered.current !== expected) return;
      const points = trail.current;
      const current = points[points.length - 1];
      const leafBox = leafRef.current?.getBoundingClientRect();
      const side = placementRef.current?.side ?? "left";
      const stopped =
        leafAimedAt.current !== null &&
        current !== undefined &&
        leafAimedAt.current.x === current.x &&
        leafAimedAt.current.y === current.y;
      if (
        !stopped &&
        leafBox &&
        points.length >= 2 &&
        aimingAt(points[0], current, leafBox, side)
      ) {
        leafAimedAt.current = current;
        leafTimer.current = window.setTimeout(
          () => switchGroupWithAim(target),
          AIM_RECHECK_MS,
        );
        return;
      }
      leafAimedAt.current = null;
      setActiveGroup(target);
    },
    [setActiveGroup],
  );

  const hoverNodeGroup = useCallback(
    (tag: string) => {
      window.clearTimeout(leafTimer.current);
      subHovered.current = tag;
      leafAimedAt.current = null;
      if (openGroupRef.current === tag) return;
      if (openGroupRef.current) {
        switchGroupWithAim(tag);
        return;
      }
      leafTimer.current = window.setTimeout(
        () => setActiveGroup(tag),
        HOVER_OPEN_MS,
      );
    },
    [setActiveGroup, switchGroupWithAim],
  );

  const hoverNodePlain = useCallback(() => {
    if (!openGroupRef.current || subHovered.current === OTHER_ITEM) return;
    window.clearTimeout(leafTimer.current);
    subHovered.current = OTHER_ITEM;
    leafAimedAt.current = null;
    switchGroupWithAim(null);
  }, [switchGroupWithAim]);

  const leaveNodeRow = useCallback(() => {
    window.clearTimeout(leafTimer.current);
    subHovered.current = null;
  }, []);

  const triggerAction = useCallback((id: string) => {
    void trayPanelApi.action(id);
  }, []);

  // Keyboard navigation matching native macOS menus: Up/Down within the active
  // column, Left/Right to expand or collapse cascading submenus, Esc to close.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (event.key === "Escape") {
        event.preventDefault();
        if (openGroupRef.current) {
          const trigger = subRef.current?.querySelector<HTMLElement>(
            `[data-group-trigger="${escapeAttrValue(openGroupRef.current)}"]`,
          );
          setActiveGroup(null);
          trigger?.focus();
        } else if (openSubRef.current) {
          const trigger = mainRef.current?.querySelector<HTMLElement>(
            `[data-sub-trigger="${openSubRef.current}"]`,
          );
          setActiveSub(null);
          trigger?.focus();
        } else {
          void trayPanelApi.hide();
        }
        return;
      }
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        const scope =
          target?.closest<HTMLElement>(".tp-card") ??
          leafRef.current ??
          subRef.current ??
          mainRef.current;
        const items = Array.from(
          scope?.querySelectorAll<HTMLElement>(
            ".tp-item:not(:disabled)",
          ) ?? [],
        );
        if (items.length === 0) return;
        event.preventDefault();
        const index = target ? items.indexOf(target) : -1;
        const step = event.key === "ArrowDown" ? 1 : -1;
        const next =
          index < 0
            ? step > 0
              ? 0
              : items.length - 1
            : (index + step + items.length) % items.length;
        items[next]?.focus();
        return;
      }
      const expandKey =
        placementRef.current?.side === "left" ? "ArrowLeft" : "ArrowRight";
      const collapseKey = expandKey === "ArrowLeft" ? "ArrowRight" : "ArrowLeft";
      if (event.key === expandKey) {
        const subKey = target?.dataset.subTrigger as SubmenuKind | undefined;
        if (subKey) {
          event.preventDefault();
          setActiveSub(subKey);
          window.setTimeout(() => {
            subRef.current
              ?.querySelector<HTMLElement>(".tp-item:not(:disabled)")
              ?.focus();
          }, 0);
          return;
        }
        const groupTag = target?.dataset.groupTrigger;
        if (groupTag) {
          event.preventDefault();
          setActiveGroup(groupTag);
          window.setTimeout(() => {
            leafRef.current
              ?.querySelector<HTMLElement>(".tp-item:not(:disabled)")
              ?.focus();
          }, 0);
        }
      } else if (event.key === collapseKey) {
        if (openGroupRef.current && leafRef.current?.contains(target)) {
          event.preventDefault();
          const trigger = subRef.current?.querySelector<HTMLElement>(
            `[data-group-trigger="${escapeAttrValue(openGroupRef.current)}"]`,
          );
          setActiveGroup(null);
          trigger?.focus();
        } else if (openSubRef.current && subRef.current?.contains(target)) {
          event.preventDefault();
          const trigger = mainRef.current?.querySelector<HTMLElement>(
            `[data-sub-trigger="${openSubRef.current}"]`,
          );
          setActiveSub(null);
          trigger?.focus();
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [setActiveGroup, setActiveSub]);

  if (!placement || !model || !canvasMetrics) return null;

  const cardRadius = placement.radius;
  const maxCardHeight = canvasMetrics.height;

  return (
    <div
      className="relative h-full w-full"
      onMouseDown={(event) => {
        if (event.target !== event.currentTarget) return;
        if (openGroup) setActiveGroup(null);
        else if (openSub) setActiveSub(null);
        else void trayPanelApi.hide();
      }}
    >
      {/* Main tray menu column */}
      <div
        ref={mainRef}
        role="menu"
        data-testid="tray-panel-main"
        className="tp-card"
        style={{
          width: placement.mainWidth,
          maxHeight: maxCardHeight,
          borderRadius: cardRadius,
        }}
      >
        <div className="tp-scroll" onScroll={() => relayout()}>
          {model.update && (
            <button
              type="button"
              role="menuitem"
              className="tp-item"
              data-gutter="true"
              disabled={!model.update.enabled}
              onMouseEnter={hoverMainPlain}
              onMouseLeave={leaveMainRow}
              onClick={() => triggerAction(model.update!.id)}
            >
              <span className="tp-gutter tp-update-arrow" aria-hidden="true">
                <ArrowUp className="h-3.5 w-3.5" strokeWidth={2.4} />
              </span>
              <span className="min-w-0 flex-1 truncate">
                {model.update.label}
              </span>
            </button>
          )}

          <button
            type="button"
            role="menuitem"
            className="tp-item"
            disabled={!model.service.enabled}
            onMouseEnter={hoverMainPlain}
            onMouseLeave={leaveMainRow}
            onClick={() => triggerAction(model.service.id)}
          >
            <span className="min-w-0 flex-1 truncate">
              {model.service.label}
            </span>
          </button>

          <button
            type="button"
            role="menuitem"
            aria-haspopup="menu"
            aria-expanded={openSub === "mode"}
            data-sub-trigger="mode"
            data-submenu-open={openSub === "mode" || undefined}
            className="tp-item"
            disabled={!model.mode.enabled}
            onMouseEnter={() => hoverMainSubmenu("mode", model.mode.enabled)}
            onMouseLeave={leaveMainRow}
            onClick={() =>
              model.mode.enabled &&
              setActiveSub(openSub === "mode" ? null : "mode")
            }
          >
            <span className="min-w-0 flex-1 truncate">{model.mode.label}</span>
            <ChevronRight className="tp-chevron h-3.5 w-3.5" />
          </button>

          <button
            type="button"
            role="menuitem"
            aria-haspopup="menu"
            aria-expanded={openSub === "nodes"}
            data-sub-trigger="nodes"
            data-submenu-open={openSub === "nodes" || undefined}
            className="tp-item"
            disabled={!model.nodes.enabled}
            onMouseEnter={() => hoverMainSubmenu("nodes", model.nodes.enabled)}
            onMouseLeave={leaveMainRow}
            onClick={() =>
              model.nodes.enabled &&
              setActiveSub(openSub === "nodes" ? null : "nodes")
            }
          >
            <span className="min-w-0 flex-1 truncate">{model.nodes.label}</span>
            <ChevronRight className="tp-chevron h-3.5 w-3.5" />
          </button>

          <button
            type="button"
            role="menuitem"
            aria-haspopup="menu"
            aria-expanded={openSub === "subs"}
            data-sub-trigger="subs"
            data-submenu-open={openSub === "subs" || undefined}
            className="tp-item"
            disabled={!model.subs.enabled}
            onMouseEnter={() => hoverMainSubmenu("subs", model.subs.enabled)}
            onMouseLeave={leaveMainRow}
            onClick={() =>
              model.subs.enabled &&
              setActiveSub(openSub === "subs" ? null : "subs")
            }
          >
            <span className="min-w-0 flex-1 truncate">{model.subs.label}</span>
            <ChevronRight className="tp-chevron h-3.5 w-3.5" />
          </button>

          <button
            type="button"
            role="menuitem"
            className="tp-item"
            disabled={!model.copyCliProxy.enabled}
            onMouseEnter={hoverMainPlain}
            onMouseLeave={leaveMainRow}
            onClick={() => triggerAction(model.copyCliProxy.id)}
          >
            <span className="min-w-0 flex-1 truncate">
              {model.copyCliProxy.label}
            </span>
          </button>

          <button
            type="button"
            role="menuitem"
            className="tp-item"
            disabled={!model.openCliProxy.enabled}
            onMouseEnter={hoverMainPlain}
            onMouseLeave={leaveMainRow}
            onClick={() => triggerAction(model.openCliProxy.id)}
          >
            <span className="min-w-0 flex-1 truncate">
              {model.openCliProxy.label}
            </span>
          </button>

          <div className="tp-separator" role="separator" />

          <button
            type="button"
            role="menuitem"
            className="tp-item"
            disabled={!model.mcp.enabled}
            onMouseEnter={hoverMainPlain}
            onMouseLeave={leaveMainRow}
            onClick={() => triggerAction(model.mcp.id)}
          >
            <span className="min-w-0 flex-1 truncate">{model.mcp.label}</span>
          </button>

          <button
            type="button"
            role="menuitem"
            className="tp-item"
            disabled={!model.copyMcpConfig.enabled}
            onMouseEnter={hoverMainPlain}
            onMouseLeave={leaveMainRow}
            onClick={() => triggerAction(model.copyMcpConfig.id)}
          >
            <span className="min-w-0 flex-1 truncate">
              {model.copyMcpConfig.label}
            </span>
          </button>

          <div className="tp-separator" role="separator" />

          <button
            type="button"
            role="menuitem"
            className="tp-item"
            disabled={!model.show.enabled}
            onMouseEnter={hoverMainPlain}
            onMouseLeave={leaveMainRow}
            onClick={() => triggerAction(model.show.id)}
          >
            <span className="min-w-0 flex-1 truncate">{model.show.label}</span>
          </button>

          <button
            type="button"
            role="menuitem"
            className="tp-item"
            disabled={!model.quit.enabled}
            onMouseEnter={hoverMainPlain}
            onMouseLeave={leaveMainRow}
            onClick={() => triggerAction(model.quit.id)}
          >
            <span className="min-w-0 flex-1 truncate">{model.quit.label}</span>
          </button>
        </div>
      </div>

      {/* Level 2 submenu column (Proxy Mode / Nodes / Subscriptions) */}
      {openSub && (
        <div
          ref={subRef}
          role="menu"
          data-testid={`tray-panel-sub-${openSub}`}
          className="tp-card"
          onMouseEnter={() => window.clearTimeout(subTimer.current)}
          style={{
            width: activeSubWidth,
            maxHeight: maxCardHeight,
            borderRadius: cardRadius,
          }}
        >
          {openSub === "mode" && (
            <div className="tp-scroll">
              {model.modeItems.map((item) => (
                <button
                  key={item.id}
                  type="button"
                  role="menuitemradio"
                  aria-checked={item.checked}
                  className="tp-item"
                  data-gutter="true"
                  disabled={!item.enabled}
                  onClick={() => triggerAction(item.id)}
                >
                  <span className="tp-gutter" aria-hidden="true">
                    {item.checked && (
                      <Check className="h-3.5 w-3.5" strokeWidth={2.5} />
                    )}
                  </span>
                  <span className="min-w-0 flex-1 truncate">{item.label}</span>
                </button>
              ))}
            </div>
          )}

          {openSub === "subs" && (
            <div className="tp-scroll">
              {model.subItems.map((item) => (
                <button
                  key={item.id}
                  type="button"
                  role="menuitemradio"
                  aria-checked={item.checked}
                  className="tp-item"
                  data-gutter="true"
                  disabled={!item.enabled}
                  onClick={() => triggerAction(item.id)}
                >
                  <span className="tp-gutter" aria-hidden="true">
                    {item.checked && (
                      <Check className="h-3.5 w-3.5" strokeWidth={2.5} />
                    )}
                  </span>
                  <span className="min-w-0 flex-1 truncate">{item.label}</span>
                </button>
              ))}
            </div>
          )}

          {openSub === "nodes" && (
            <>
              {model.nodesDelay && (
                <>
                  <button
                    type="button"
                    role="menuitem"
                    className="tp-item shrink-0"
                    data-gutter={
                      model.nodeEntries.some((entry) => entry.kind === "item")
                        ? "true"
                        : undefined
                    }
                    disabled={!model.nodesDelay.enabled}
                    onMouseEnter={hoverNodePlain}
                    onMouseLeave={leaveNodeRow}
                    onClick={() => triggerAction(model.nodesDelay!.id)}
                  >
                    {model.nodeEntries.some(
                      (entry) => entry.kind === "item",
                    ) && <span className="tp-gutter" aria-hidden="true" />}
                    <span className="min-w-0 flex-1 truncate">
                      {model.nodesDelay.label}
                    </span>
                  </button>
                  <div className="tp-separator" role="separator" />
                </>
              )}
              <div className="tp-scroll" onScroll={() => relayout()}>
                {model.nodeEntries.map((entry) =>
                  entry.kind === "item" ? (
                    <button
                      key={entry.id}
                      type="button"
                      role="menuitemradio"
                      aria-checked={entry.checked}
                      className="tp-item"
                      data-gutter="true"
                      disabled={!entry.enabled}
                      onMouseEnter={hoverNodePlain}
                      onMouseLeave={leaveNodeRow}
                      onClick={() => triggerAction(entry.id)}
                    >
                      <span className="tp-gutter" aria-hidden="true">
                        {entry.checked && (
                          <Check className="h-3.5 w-3.5" strokeWidth={2.5} />
                        )}
                      </span>
                      <span className="min-w-0 flex-1 truncate">
                        {entry.label}
                      </span>
                      <DelaySuffix delay={entry.delay} />
                    </button>
                  ) : (
                    <button
                      key={entry.tag}
                      type="button"
                      role="menuitem"
                      aria-haspopup="menu"
                      aria-expanded={openGroup?.tag === entry.tag}
                      data-group-trigger={entry.tag}
                      data-submenu-open={
                        openGroup?.tag === entry.tag || undefined
                      }
                      className="tp-item"
                      onMouseEnter={() => hoverNodeGroup(entry.tag)}
                      onMouseLeave={leaveNodeRow}
                      onClick={() =>
                        setActiveGroup(
                          openGroup?.tag === entry.tag ? null : entry.tag,
                        )
                      }
                    >
                      <span className="min-w-0 flex-1 truncate">
                        {entry.label}
                      </span>
                      <DelaySuffix delay={entry.delay} />
                      <ChevronRight className="tp-chevron h-3.5 w-3.5" />
                    </button>
                  ),
                )}
              </div>
            </>
          )}
        </div>
      )}

      {/* Level 3 submenu column (Strategy Group members) */}
      {openGroup && (
        <div
          ref={leafRef}
          role="menu"
          data-testid="tray-panel-group"
          className="tp-card"
          onMouseEnter={() => {
            window.clearTimeout(subTimer.current);
            window.clearTimeout(leafTimer.current);
          }}
          style={{
            width: placement.leafWidth,
            maxHeight: maxCardHeight,
            borderRadius: cardRadius,
          }}
        >
          <button
            type="button"
            role="menuitem"
            className="tp-item shrink-0"
            data-gutter="true"
            disabled={!openGroup.delayButton.enabled}
            onClick={() => triggerAction(openGroup.delayButton.id)}
          >
            <span className="tp-gutter" aria-hidden="true" />
            <span className="min-w-0 flex-1 truncate">
              {openGroup.delayButton.label}
            </span>
          </button>
          <div className="tp-separator" role="separator" />
          <div className="tp-scroll">
            {openGroup.members.map((member) => (
              <button
                key={member.id}
                type="button"
                role="menuitemradio"
                aria-checked={member.checked}
                className="tp-item"
                data-gutter="true"
                disabled={!member.enabled}
                onClick={() => triggerAction(member.id)}
              >
                <span className="tp-gutter" aria-hidden="true">
                  {member.checked && (
                    <Check className="h-3.5 w-3.5" strokeWidth={2.5} />
                  )}
                </span>
                <span className="min-w-0 flex-1 truncate">{member.label}</span>
                <DelaySuffix delay={member.delay} />
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
