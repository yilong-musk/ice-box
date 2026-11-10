// SPDX-License-Identifier: GPL-3.0-or-later

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { X } from "lucide-react";
import {
  api,
  formatInvokeError,
  type ConnectionRow,
  type ConnectionsResponse,
} from "../api/client";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { ErrorAlert } from "../components/StatusAlert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardAction,
  CardContent,
  CardHeader,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemSeparator,
  ItemTitle,
} from "@/components/ui/item";
import { ScrollArea } from "@/components/ui/scroll-area";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import {
  connectionChain,
  connectionRule,
  connectionTarget,
  formatBytes,
  formatDuration,
  matchesKeyword,
  processName,
  sortConnections,
  type ConnectionSort,
} from "../lib/connections";
import { useGenerationGuard } from "../lib/generationGuard";
import { t, useLanguagePreference } from "../lib/i18n";

const POLL_MS = 1000;
/** Rows drawn at once. The list is redrawn every poll; search narrows a longer one. */
const MAX_ROWS = 300;

const SORTS: readonly ConnectionSort[] = ["newest", "traffic"];

/** Download and upload figures, coloured like the Home traffic chart. */
function TrafficPair({ down, up }: { down: number; up: number }) {
  return (
    <>
      <span className="text-ok" data-testid="traffic-down">
        ↓ {formatBytes(down)}
      </span>{" "}
      <span className="text-primary" data-testid="traffic-up">
        ↑ {formatBytes(up)}
      </span>
    </>
  );
}

function documentHidden(): boolean {
  return document.visibilityState === "hidden";
}

function ConnectionItem({
  row,
  nowMs,
  onClose,
}: {
  row: ConnectionRow;
  nowMs: number;
  onClose: (id: string) => void;
}) {
  const target = connectionTarget(row) || t("common.dash");
  const chain = connectionChain(row);
  const rule = connectionRule(row);
  const process = processName(row.process_path);
  return (
    <Item size="sm" className="pl-0 pr-3" data-testid="connection-row">
      <ItemContent className="min-w-0">
        <ItemTitle
          className="w-full min-w-0"
          title={row.source ? `${row.source} → ${target}` : target}
        >
          <span className="truncate">{target}</span>
          <Badge variant="outline" className="font-mono">
            {row.network.toUpperCase()}
          </Badge>
          {row.start_ms != null ? (
            <span
              className="shrink-0 text-xs font-normal tabular-nums text-muted-foreground"
              data-testid="connection-age"
            >
              {formatDuration(nowMs - row.start_ms)}
            </span>
          ) : null}
        </ItemTitle>
        <ItemDescription>
          {/* ItemDescription is a line-clamped -webkit-box, so the flex row
              lives in an inner span. */}
          <span className="flex min-w-0 items-center gap-1.5">
            {chain ? (
              <span
                className="max-w-[55%] shrink-0 truncate"
                title={chain}
                data-testid="connection-chain"
              >
                {chain}
              </span>
            ) : null}
            {rule ? (
              <>
                <span className="shrink-0" aria-hidden="true">
                  ·
                </span>
                <span className="min-w-0 shrink-[2] truncate" title={row.rule}>
                  {rule}
                </span>
              </>
            ) : null}
            {process ? (
              <>
                <span className="shrink-0" aria-hidden="true">
                  ·
                </span>
                <span className="min-w-0 shrink-[3] truncate" title={row.process_path}>
                  {process}
                </span>
              </>
            ) : null}
          </span>
        </ItemDescription>
      </ItemContent>
      <ItemActions>
        {/* Fixed width so the rows do not shift as the figures change. */}
        <span
          className="w-40 shrink-0 whitespace-nowrap text-right text-xs tabular-nums text-muted-foreground"
          data-testid="connection-traffic"
        >
          <TrafficPair down={row.download} up={row.upload} />
        </span>
        <Button
          type="button"
          size="icon-sm"
          variant="ghost"
          aria-label={t("connections.close", { target })}
          title={t("connections.close", { target })}
          onClick={() => onClose(row.id)}
        >
          <X aria-hidden="true" />
        </Button>
      </ItemActions>
    </Item>
  );
}

export function Connections({ active = true }: { active?: boolean }) {
  useLanguagePreference();
  const { nextGeneration, isStale } = useGenerationGuard();
  const [data, setData] = useState<ConnectionsResponse | null>(null);
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [error, setError] = useState<string | null>(null);
  const [keyword, setKeyword] = useState("");
  const [sort, setSort] = useState<ConnectionSort>("newest");
  const [confirmCloseAll, setConfirmCloseAll] = useState(false);
  const [closingAll, setClosingAll] = useState(false);
  const activeRef = useRef(active);
  activeRef.current = active;

  const refresh = useCallback(async () => {
    if (!activeRef.current || documentHidden()) return;
    const gen = nextGeneration();
    try {
      const next = await api.listConnections();
      if (isStale(gen) || !activeRef.current) return;
      setData(next);
      setNowMs(Date.now());
      setError(null);
    } catch (e) {
      if (!isStale(gen) && activeRef.current) setError(formatInvokeError(e));
    }
  }, [isStale, nextGeneration]);

  useEffect(() => {
    if (!active) {
      // Drop an in-flight poll.
      nextGeneration();
      return;
    }
    let cancelled = false;
    let timer = 0;
    // Each poll is scheduled after the previous one settles, so a slow core
    // never has two reads in flight.
    const tick = async () => {
      await refresh();
      if (!cancelled) timer = window.setTimeout(() => void tick(), POLL_MS);
    };
    void tick();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [active, nextGeneration, refresh]);

  const closeOne = useCallback(
    (id: string) => {
      // A poll already in flight still lists this connection; drop its result
      // so the row does not come back for a second.
      nextGeneration();
      setData((prev) =>
        prev
          ? { ...prev, connections: prev.connections.filter((row) => row.id !== id) }
          : prev,
      );
      void api.closeConnection(id).catch((e) => {
        if (activeRef.current) setError(formatInvokeError(e));
      });
    },
    [nextGeneration],
  );

  const closeAll = useCallback(async () => {
    setClosingAll(true);
    nextGeneration();
    try {
      await api.closeAllConnections();
      setData((prev) => (prev ? { ...prev, connections: [] } : prev));
      setError(null);
    } catch (e) {
      setError(formatInvokeError(e));
    } finally {
      setClosingAll(false);
    }
  }, [nextGeneration]);

  const all = data?.connections;
  const sorted = useMemo(() => {
    const rows = (all ?? []).filter((row) => matchesKeyword(row, keyword));
    return sortConnections(rows, sort);
  }, [all, keyword, sort]);
  const shown = sorted.length > MAX_ROWS ? sorted.slice(0, MAX_ROWS) : sorted;
  const count = all?.length ?? 0;
  const filtering = keyword.trim() !== "";

  let emptyText: string | null = null;
  if (data && !data.running) emptyText = t("connections.stopped");
  else if (data && count === 0) emptyText = t("connections.empty");
  else if (data && sorted.length === 0) emptyText = t("connections.emptyFiltered");

  return (
    <div
      className="connections-panel flex min-h-0 flex-1 flex-col gap-3"
      data-testid="connections-panel"
    >
      {error && <ErrorAlert className="shrink-0">{error}</ErrorAlert>}

      <Card size="sm" className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <CardHeader className="shrink-0">
          <CardAction className="col-span-2 col-start-1 flex w-full flex-wrap items-center justify-end gap-2">
            <Input
              type="search"
              className="min-w-48 flex-1"
              placeholder={t("connections.searchPlaceholder")}
              aria-label={t("connections.searchAria")}
              value={keyword}
              onChange={(e) => setKeyword(e.target.value)}
            />
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="shrink-0"
              disabled={closingAll || count === 0}
              onClick={() => setConfirmCloseAll(true)}
            >
              {t("connections.closeAll")}
            </Button>
          </CardAction>
        </CardHeader>
        <CardContent className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden">
          <div className="flex w-full min-w-0 shrink-0 flex-wrap items-center gap-x-3 gap-y-2">
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              spacing={2}
              value={sort}
              onValueChange={(value) => {
                // Radix reports "" when the pressed item is clicked again.
                if (value) setSort(value as ConnectionSort);
              }}
              className="h-auto shrink-0"
              aria-label={t("connections.sortAria")}
            >
              {SORTS.map((id) => (
                <ToggleGroupItem key={id} value={id}>
                  {t(`connections.sort.${id}`)}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
            <span
              className="ml-auto min-w-0 truncate text-xs tabular-nums text-muted-foreground"
              data-testid="connections-summary"
            >
              {filtering
                ? t("connections.countFiltered", { shown: sorted.length, count })
                : t("connections.count", { count })}
              {data?.running ? (
                <>
                  {` · ${t("connections.totals")} `}
                  <TrafficPair down={data.download_total} up={data.upload_total} />
                </>
              ) : null}
            </span>
          </div>

          {emptyText ? (
            <p
              className="my-auto self-center text-sm text-muted-foreground"
              data-testid="connections-empty"
            >
              {emptyText}
            </p>
          ) : (
            <ScrollArea
              type="scroll"
              scrollHideDelay={600}
              className="min-h-0 flex-1 overflow-hidden"
            >
              <ItemGroup aria-label={t("connections.listAria")} className="gap-0">
                {shown.map((row, index) => (
                  <div key={row.id}>
                    {index > 0 ? <ItemSeparator className="my-0" /> : null}
                    <ConnectionItem
                      row={row}
                      nowMs={nowMs}
                      onClose={closeOne}
                    />
                  </div>
                ))}
              </ItemGroup>
              {sorted.length > MAX_ROWS ? (
                <p className="py-3 text-center text-xs text-muted-foreground">
                  {t("connections.truncated", { limit: MAX_ROWS })}
                </p>
              ) : null}
            </ScrollArea>
          )}
        </CardContent>
      </Card>
      <ConfirmDialog
        open={confirmCloseAll}
        title={t("connections.closeAllTitle")}
        description={t("connections.closeAllConfirm", { count })}
        confirmLabel={t("connections.closeAll")}
        busy={closingAll}
        onOpenChange={setConfirmCloseAll}
        onConfirm={() => {
          setConfirmCloseAll(false);
          void closeAll();
        }}
      />
    </div>
  );
}
