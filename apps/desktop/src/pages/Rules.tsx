import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  formatInvokeError,
  type ListRulesRequest,
  type RuleOverview,
  type RuleRow,
} from "../api/tauri";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { EmptyState } from "../components/EmptyState";
import { RuleFormDialog } from "../components/RuleFormDialog";
import { ErrorAlert, WarnAlert } from "../components/StatusAlert";
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
import { Toggle } from "@/components/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";
import { t, useLanguagePreference } from "../lib/i18n";
import { useGenerationGuard } from "../lib/generationGuard";
import {
  pageCount,
  ruleMatchSummary,
  ruleOutbound,
  ruleTypeLabel,
} from "../lib/rules";

const MAX_KEYWORD_DEBOUNCE_MS = 300;
/** Distance from the list bottom within which the pager stays visible. */
const PAGER_BOTTOM_THRESHOLD_PX = 32;

type StatusFilter = "all" | "disabled" | "enabled";

type Filters = {
  keyword: string;
  type: string;
  status: StatusFilter;
  custom: boolean;
};

type Props = {
  onNavigate?: (tab: "subs") => void;
  /** When false the page stays mounted but refreshes again on reactivation. */
  active?: boolean;
};

const EMPTY_OVERVIEW: RuleOverview = {
  total: 0,
  disabled: 0,
  custom: 0,
  rule_sets: 0,
  types: [],
};

export function Rules({ onNavigate, active = true }: Props) {
  useLanguagePreference();
  const { nextGeneration, isStale } = useGenerationGuard();
  const [overview, setOverview] = useState<RuleOverview>(EMPTY_OVERVIEW);
  const [rows, setRows] = useState<RuleRow[]>([]);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);
  const [limit] = useState(100);
  const [filters, setFilters] = useState<Filters>({
    keyword: "",
    type: "",
    status: "all",
    custom: false,
  });
  const [debouncedKeyword, setDebouncedKeyword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [applyWarning, setApplyWarning] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showCustomForm, setShowCustomForm] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<RuleRow | null>(null);
  const [nearBottom, setNearBottom] = useState(true);
  const listRef = useRef<HTMLDivElement>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    const id = window.setTimeout(
      () => setDebouncedKeyword(filters.keyword),
      MAX_KEYWORD_DEBOUNCE_MS,
    );
    return () => window.clearTimeout(id);
  }, [filters.keyword]);

  const load = useCallback(async () => {
    const gen = nextGeneration();
    try {
      const req: ListRulesRequest = {
        keyword: debouncedKeyword || null,
        type: filters.type || null,
        disabled: filters.status,
        custom: filters.custom ? true : null,
        offset,
        limit,
      };
      const [ov, list] = await Promise.all([
        api.getRuleOverview(),
        api.listRules(req),
      ]);
      if (isStale(gen)) return;
      setOverview(ov);
      setRows(list.items);
      setTotal(list.total);
      if (list.offset >= list.total && list.total > 0 && list.offset > 0) {
        setOffset(Math.max(0, list.total - list.limit));
        return;
      }
      setError(null);
    } catch (e) {
      if (!isStale(gen)) setError(formatInvokeError(e));
    }
  }, [debouncedKeyword, filters, offset, limit, isStale, nextGeneration]);

  useEffect(() => {
    if (!active) return;
    void load();
  }, [active, load]);

  // Recompute pager visibility whenever the list content changes.
  useEffect(() => {
    const el = listRef.current;
    if (!el || rows.length === 0) return;
    setNearBottom(
      el.scrollHeight - el.scrollTop - el.clientHeight <=
        PAGER_BOTTOM_THRESHOLD_PX,
    );
  }, [rows, total, offset, limit]);

  function onListScroll(e: React.UIEvent<HTMLDivElement>) {
    const el = e.currentTarget;
    setNearBottom(
      el.scrollHeight - el.scrollTop - el.clientHeight <=
        PAGER_BOTTOM_THRESHOLD_PX,
    );
  }

  function changeFilters(next: Partial<Filters>) {
    setFilters((f) => ({ ...f, ...next }));
    setOffset(0);
  }

  async function reloadAfterMutation() {
    if (mountedRef.current) await load();
  }

  async function onToggleDisabled(row: RuleRow) {
    nextGeneration();
    setBusy(true);
    setApplyWarning(null);
    try {
      const r = await api.setRuleDisabled(row.fingerprint, !row.disabled);
      if (mountedRef.current && r.apply_warning) {
        setApplyWarning(
          `${r.apply_warning.code}: ${r.apply_warning.message}`,
        );
      }
      await reloadAfterMutation();
    } catch (e) {
      if (mountedRef.current) setError(formatInvokeError(e));
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }

  async function onRemoveCustom(row: RuleRow) {
    nextGeneration();
    setBusy(true);
    setApplyWarning(null);
    try {
      const r = await api.removeCustomRule(row.fingerprint);
      if (mountedRef.current && r.apply_warning) {
        setApplyWarning(
          `${r.apply_warning.code}: ${r.apply_warning.message}`,
        );
      }
      await reloadAfterMutation();
    } catch (e) {
      if (mountedRef.current) setError(formatInvokeError(e));
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }

  async function onAddCustomRule(rule: Record<string, unknown>) {
    nextGeneration();
    setBusy(true);
    setApplyWarning(null);
    try {
      const r = await api.addCustomRule(rule);
      if (mountedRef.current && r.apply_warning) {
        setApplyWarning(
          `${r.apply_warning.code}: ${r.apply_warning.message}`,
        );
      }
      await reloadAfterMutation();
    } catch (e) {
      if (mountedRef.current) throw new Error(formatInvokeError(e));
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }

  const pages = pageCount(total, limit);
  const page = Math.floor(offset / limit) + 1;
  const showList = overview.total > 0 || overview.custom > 0 || rows.length > 0;

  return (
    <div className="rules-panel flex min-h-0 flex-1 flex-col gap-3">
      {error && <ErrorAlert className="shrink-0">{error}</ErrorAlert>}
      {applyWarning && (
        <WarnAlert className="shrink-0" role="alert">
          {t("rules.savedButApplyFailed", { detail: applyWarning })}
        </WarnAlert>
      )}

      <Card size="sm" className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <CardHeader className="shrink-0">
          <CardAction className="col-span-2 col-start-1 flex w-full flex-wrap items-center justify-end gap-2">
            <Input
              type="search"
              className="min-w-48 flex-1"
              placeholder={t("rules.searchPlaceholder")}
              aria-label={t("rules.searchAria")}
              value={filters.keyword}
              onChange={(e) => changeFilters({ keyword: e.target.value })}
            />
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() => void load()}
              disabled={busy}
            >
              {t("common.refresh")}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() => setShowCustomForm(true)}
            >
              {t("rules.addCustom")}
            </Button>
          </CardAction>
        </CardHeader>
        <CardContent className="relative flex min-h-0 flex-1 flex-col gap-3 overflow-hidden">
          <div className="flex h-auto w-full min-w-0 shrink-0 flex-wrap items-start gap-2">
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              spacing={2}
              value={filters.type || "all"}
              onValueChange={(value) => {
                changeFilters({
                  type: !value || value === "all" ? "" : value,
                });
              }}
              className="h-auto min-w-0 shrink-0 flex-wrap items-start justify-start"
              aria-label={t("rules.filtersAria")}
            >
              {overview.types.length > 0 ? (
                <ToggleGroupItem value="all">{t("rules.typeAll")}</ToggleGroupItem>
              ) : null}
              {overview.types.map((t) => (
                <ToggleGroupItem key={t.rule_type} value={t.rule_type}>
                  {ruleTypeLabel(t.rule_type)} {t.count}
                </ToggleGroupItem>
              ))}
              <Toggle
                variant="outline"
                size="sm"
                pressed={filters.custom}
                onPressedChange={(pressed) =>
                  changeFilters({ custom: pressed })
                }
              >
                {t("rules.customCount", { count: overview.custom })}
              </Toggle>
              <Toggle
                variant="outline"
                size="sm"
                pressed={filters.status === "disabled"}
                onPressedChange={(pressed) =>
                  changeFilters({
                    status: pressed ? "disabled" : "all",
                  })
                }
              >
                {t("rules.disabledCount", { count: overview.disabled })}
              </Toggle>
            </ToggleGroup>
          </div>

          {!showList ? (
            <EmptyState
              framed={false}
              className="my-auto"
              title={t("rules.emptyTitle")}
              description={t("rules.emptyDesc")}
              actionLabel={t("home.goToSubs")}
              onAction={() => onNavigate?.("subs")}
            />
          ) : (
            <ScrollArea
              type="scroll"
              scrollHideDelay={600}
              className="min-h-0 flex-1 overflow-hidden"
              viewportRef={listRef}
              onViewportScroll={onListScroll}
            >
              <ItemGroup aria-label={t("rules.listAria")} className="gap-0 pb-14">
                {rows.map((row, index) => {
                  const summary = ruleMatchSummary(row);
                  const outboundLabel = ruleOutbound(row) || "—";
                  return (
                    <div key={row.fingerprint}>
                      {index > 0 ? <ItemSeparator className="my-0" /> : null}
                      <Item
                        size="sm"
                        variant={row.disabled ? "muted" : "default"}
                        className={cn(
                          "pl-0 pr-3",
                          row.disabled && "opacity-55",
                        )}
                      >
                        <ItemContent className="min-w-0">
                          <ItemTitle
                            title={`${summary}\n${JSON.stringify(row.rule)}`}
                          >
                            <span className="truncate">
                              {summary || JSON.stringify(row.rule)}
                            </span>
                            {row.custom ? (
                              <span className="shrink-0 text-xs font-normal text-muted-foreground">
                                {t("rules.custom")}
                              </span>
                            ) : (
                              <Badge variant="outline" className="font-mono">
                                #{(row.index ?? 0) + 1}
                              </Badge>
                            )}
                          </ItemTitle>
                          <ItemDescription>
                            {ruleTypeLabel(row.rule_type)}
                          </ItemDescription>
                        </ItemContent>
                        <ItemActions className="flex-wrap">
                          <Badge variant="outline" title={outboundLabel}>
                            {outboundLabel}
                          </Badge>
                          <Button
                            type="button"
                            size="sm"
                            variant="outline"
                            disabled={busy}
                            onClick={() => void onToggleDisabled(row)}
                          >
                            {row.disabled ? t("common.enable") : t("common.disable")}
                          </Button>
                          {row.custom ? (
                            <Button
                              type="button"
                              size="sm"
                              variant="destructive"
                              disabled={busy}
                              onClick={() => setPendingDelete(row)}
                            >
                              {t("common.delete")}
                            </Button>
                          ) : null}
                        </ItemActions>
                      </Item>
                    </div>
                  );
                })}
              </ItemGroup>
            </ScrollArea>
          )}
          {showList ? (
            <div
              className={cn(
                "absolute inset-x-0 bottom-0 z-10 flex items-center justify-center gap-3 border-t border-border bg-background/95 px-3 py-2 backdrop-blur-sm transition-opacity duration-200",
                nearBottom
                  ? "opacity-100"
                  : "pointer-events-none opacity-0",
              )}
            >
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={busy || offset === 0}
                onClick={() => setOffset((o) => Math.max(0, o - limit))}
              >
                {t("rules.prevPage")}
              </Button>
              <span className="text-sm text-muted-foreground">
                {t("rules.pageInfo", { page, pages, total })}
              </span>
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={busy || offset + limit >= total}
                onClick={() => setOffset((o) => o + limit)}
              >
                {t("rules.nextPage")}
              </Button>
            </div>
          ) : null}
        </CardContent>
      </Card>
      <RuleFormDialog
        open={showCustomForm}
        onOpenChange={setShowCustomForm}
        busy={busy}
        onAdd={(rule) => onAddCustomRule(rule)}
      />
      <ConfirmDialog
        open={pendingDelete !== null}
        title={t("rules.deleteCustomTitle")}
        description={
          pendingDelete
            ? t("rules.deleteCustomConfirm", {
                summary: ruleMatchSummary(pendingDelete),
              })
            : undefined
        }
        confirmLabel={t("common.delete")}
        busy={busy}
        onOpenChange={(open) => {
          if (!open) setPendingDelete(null);
        }}
        onConfirm={() => {
          if (!pendingDelete) return;
          const row = pendingDelete;
          setPendingDelete(null);
          void onRemoveCustom(row);
        }}
      />
    </div>
  );
}
