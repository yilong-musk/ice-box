// SPDX-License-Identifier: GPL-3.0-or-later

import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ChevronRight, CircleCheck, Gauge, Star } from "lucide-react";
import {
  api,
  formatInvokeError,
  type NodeInfo,
} from "../api/client";
import { EmptyState } from "../components/EmptyState";
import { TitlebarActions } from "../components/TitlebarActions";
import { ErrorAlert, WarnAlert } from "../components/StatusAlert";
import { Button, buttonVariants } from "@/components/ui/button";
import { Card, CardContent, CardTitle } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";
import { isPhoneShell } from "@platform/shell";
import { t, useLanguagePreference, type ResolvedLanguage } from "../lib/i18n";
import { useGenerationGuard } from "../lib/generationGuard";
import { RUNTIME_STATUS_FALLBACK_MS, useRuntimeStore } from "../lib/runtimeStore";
import {
  CARD_GAP,
  CARD_HEIGHT,
  CARD_MIN_WIDTH,
  COMPACT_CARD_MIN_WIDTH,
  SECTION_HEADER_HEIGHT,
  cardWindow,
  gridColumns,
  sectionCardsOffset,
  sectionLayout,
  visibleSections,
} from "../lib/virtualNodes";
import {
  applyFavoriteToNodes,
  applyGroupNowToNodes,
  applySelectedTagToNodes,
  delayTestTagsForGroup,
  delayTestTagsForList,
  delayResultTone,
  formatDelay,
  groupLineupKey,
  hasStrategyGroups,
  isFavoriteNode,
  isGroupType,
  nodesEqual,
  orderMembersByFavorite,
  readExpandedGroups,
  readNodesSnapshot,
  sameNodeLineup,
  resolveSelectedTag,
  resolveExitTag,
  reuseUnchangedNodes,
  structuredTopLevel,
  writeExpandedGroups,
  writeNodesSnapshot,
  type DelayCell,
} from "../lib/nodes";

type Props = {
  onNavigate?: (tab: "subs") => void;
  /** When false the page stays mounted but stops polling. */
  active?: boolean;
  /** Render inside the phone Home page: no title-bar portal, inline actions. */
  embedded?: boolean;
};

/** HTML id-safe token derived from a possibly spaced group tag. */
function groupMembersDomId(tag: string): string {
  return `group-members-${encodeURIComponent(tag).replace(/%/g, "_")}`;
}

/** Delay text with its colour band; nothing for an untested node. */
function DelayText({ delay }: { delay: DelayCell }) {
  if (typeof delay === "number") {
    const tone = delayResultTone(delay);
    return (
      <span
        data-testid={`delay-${tone}`}
        className={cn(
          "font-mono text-xs tabular-nums",
          tone === "ok" && "text-ok",
          tone === "warn" && "text-warn",
          tone === "bad" && "text-destructive",
        )}
      >
        {formatDelay(delay)}
      </span>
    );
  }
  if (delay === "error") {
    return (
      <span data-testid="delay-error" className="text-xs text-destructive">
        {formatDelay(delay)}
      </span>
    );
  }
  if (delay === null) return null;
  return (
    <span data-testid="delay-testing" className="text-xs text-muted-foreground">
      {formatDelay(delay)}
    </span>
  );
}

function FavoriteButton({
  node,
  pending,
  onToggle,
}: {
  node: NodeInfo;
  pending: boolean;
  onToggle: (node: NodeInfo) => void;
}) {
  const favorite = isFavoriteNode(node);
  return (
    <button
      type="button"
      className={cn(
        // Same right inset as the card's delay button below it, so the star and
        // the delay/test glyph line up on the card's right edge.
        "relative z-10 -mr-1.5 flex h-6 shrink-0 items-center justify-end rounded px-1.5 transition-colors hover:bg-muted disabled:pointer-events-none disabled:opacity-50",
        favorite ? "text-warn" : "text-muted-foreground/70 hover:text-foreground",
      )}
      disabled={pending}
      aria-pressed={favorite}
      aria-label={t(favorite ? "nodes.unfavoriteAria" : "nodes.favoriteAria")}
      title={t(favorite ? "nodes.unfavorite" : "nodes.favorite")}
      onClick={() => onToggle(node)}
    >
      <Star className={cn("size-3.5", favorite && "fill-current")} aria-hidden="true" />
    </button>
  );
}

const EMPTY_DELAYS: Record<string, DelayCell> = {};
const EMPTY_LAYOUT: ReturnType<typeof sectionLayout<GridSection>> = { rows: [], height: 0 };
const EMPTY_RANGE = { start: 0, end: 0, offset: 0 };
/** Probes in flight during one delay test: a few nodes at a time keeps a long
 * list moving without flooding the core or the upstream links. The macOS tray
 * menu's delay test runs the same pool (`MAX_PROBES_IN_FLIGHT` in
 * `apps/desktop/src-tauri/src/tray_delay.rs`). */
const DELAY_TEST_CONCURRENCY = 4;
/** How long a finished delay result (a latency or a failure) stays on screen.
 * After that the card shows the untested state again: an old number says
 * little about the link now. */
const DELAY_RESULT_TTL_MS = 3 * 60_000;

/**
 * One block of the grid. A strategy group is a collapsible section whose
 * cards are its members; a flat profile is one headerless section of every
 * node; a grouped profile's nodes outside every group form a read-only
 * section at the end.
 */
type GridSection = {
  key: string;
  /** The strategy group this section shows; null for node-only sections. */
  group: NodeInfo | null;
  /** Card tags in display order (favorites first). */
  cards: string[];
  /** Whether clicking a card switches to it. */
  selectable: boolean;
  /** The card marked as in use. */
  current: string | null;
  header: boolean;
  collapsed: boolean;
  cardCount: number;
};

type NodeCardProps = {
  tag: string;
  node: NodeInfo | undefined;
  /** Group the card is a member of; null for a flat profile's node. */
  groupTag: string | null;
  current: boolean;
  selectable: boolean;
  running: boolean;
  busy: boolean;
  favoritePending: boolean;
  delay: DelayCell;
  onPick: (groupTag: string | null, tag: string) => void;
  onTestTag: (tag: string) => void;
  onToggleFavorite: (node: NodeInfo) => void;
};

const NodeCard = memo(function NodeCard({
  tag,
  node,
  groupTag,
  current,
  selectable,
  running,
  busy,
  favoritePending,
  delay,
  onPick,
  onTestTag,
  onToggleFavorite,
}: NodeCardProps) {
  // A member that is itself a strategy group (e.g. an urltest inside the main
  // selector) shows where it currently leads instead of a protocol.
  const nestedNow =
    node && isGroupType(node.outbound_type) ? node.group_now : null;
  const subtitle = nestedNow
    ? `→ ${nestedNow}`
    : node && isGroupType(node.outbound_type)
      ? node.outbound_type
      : (node?.outbound_type ?? "");
  const pickLabel = current
    ? t("nodes.memberCurrentAria", { member: tag })
    : groupTag
      ? t("nodes.setMemberAria", { member: tag, group: groupTag })
      : t("nodes.selectNodeAria", { node: tag });
  const pickTitle = current
    ? t(groupTag ? "nodes.currentExit" : "nodes.inUse")
    : groupTag
      ? running
        ? t("nodes.setExit")
        : t("nodes.setExitAfterStart")
      : t("nodes.select");

  return (
    <div
      data-testid="node-card"
      data-current={current ? "true" : undefined}
      className={cn(
        "relative flex min-w-0 flex-col justify-center rounded-lg border px-3 transition-colors",
        current
          ? "border-ok/60 bg-ok/10"
          : "border-border bg-muted/20",
        selectable && !current && "hover:border-foreground/25 hover:bg-muted/50",
      )}
      style={{ height: CARD_HEIGHT }}
    >
      {/* The whole card picks the node; the star and the delay stay their own
          buttons above it instead of nesting inside a button. */}
      {selectable ? (
        <button
          type="button"
          className="absolute inset-0 rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-default"
          disabled={busy || current}
          aria-current={current ? "true" : undefined}
          aria-label={pickLabel}
          title={pickTitle}
          onClick={() => onPick(groupTag, tag)}
        />
      ) : null}
      <div className="flex min-w-0 items-center gap-1">
        <span
          data-testid="node-card-title"
          className="min-w-0 flex-1 truncate text-sm font-medium"
          title={tag}
        >
          {tag}
        </span>
        {node ? (
          <FavoriteButton
            node={node}
            pending={favoritePending}
            onToggle={onToggleFavorite}
          />
        ) : null}
      </div>
      <div className="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
        {current ? (
          <CircleCheck
            className="size-3.5 shrink-0 text-ok"
            role="img"
            aria-label={t("nodes.inUse")}
          />
        ) : null}
        <span className="min-w-0 flex-1 truncate" title={subtitle}>
          {subtitle}
        </span>
        {running && node ? (
          <button
            type="button"
            className="relative z-10 -mr-1.5 flex h-5 shrink-0 items-center justify-end rounded px-1.5 hover:bg-muted disabled:pointer-events-none"
            disabled={busy}
            aria-label={t("nodes.testNodeAria", { node: tag })}
            title={t("nodes.test")}
            onClick={() => onTestTag(tag)}
          >
            {delay === null ? (
              <Gauge className="size-3.5 text-muted-foreground/70" aria-hidden="true" />
            ) : (
              <DelayText delay={delay} />
            )}
          </button>
        ) : (
          <DelayText delay={delay} />
        )}
      </div>
    </div>
  );
});

type SectionHeaderProps = {
  section: GridSection;
  membersId: string;
  running: boolean;
  busy: boolean;
  compact: boolean;
  delay: DelayCell;
  onToggleGroup: (tag: string, open: boolean) => void;
  onTestSection: (section: GridSection) => void;
};

const SectionHeader = memo(function SectionHeader({
  section,
  membersId,
  running,
  busy,
  compact,
  delay,
  onToggleGroup,
  onTestSection,
}: SectionHeaderProps) {
  const group = section.group;
  const expanded = !section.collapsed;
  const title = group?.tag ?? t("nodes.standalone");
  const heading = (
    <>
      {group ? (
        <ChevronRight
          className={cn(
            "size-4 shrink-0 text-muted-foreground transition-transform",
            expanded && "rotate-90",
          )}
          aria-hidden="true"
        />
      ) : null}
      <span
        data-testid="group-header-title"
        className="min-w-0 truncate text-sm font-semibold"
        title={title}
      >
        {title}
      </span>
      {group ? (
        <span className="shrink-0 rounded-md bg-muted px-1.5 py-0.5 text-[11px] leading-none text-muted-foreground">
          {group.outbound_type}
        </span>
      ) : (
        <span className="min-w-0 truncate text-xs text-muted-foreground" title={t("nodes.standaloneHint")}>
          {compact ? null : t("nodes.standaloneHint")}
        </span>
      )}
      <span className="flex-1" />
      {group?.group_now ? (
        <span
          className="min-w-0 max-w-[45%] truncate text-xs text-foreground/80"
          title={group.group_now}
        >
          → {group.group_now}
        </span>
      ) : group && !compact ? (
        <span className="shrink-0 text-xs text-muted-foreground">
          {t("nodes.notRunning")}
        </span>
      ) : null}
      {group ? <DelayText delay={delay} /> : null}
    </>
  );

  return (
    <div
      data-testid="group-header"
      className="flex items-center gap-2 border-b border-border/60"
      style={{ height: SECTION_HEADER_HEIGHT }}
    >
      {group ? (
        <div
          role="button"
          tabIndex={0}
          aria-label={group.tag}
          aria-expanded={expanded}
          aria-controls={membersId}
          title={expanded ? t("nodes.collapseMembers") : t("nodes.expandMembers")}
          className="flex h-full min-w-0 flex-1 cursor-pointer items-center gap-2 rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onClick={() => onToggleGroup(group.tag, !expanded)}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              onToggleGroup(group.tag, !expanded);
            }
          }}
        >
          {heading}
        </div>
      ) : (
        <div className="flex h-full min-w-0 flex-1 items-center gap-2">{heading}</div>
      )}
      <button
        type="button"
        className={cn(
          buttonVariants({ variant: "ghost", size: "icon-sm" }),
          "shrink-0 text-muted-foreground",
        )}
        disabled={!running || busy}
        aria-label={t("nodes.test")}
        title={
          group
            ? expanded
              ? t("nodes.testAllMembers")
              : t("nodes.testCurrent")
            : t("nodes.test")
        }
        onClick={() => onTestSection(section)}
      >
        <Gauge className="size-4" aria-hidden="true" />
      </button>
    </div>
  );
});

type SectionBlockProps = {
  section: GridSection;
  columns: number;
  cardRange: { start: number; end: number; offset: number };
  nodesByTag: ReadonlyMap<string, NodeInfo>;
  running: boolean;
  busy: boolean;
  favoritePending: boolean;
  compact: boolean;
  /** The delay of the node the group currently leads to, shown in the header
   * even while the section is collapsed. */
  headerDelay: DelayCell;
  /** Delay results by node tag; a group card shows its exit node's result.
   * Empty while collapsed so a delay test does not re-render sections that
   * show no cards. */
  delays: Record<string, DelayCell>;
  lang: ResolvedLanguage;
  onToggleGroup: (tag: string, open: boolean) => void;
  onTestSection: (section: GridSection) => void;
  onTestTag: (tag: string) => void;
  onPick: (groupTag: string | null, tag: string) => void;
  onToggleFavorite: (node: NodeInfo) => void;
};

const SectionBlock = memo(function SectionBlock({
  section,
  columns,
  cardRange,
  nodesByTag,
  running,
  busy,
  favoritePending,
  compact,
  headerDelay,
  delays,
  lang,
  onToggleGroup,
  onTestSection,
  onTestTag,
  onPick,
  onToggleFavorite,
}: SectionBlockProps) {
  const groupTag = section.group?.tag ?? null;
  const membersId = groupMembersDomId(section.key);
  return (
    <div lang={lang}>
      {section.header ? (
        <SectionHeader
          section={section}
          membersId={membersId}
          running={running}
          busy={busy}
          compact={compact}
          delay={headerDelay}
          onToggleGroup={onToggleGroup}
          onTestSection={onTestSection}
        />
      ) : null}
      {section.collapsed ? null : (
        <div
          id={membersId}
          role="group"
          data-card-count={section.cardCount}
          aria-label={
            groupTag
              ? t("nodes.membersAria", { group: groupTag })
              : section.header
                ? t("nodes.standalone")
                : undefined
          }
          className="grid"
          style={{
            gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
            gap: CARD_GAP,
            paddingTop: sectionCardsOffset(section) - (section.header ? SECTION_HEADER_HEIGHT : 0) + cardRange.offset,
          }}
        >
          {section.cards.slice(cardRange.start, cardRange.end).map((tag) => (
            <NodeCard
              key={`${section.key}::${tag}`}
              tag={tag}
              node={nodesByTag.get(tag)}
              groupTag={groupTag}
              current={tag === section.current}
              selectable={section.selectable}
              running={running}
              busy={busy || favoritePending}
              favoritePending={favoritePending}
              delay={delays[resolveExitTag(tag, nodesByTag)] ?? null}
              onPick={onPick}
              onTestTag={onTestTag}
              onToggleFavorite={onToggleFavorite}
            />
          ))}
        </div>
      )}
    </div>
  );
});

export function Nodes({ onNavigate, active = true, embedded = false }: Props) {
  // Row trimming follows the phone shell, not `embedded`: embedding is about
  // the card chrome, the narrow screen is what forces the compact row.
  const compact = isPhoneShell();
  const { resolved: lang } = useLanguagePreference();
  const { nextGeneration, isStale } = useGenerationGuard();
  const runtime = useRuntimeStore();
  const runtimeRef = useRef(runtime);
  runtimeRef.current = runtime;
  const [nodes, setNodes] = useState<NodeInfo[]>(
    () => readNodesSnapshot()?.nodes ?? [],
  );
  const [selectedTag, setSelectedTag] = useState(
    () => readNodesSnapshot()?.selectedTag ?? "",
  );
  const [running, setRunning] = useState(
    () => readNodesSnapshot()?.running ?? false,
  );
  const [listReady, setListReady] = useState(
    () => (readNodesSnapshot()?.nodes.length ?? 0) > 0,
  );
  const [delays, setDelays] = useState<Record<string, DelayCell>>({});
  // Expanded groups are remembered per lineup (another subscription has other
  // groups); everything starts collapsed the first time.
  const groupLineup = useMemo(() => groupLineupKey(nodes), [nodes]);
  const [expansion, setExpansion] = useState(() => {
    const lineup = groupLineupKey(readNodesSnapshot()?.nodes ?? []);
    return { lineup, groups: readExpandedGroups(lineup) };
  });
  if (expansion.lineup !== groupLineup) {
    // Adjust state while rendering (React's "previous render" pattern): the
    // stored groups of the new lineup apply before this render commits.
    setExpansion({ lineup: groupLineup, groups: readExpandedGroups(groupLineup) });
  }
  const expandedGroups = expansion.groups;
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [favoritePending, setFavoritePending] = useState(false);
  const [selectionPending, setSelectionPending] = useState(false);
  const [batchProgress, setBatchProgress] = useState<string | null>(null);
  const cancelRef = useRef(false);
  const favoriteInFlightRef = useRef(false);
  const selectionInFlightRef = useRef(false);
  const mountedRef = useRef(true);
  const activeRef = useRef(active);
  const delayRunRef = useRef(0);
  // Advances when the node lineup changes (another subscription or config).
  // Delay results and probes from an older lineup are dropped: a reused tag
  // may now name another server.
  const lineupRef = useRef(0);
  // When each finished delay result was written, by tag; drives the expiry.
  const delayStampsRef = useRef(new Map<string, number>());
  const nodesRef = useRef(nodes);
  nodesRef.current = nodes;
  const selectedTagRef = useRef(selectedTag);
  selectedTagRef.current = selectedTag;
  const runningRef = useRef(running);
  runningRef.current = running;
  const expandedRef = useRef(expandedGroups);
  expandedRef.current = expandedGroups;
  const onSelectRef = useRef<(tag: string) => void>(() => {});
  const onGroupSelectRef = useRef<(group: string, member: string) => void>(
    () => {},
  );
  const viewportRef = useRef<HTMLDivElement>(null);
  const [viewport, setViewport] = useState({ top: 0, height: 600, width: 0 });
  const viewportStateRef = useRef(viewport);
  viewportStateRef.current = viewport;
  // Rows render only while the page is shown. The hidden page catches up at
  // low priority and drops its rows then; the scroll position is restored from
  // `viewport` on the next visit.
  const listVisible = active && (runtime?.visible ?? true);
  const grouped = useMemo(() => hasStrategyGroups(nodes), [nodes]);
  const orderedNodes = useMemo(() => structuredTopLevel(nodes), [nodes]);
  const nodesByTag = useMemo(
    () => new Map(nodes.map((node) => [node.tag, node])),
    [nodes],
  );
  const memberOrder = useMemo(() => {
    const order = new Map<string, string[]>();
    for (const node of nodes) {
      if (node.group_all) {
        order.set(node.tag, orderMembersByFavorite(node.group_all, nodesByTag));
      }
    }
    return order;
  }, [nodes, nodesByTag]);
  const sections = useMemo<GridSection[]>(() => {
    if (!grouped) {
      const cards = orderedNodes.map((node) => node.tag);
      return [{
        key: "__all",
        group: null,
        cards,
        selectable: true,
        current: selectedTag || null,
        header: false,
        collapsed: false,
        cardCount: cards.length,
      }];
    }
    const result: GridSection[] = [];
    const standalone: string[] = [];
    for (const node of orderedNodes) {
      if (!isGroupType(node.outbound_type)) {
        standalone.push(node.tag);
        continue;
      }
      const cards = memberOrder.get(node.tag) ?? [];
      const collapsed = !expandedGroups.has(node.tag) || cards.length === 0;
      result.push({
        key: node.tag,
        group: node,
        cards,
        selectable: node.outbound_type === "selector",
        current: node.group_now ?? null,
        header: true,
        collapsed,
        cardCount: cards.length,
      });
    }
    if (standalone.length > 0) {
      // Nodes no group holds: grouped profiles have no flat selector for them,
      // so they can be tested and starred but not switched to.
      result.push({
        key: "__standalone",
        group: null,
        cards: standalone,
        selectable: false,
        current: null,
        header: true,
        collapsed: false,
        cardCount: standalone.length,
      });
    }
    return result;
  }, [grouped, orderedNodes, memberOrder, expandedGroups, selectedTag]);
  const columns = gridColumns(
    viewport.width,
    compact ? COMPACT_CARD_MIN_WIDTH : CARD_MIN_WIDTH,
  );
  const layout = useMemo(
    () => (listVisible ? sectionLayout(sections, columns) : EMPTY_LAYOUT),
    [listVisible, sections, columns],
  );
  const viewportTop = Math.max(0, Math.min(viewport.top, layout.height - viewport.height));
  const updateViewport = useCallback(() => {
    const el = viewportRef.current;
    if (!el) return;
    const next = {
      top: el.scrollTop,
      height: el.clientHeight || 600,
      width: el.clientWidth,
    };
    setViewport((prev) =>
      prev.top === next.top && prev.height === next.height && prev.width === next.width
        ? prev
        : next,
    );
  }, []);

  useLayoutEffect(() => {
    const el = viewportRef.current;
    if (!listVisible || !el) return;
    const height = el.clientHeight || 600;
    el.scrollTop = Math.max(0, Math.min(viewportStateRef.current.top, layout.height - height));
    updateViewport();
    const observer = new ResizeObserver(updateViewport);
    observer.observe(el);
    return () => observer.disconnect();
  }, [listVisible, listReady, layout.height, updateViewport]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      cancelRef.current = true;
    };
  }, []);

  const refresh = useCallback(async (opts?: { allowDuringFavorite?: boolean }) => {
    // A favorite write owns the optimistic list until its post-success refresh.
    // That refresh passes `allowDuringFavorite` because the single-flight guard
    // intentionally stays set until the list has caught up with the backend.
    if (favoriteInFlightRef.current && opts?.allowDuringFavorite !== true) return;
    const gen = nextGeneration();
    try {
      // Read the status directly instead of the shared store's cached value:
      // right after a state-changed event that cache still holds the previous
      // running/stopped state, and on a cold start it is empty. Either race
      // made the node list flash "proxy service not running" or keep stale
      // Test/Select controls. One status read per poll is cheap.
      const statusPromise = api.getStatus();
      const [n, settings, status] = await Promise.all([
        api.listNodes(),
        api.getSettings(),
        statusPromise,
      ]);
      if (isStale(gen) || !activeRef.current) return;
      const selected = resolveSelectedTag(settings.selected_tag, n);
      const runningNow = status?.core.status === "running";
      const prevNodes = nodesRef.current;
      if (prevNodes.length > 0 && !sameNodeLineup(prevNodes, n)) {
        lineupRef.current += 1;
        delayStampsRef.current.clear();
        setDelays((prev) => (Object.keys(prev).length === 0 ? prev : {}));
      }
      setNodes((prev) => reuseUnchangedNodes(prev, n));
      setSelectedTag((prev) => (prev === selected ? prev : selected));
      setRunning((prev) => (prev === runningNow ? prev : runningNow));
      writeNodesSnapshot({
        nodes: n,
        selectedTag: selected,
        running: runningNow,
      });
      setListReady(true);
      setError(null);
    } catch (e) {
      if (!isStale(gen) && activeRef.current) {
        setError(formatInvokeError(e));
        setListReady(true);
      }
    }
  }, [isStale, nextGeneration]);

  // The tray menu can switch the active exit (flat pick or group member) while
  // this page is on screen. Those selections are otherwise read only on poll
  // boundaries, so re-read on the announcement instead; a local switch in
  // flight publishes its own result on top.
  useEffect(() => {
    if (typeof api.listenStateChanged !== "function") return;
    let cancelled = false;
    let unlisten = () => {};
    void api
      .listenStateChanged(() => {
        if (!activeRef.current) return;
        void refresh();
      })
      .then((off) => {
        if (cancelled) off();
        else unlisten = off;
      });
    return () => {
      cancelled = true;
      unlisten();
    };
  }, [refresh]);

  // Leaving the tab keeps the list, its member lists, the expanded groups and
  // the delay results; only the rendered rows unmount (`listVisible`). The
  // delay results reset in `refresh` when the lineup changes, since a reused
  // tag may then name another server; the expanded groups follow the group
  // lineup and come back from storage.

  useEffect(() => {
    activeRef.current = active;
    if (!active) {
      nextGeneration();
      return;
    }
    void refresh();
    const id = window.setInterval(() => {
      if (document.visibilityState === "hidden") return;
      if (runtimeRef.current && !runtimeRef.current.visible) return;
      void refresh();
    }, RUNTIME_STATUS_FALLBACK_MS);
    return () => {
      activeRef.current = false;
      nextGeneration();
      window.clearInterval(id);
    };
  }, [active, nextGeneration, refresh]);

  const setGroupOpen = useCallback((tag: string, open: boolean) => {
    setExpansion((prev) => {
      if (prev.groups.has(tag) === open) return prev;
      const groups = new Set(prev.groups);
      if (open) groups.add(tag);
      else groups.delete(tag);
      return { lineup: prev.lineup, groups };
    });
  }, []);

  const savedExpansionRef = useRef(expansion);
  useEffect(() => {
    // Only a toggle writes: a lineup switch loads what storage already holds.
    if (savedExpansionRef.current === expansion) return;
    const lineupChanged = savedExpansionRef.current.lineup !== expansion.lineup;
    savedExpansionRef.current = expansion;
    if (!lineupChanged) writeExpandedGroups(expansion.lineup, expansion.groups);
  }, [expansion]);

  // Results are kept per node. A group (header or nested-group card) is not
  // given a copy: it shows the result of the node its exit resolves to at
  // render time, so switching the exit shows that node's result at once.
  function writeDelay(tag: string, value: DelayCell) {
    // Only a finished result ages out; an in-flight probe has no age yet.
    if (typeof value === "number" || value === "error") {
      delayStampsRef.current.set(tag, Date.now());
    } else {
      delayStampsRef.current.delete(tag);
    }
    setDelays((prev) => (prev[tag] === value ? prev : { ...prev, [tag]: value }));
  }

  // Drop each finished result DELAY_RESULT_TTL_MS after it was measured. One
  // timer for the earliest expiry; every change to the results reschedules it.
  useEffect(() => {
    const stamps = delayStampsRef.current;
    if (stamps.size === 0) return;
    let earliest = Infinity;
    for (const at of stamps.values()) earliest = Math.min(earliest, at);
    const id = window.setTimeout(() => {
      const now = Date.now();
      const expired: string[] = [];
      for (const [tag, at] of stamps) {
        if (now - at >= DELAY_RESULT_TTL_MS) expired.push(tag);
      }
      for (const tag of expired) stamps.delete(tag);
      if (expired.length === 0) return;
      setDelays((prev) => {
        const next = { ...prev };
        for (const tag of expired) delete next[tag];
        return next;
      });
    }, Math.max(0, earliest + DELAY_RESULT_TTL_MS - Date.now()));
    return () => window.clearTimeout(id);
  }, [delays]);

  function clearTestingDelays() {
    setDelays((prev) => {
      const next = { ...prev };
      let changed = false;
      for (const [tag, value] of Object.entries(next)) {
        if (value === "testing") {
          next[tag] = null;
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }

  async function testTags(requested: string[]) {
    // Probe the nodes behind any group tag, so the result lands on the node
    // the group header and nested-group cards look up.
    const byTag = new Map(nodesRef.current.map((node) => [node.tag, node]));
    const tags = [...new Set(requested.map((tag) => resolveExitTag(tag, byTag)))];
    if (tags.length === 0) return;
    const run = ++delayRunRef.current;
    const lineup = lineupRef.current;
    nextGeneration();
    cancelRef.current = false;
    setBusy(true);
    setError(null);
    const multi = tags.length > 1;
    const isCurrent = () =>
      run === delayRunRef.current &&
      lineup === lineupRef.current &&
      !cancelRef.current &&
      mountedRef.current;

    // Probes run a few at a time — one at a time made a long list take too
    // long. Workers claim tags in list order before awaiting, so probes still
    // leave in page order even though their results land out of order.
    let next = 0;
    let finished = 0;
    const progressText = () =>
      `${Math.min(finished + 1, tags.length)} / ${tags.length}`;
    if (multi) setBatchProgress(progressText());
    const probe = async () => {
      for (;;) {
        if (!isCurrent()) return;
        const index = next++;
        if (index >= tags.length) return;
        const tag = tags[index];
        writeDelay(tag, "testing");
        let value: DelayCell;
        try {
          const r = await api.testNodeDelay(tag);
          value = r.delay_ms;
        } catch {
          // Probe failure is shown on the delay cell only — same as batch.
          value = "error";
        }
        if (!isCurrent()) return;
        writeDelay(tag, value);
        finished += 1;
        if (multi) setBatchProgress(progressText());
      }
    };
    await Promise.all(
      Array.from(
        { length: Math.min(tags.length, DELAY_TEST_CONCURRENCY) },
        () => probe(),
      ),
    );

    if (!mountedRef.current || run !== delayRunRef.current) return;
    if (multi) setBatchProgress(null);
    setBusy(false);
  }

  const onTest = useCallback((node: NodeInfo) => {
    void (async () => {
      if (!isGroupType(node.outbound_type)) {
        await testTags([node.tag]);
        return;
      }
      const tags = delayTestTagsForGroup({
        expanded: expandedRef.current.has(node.tag),
        groupNow: node.group_now,
        groupAll: node.group_all,
      });
      if (tags.length === 0) {
        setError(t("nodes.noTestableGroup"));
        return;
      }
      await testTags(tags);
    })();
  }, []);

  const onTestTag = useCallback((tag: string) => {
    void testTags([tag]);
  }, []);

  const onTestSection = useCallback((section: GridSection) => {
    if (section.group) {
      onTest(section.group);
      return;
    }
    void testTags(section.cards);
  }, [onTest]);

  const onToggleFavorite = useCallback(
    (node: NodeInfo) => {
      void (async () => {
        if (favoriteInFlightRef.current || selectionInFlightRef.current) return;
        favoriteInFlightRef.current = true;
        setFavoritePending(true);
        // Invalidate any poll that started before the optimistic update.
        nextGeneration();
        const previous = nodesRef.current;
        const wasFavorite = isFavoriteNode(node);
        setError(null);
        setNodes(applyFavoriteToNodes(previous, node.tag, !wasFavorite));
        try {
          await api.setNodeFavorite(node.tag, !wasFavorite);
          if (mountedRef.current && activeRef.current) {
            await refresh({ allowDuringFavorite: true });
          }
        } catch (e) {
          // Keep the snapshot as the last backend result; reverting to
          // `previous` restores the rows without writing a favorite state.
          if (mountedRef.current) {
            setNodes(previous);
            setError(formatInvokeError(e));
          }
        } finally {
          // Stay single-flight through the post-success refresh: a second star
          // click must not race the list while it is catching up.
          favoriteInFlightRef.current = false;
          if (mountedRef.current) setFavoritePending(false);
        }
      })();
    },
    [nextGeneration, refresh],
  );

  const handlePick = useCallback((group: string | null, tag: string) => {
    if (group) void onGroupSelectRef.current(group, tag);
    else void onSelectRef.current(tag);
  }, []);

  async function onBatchTest() {
    if (!running || orderedNodes.length === 0) return;
    const tags = delayTestTagsForList(orderedNodes, expandedGroups);
    if (tags.length === 0) {
      setError(t("nodes.noTestable"));
      return;
    }
    await testTags(tags);
  }

  function onCancelBatch() {
    cancelRef.current = true;
    delayRunRef.current += 1;
    clearTestingDelays();
    setBatchProgress(null);
    setBusy(false);
  }

  async function onSelect(tag: string) {
    if (favoriteInFlightRef.current || selectionInFlightRef.current) return;
    selectionInFlightRef.current = true;
    setSelectionPending(true);
    nextGeneration();
    setBusy(true);
    setError(null);
    try {
      await api.setSelectedNode(tag);
      const nextNodes = applySelectedTagToNodes(nodesRef.current, tag);
      const snap = readNodesSnapshot();
      writeNodesSnapshot({
        nodes: nextNodes,
        selectedTag: tag,
        running: snap?.running ?? runningRef.current,
      });
      if (!mountedRef.current) return;
      setSelectedTag(tag);
      setNodes((prev) => (nodesEqual(prev, nextNodes) ? prev : nextNodes));
    } catch (e) {
      if (mountedRef.current) setError(formatInvokeError(e));
    } finally {
      selectionInFlightRef.current = false;
      if (mountedRef.current) {
        setSelectionPending(false);
        setBusy(false);
      }
    }
  }

  async function onGroupSelect(group: string, member: string) {
    if (favoriteInFlightRef.current || selectionInFlightRef.current) return;
    selectionInFlightRef.current = true;
    setSelectionPending(true);
    nextGeneration();
    setBusy(true);
    setError(null);
    try {
      await api.setGroupSelection(group, member);
      const nextNodes = applyGroupNowToNodes(nodesRef.current, group, member);
      // A pick in the main group also moves the selected node on the backend.
      // Publish it with the new exit so Home's outbound line follows at once,
      // even while this page is hidden and skips its refresh.
      const snap = readNodesSnapshot();
      let selected = snap?.selectedTag ?? selectedTagRef.current;
      try {
        selected = resolveSelectedTag((await api.getSettings()).selected_tag, nextNodes);
      } catch {
        // Keep the previous selection; the next poll catches up.
      }
      writeNodesSnapshot({
        nodes: nextNodes,
        selectedTag: selected,
        running: snap?.running ?? runningRef.current,
      });
      if (mountedRef.current) {
        setNodes((prev) => (nodesEqual(prev, nextNodes) ? prev : nextNodes));
        setSelectedTag((prev) => (prev === selected ? prev : selected));
      }
      if (mountedRef.current && activeRef.current) await refresh();
    } catch (e) {
      if (mountedRef.current) setError(formatInvokeError(e));
    } finally {
      selectionInFlightRef.current = false;
      if (mountedRef.current) {
        setSelectionPending(false);
        setBusy(false);
      }
    }
  }

  onSelectRef.current = onSelect;
  onGroupSelectRef.current = onGroupSelect;

  const visible = useMemo(
    () => visibleSections(layout.rows, viewportTop, viewport.height),
    [layout.rows, viewportTop, viewport.height],
  );

  const batchButton = (
    <>
      {embedded && busy && batchProgress ? (
        <span className="text-xs text-muted-foreground tabular-nums">
          {batchProgress}
        </span>
      ) : null}
      {/* The phone header shares its narrow row with the title and the
          progress/cancel controls, so the batch test is a short icon + label
          there; the full name stays as its accessible name and tooltip. */}
      <Button
        type="button"
        size="sm"
        variant={compact ? "ghost" : "outline"}
        disabled={!running || busy || favoritePending || nodes.length === 0}
        aria-label={compact ? t("nodes.batchTest") : undefined}
        title={compact ? t("nodes.batchTest") : undefined}
        onClick={() => void onBatchTest()}
      >
        {compact ? (
          <>
            <Gauge data-icon="inline-start" aria-hidden="true" />
            {t("nodes.test")}
          </>
        ) : (
          t("nodes.batchTest")
        )}
      </Button>
      {busy && batchProgress ? (
        <Button type="button" size="sm" variant="outline" onClick={onCancelBatch}>
          {t("common.cancel")}
        </Button>
      ) : null}
    </>
  );

  return (
    <div
      className={cn(
        "nodes-panel flex min-h-0 flex-1 flex-col gap-3",
        embedded && "h-full",
      )}
      data-testid="nodes-panel"
    >
      {active && !embedded ? (
        <TitlebarActions>{batchButton}</TitlebarActions>
      ) : null}
      {error && <ErrorAlert className="shrink-0">{error}</ErrorAlert>}

      {!running && nodes.length > 0 && (
        <WarnAlert className="shrink-0">
          {t("nodes.notRunningWarn")}
        </WarnAlert>
      )}

      {/* On the phone the header bar starts the card, so the card's own top
          padding would only add empty space above the title. */}
      <Card
        size="sm"
        className={cn(
          "flex min-h-0 flex-1 flex-col overflow-hidden",
          embedded && "pt-0",
        )}
      >
        {embedded ? (
          <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-1.5">
            {/* Same title style as the other phone Home cards. */}
            <CardTitle className="min-w-0 flex-1 truncate text-xs font-medium text-muted-foreground">
              {t("app.nav.nodes")}
            </CardTitle>
            {batchButton}
          </div>
        ) : null}
        <CardContent className={cn("flex min-h-0 flex-1 flex-col overflow-hidden", embedded && "pt-2")}>
          {!listReady ? (
            <p className="my-auto text-sm text-muted-foreground">
              {t("nodes.loading")}
            </p>
          ) : nodes.length === 0 ? (
            <EmptyState
              framed={false}
              className="my-auto"
              title={t("nodes.emptyTitle")}
              description={t("nodes.emptyDesc")}
              actionLabel={t("home.goToSubs")}
              onAction={() => onNavigate?.("subs")}
            />
          ) : listVisible ? (
            <ScrollArea
              viewportRef={viewportRef}
              onViewportScroll={updateViewport}
              type="scroll"
              scrollHideDelay={600}
              className="min-h-0 flex-1 overflow-hidden"
            >
              <div
                role="list"
                aria-label={t("nodes.listAria")}
                className="relative"
                style={{ height: layout.height }}
              >
                {visible.map(({ section, index, top }) => (
                  <div
                    key={section.key}
                    role="listitem"
                    aria-posinset={index + 1}
                    aria-setsize={sections.length}
                    className="absolute inset-x-0"
                    style={{ top }}
                  >
                    <SectionBlock
                      section={section}
                      columns={columns}
                      cardRange={
                        section.collapsed
                          ? EMPTY_RANGE
                          : cardWindow(
                              section.cardCount,
                              columns,
                              top + sectionCardsOffset(section),
                              viewportTop,
                              viewport.height,
                            )
                      }
                      nodesByTag={nodesByTag}
                      running={running}
                      busy={busy}
                      favoritePending={favoritePending || selectionPending}
                      compact={compact}
                      headerDelay={
                        section.group
                          ? (delays[resolveExitTag(section.group.tag, nodesByTag)] ?? null)
                          : null
                      }
                      delays={section.collapsed ? EMPTY_DELAYS : delays}
                      lang={lang}
                      onToggleGroup={setGroupOpen}
                      onTestSection={onTestSection}
                      onTestTag={onTestTag}
                      onPick={handlePick}
                      onToggleFavorite={onToggleFavorite}
                    />
                  </div>
                ))}
              </div>
            </ScrollArea>
          ) : null}
        </CardContent>
      </Card>
    </div>
  );
}
