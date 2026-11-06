// SPDX-License-Identifier: GPL-3.0-or-later

import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  formatInvokeError,
  type ListRulesRequest,
  type RuleOverview,
  type RuleRow,
  type RuleSetStatus,
} from "../api/client";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { EmptyState } from "../components/EmptyState";
import { RuleFormDialog } from "../components/RuleFormDialog";
import { RuleJsonDialog } from "../components/RuleJsonDialog";
import { RuleSetStatusDialog } from "../components/RuleSetStatusDialog";
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
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";
import { isPhoneShell } from "@platform/shell";
import { t, useLanguagePreference } from "../lib/i18n";
import { useGenerationGuard } from "../lib/generationGuard";
import {
  customRuleEditable,
  pageCount,
  ruleMatchSummary,
  ruleOutbound,
  ruleTypeLabel,
} from "../lib/rules";

const MAX_KEYWORD_DEBOUNCE_MS = 300;
/** Distance from the list bottom within which the pager stays visible. */
const PAGER_BOTTOM_THRESHOLD_PX = 32;

type StatusFilter = "all" | "disabled" | "enabled";

/** Quick-filter values beside the rule types; rule types never contain ":". */
const QUICK_FILTER_CUSTOM = ":custom";
const QUICK_FILTER_DISABLED = ":disabled";

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
  const phone = isPhoneShell();
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
  const [customForm, setCustomForm] = useState<
    { mode: "add" } | { mode: "edit"; row: RuleRow } | null
  >(null);
  const [jsonForm, setJsonForm] = useState<RuleRow | null>(null);
  const [pendingDelete, setPendingDelete] = useState<RuleRow | null>(null);
  const [ruleSets, setRuleSets] = useState<RuleSetStatus[]>([]);
  const [ruleSetsOpen, setRuleSetsOpen] = useState(false);
  const [nearBottom, setNearBottom] = useState(true);
  const listRef = useRef<HTMLDivElement>(null);
  const mountedRef = useRef(true);
  const overviewRequestRef = useRef<Promise<RuleOverview> | null>(null);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    if (filters.keyword === debouncedKeyword) return;
    const id = window.setTimeout(
      () => {
        setDebouncedKeyword(filters.keyword);
        setOffset(0);
      },
      MAX_KEYWORD_DEBOUNCE_MS,
    );
    return () => window.clearTimeout(id);
  }, [filters.keyword, debouncedKeyword]);

  const load = useCallback(async (refreshOverview = false) => {
    const gen = nextGeneration();
    let overviewRequest: Promise<RuleOverview> | null = null;
    try {
      if (refreshOverview || !overviewRequestRef.current) {
        overviewRequestRef.current = api.getRuleOverview();
      }
      overviewRequest = overviewRequestRef.current;
      const req: ListRulesRequest = {
        keyword: debouncedKeyword || null,
        type: filters.type || null,
        disabled: filters.status,
        custom: filters.custom ? true : null,
        offset,
        limit,
      };
      const [ov, list] = await Promise.all([
        overviewRequest,
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
      if (overviewRequestRef.current === overviewRequest) overviewRequestRef.current = null;
      if (!isStale(gen)) setError(formatInvokeError(e));
    }
  }, [debouncedKeyword, filters.type, filters.status, filters.custom, offset, limit, isStale, nextGeneration]);

  useEffect(() => {
    overviewRequestRef.current = null;
    if (!active) {
      nextGeneration();
      setRows([]);
      setOverview(EMPTY_OVERVIEW);
      setTotal(0);
      if (typeof api.releaseRuleSearch === "function") void api.releaseRuleSearch();
    }
  }, [active, nextGeneration]);

  useEffect(() => {
    if (!active) return;
    void load();
  }, [active, load]);

  // Rule-set status follows the overview: it is re-read whenever the
  // overview is (page activation, refresh, a background update).
  useEffect(() => {
    if (!active || typeof api.getRuleSetStatus !== "function") {
      setRuleSets([]);
      return;
    }
    let cancelled = false;
    api
      .getRuleSetStatus()
      .then((statuses) => {
        if (!cancelled && mountedRef.current) setRuleSets(statuses);
      })
      .catch(() => {
        // The status is informational; the rule list reports real errors.
      });
    return () => {
      cancelled = true;
    };
  }, [active, overview]);

  // Keep the latest `load` and `busy` reachable from the long-lived listener
  // below, so filter changes do not re-register it.
  const loadRef = useRef(load);
  const busyRef = useRef(false);
  useEffect(() => {
    loadRef.current = load;
  }, [load]);
  useEffect(() => {
    busyRef.current = busy;
  }, [busy]);

  // A background subscription auto-update (or a tray action) changes the rule
  // set while this page stays on screen. The overview is memoized between
  // loads, so without this its counts and the type chips would keep the
  // pre-update numbers until the user leaves the page or edits a rule. The
  // announcement carries no detail, so re-read the overview and the current
  // page together. A local mutation in flight reloads on its own, and an
  // inactive pane reloads when it is reactivated.
  useEffect(() => {
    if (!active || typeof api.listenStateChanged !== "function") return;
    let cancelled = false;
    let unlisten = () => {};
    void api
      .listenStateChanged(() => {
        if (busyRef.current) return;
        void loadRef.current(true);
      })
      .then((off) => {
        if (cancelled) off();
        else unlisten = off;
      });
    return () => {
      cancelled = true;
      unlisten();
    };
  }, [active]);

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
    if (next.keyword === undefined) setOffset(0);
  }

  async function reloadAfterMutation() {
    if (mountedRef.current) await load(true);
  }

  async function onToggleDisabled(row: RuleRow) {
    nextGeneration();
    setBusy(true);
    setApplyWarning(null);
    try {
      const r = await api.setRuleDisabled(row.fingerprint, !row.disabled);
      if (mountedRef.current && r.apply_warning) {
        setApplyWarning(
          `${formatInvokeError(r.apply_warning)}`,
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
          `${formatInvokeError(r.apply_warning)}`,
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
          `${formatInvokeError(r.apply_warning)}`,
        );
      }
      await reloadAfterMutation();
    } catch (e) {
      if (mountedRef.current) throw new Error(formatInvokeError(e));
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }

  async function onUpdateCustomRule(
    fingerprint: string,
    rule: Record<string, unknown>,
  ) {
    nextGeneration();
    setBusy(true);
    setApplyWarning(null);
    try {
      const r = await api.updateCustomRule(fingerprint, rule);
      if (mountedRef.current && r.apply_warning) {
        setApplyWarning(
          `${formatInvokeError(r.apply_warning)}`,
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

  const addCustomButton = (
    <Button
      type="button"
      size="sm"
      variant="outline"
      className="shrink-0"
      onClick={() => setCustomForm({ mode: "add" })}
    >
      {t("rules.addCustom")}
    </Button>
  );

  return (
    <div className="rules-panel flex min-h-0 flex-1 flex-col gap-3" data-testid="rules-panel">
      {error && <ErrorAlert className="shrink-0">{error}</ErrorAlert>}
      {applyWarning && (
        <WarnAlert className="shrink-0" role="alert">
          {t("rules.savedButApplyFailed", { detail: applyWarning })}
        </WarnAlert>
      )}

      <Card size="sm" className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <CardHeader className="shrink-0">
          {/* On the phone the toolbar is only search + refresh; the add
              button joins the end of the filter chips below, so it fills the
              gap after the last chip instead of taking a row of its own. */}
          <CardAction
            className={cn(
              "col-span-2 col-start-1 flex w-full items-center justify-end gap-2",
              phone ? "flex-nowrap" : "flex-wrap",
            )}
            data-testid="rules-toolbar"
          >
            <Input
              type="search"
              className={phone ? "min-w-0 flex-1" : "min-w-48 flex-1"}
              placeholder={t("rules.searchPlaceholder")}
              aria-label={t("rules.searchAria")}
              value={filters.keyword}
              onChange={(e) => changeFilters({ keyword: e.target.value })}
            />
            <Button
              type="button"
              size="sm"
              variant="outline"
              className={phone ? "shrink-0" : undefined}
              onClick={() => void load(true)}
              disabled={busy}
            >
              {t("common.refresh")}
            </Button>
            {ruleSets.length > 0 ? (
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="shrink-0"
                data-testid="rule-sets-button"
                onClick={() => setRuleSetsOpen(true)}
              >
                {(() => {
                  const pending = ruleSets.filter((s) => s.state === "pending").length;
                  return pending > 0
                    ? t("rules.ruleSetsPending", { count: ruleSets.length, pending })
                    : t("rules.ruleSets", { count: ruleSets.length });
                })()}
              </Button>
            ) : null}
            {phone ? null : addCustomButton}
          </CardAction>
        </CardHeader>
        <CardContent className="relative flex min-h-0 flex-1 flex-col gap-3 overflow-hidden">
          <div
            className="flex h-auto w-full min-w-0 shrink-0 flex-wrap items-start gap-2"
            data-testid="rules-filters"
          >
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              spacing={2}
              // Quick filters are exclusive: the server ANDs type, custom and
              // disabled, so combining a type chip with Custom or Disabled
              // could match nothing and leave the list empty.
              value={
                filters.custom
                  ? QUICK_FILTER_CUSTOM
                  : filters.status === "disabled"
                    ? QUICK_FILTER_DISABLED
                    : filters.type || "all"
              }
              onValueChange={(value) => {
                changeFilters({
                  type:
                    !value ||
                    value === "all" ||
                    value === QUICK_FILTER_CUSTOM ||
                    value === QUICK_FILTER_DISABLED
                      ? ""
                      : value,
                  custom: value === QUICK_FILTER_CUSTOM,
                  status: value === QUICK_FILTER_DISABLED ? "disabled" : "all",
                });
              }}
              // On the phone the group box is dissolved (display: contents)
              // so its chips and the add button wrap as one flow.
              className={
                phone
                  ? "contents"
                  : "h-auto min-w-0 shrink-0 flex-wrap items-start justify-start"
              }
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
              <ToggleGroupItem value={QUICK_FILTER_CUSTOM}>
                {t("rules.customCount", { count: overview.custom })}
              </ToggleGroupItem>
              <ToggleGroupItem value={QUICK_FILTER_DISABLED}>
                {t("rules.disabledCount", { count: overview.disabled })}
              </ToggleGroupItem>
            </ToggleGroup>
            {phone ? addCustomButton : null}
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
                  const outbound = ruleOutbound(row);
                  const outboundLabel = outbound || t("common.dash");
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
                            className="w-full min-w-0"
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
                          <ItemDescription data-testid="rule-description">
                            {/*
                              ItemDescription is a line-clamped -webkit-box,
                              so the flex row lives in an inner span rather
                              than fighting it for `display`.
                            */}
                            <span className="flex min-w-0 items-center gap-1">
                              <span className="shrink-0">
                                {ruleTypeLabel(row.rule_type)}
                              </span>
                              {/*
                                The phone row is too narrow for an outbound
                                badge beside the actions: a long node name would
                                squeeze the match itself to a few letters. Show
                                it under the match instead, truncated on its
                                own. Action-only rules have no outbound, so
                                there is no arrow to draw.
                              */}
                              {phone && outbound ? (
                                <>
                                  <span className="shrink-0" aria-hidden="true">
                                    →
                                  </span>
                                  <span
                                    className="truncate"
                                    title={outbound}
                                    data-testid="rule-outbound"
                                  >
                                    {outbound}
                                  </span>
                                </>
                              ) : null}
                            </span>
                          </ItemDescription>
                        </ItemContent>
                        <ItemActions className="flex-wrap">
                          {phone ? null : (
                            <Badge variant="outline" title={outboundLabel}>
                              {outboundLabel}
                            </Badge>
                          )}
                          {row.custom ? (
                            <Button
                              type="button"
                              size="sm"
                              variant="outline"
                              disabled={busy}
                              onClick={() =>
                                customRuleEditable(row.rule)
                                  ? setCustomForm({ mode: "edit", row })
                                  : setJsonForm(row)
                              }
                            >
                              {t("common.edit")}
                            </Button>
                          ) : (
                            <Button
                              type="button"
                              size="sm"
                              variant="outline"
                              disabled={busy}
                              onClick={() => void onToggleDisabled(row)}
                            >
                              {row.disabled
                                ? t("common.enable")
                                : t("common.disable")}
                            </Button>
                          )}
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
              data-testid="rules-pager"
              data-visible={nearBottom ? "true" : "false"}
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
        open={customForm !== null}
        onOpenChange={(open) => {
          if (!open) setCustomForm(null);
        }}
        busy={busy}
        editing={customForm?.mode === "edit" ? customForm.row : null}
        onAdd={(rule) => onAddCustomRule(rule)}
        onUpdate={(fingerprint, rule) =>
          onUpdateCustomRule(fingerprint, rule)
        }
      />
      <RuleJsonDialog
        open={jsonForm !== null}
        editing={jsonForm}
        busy={busy}
        onOpenChange={(open) => {
          if (!open) setJsonForm(null);
        }}
        onSave={(fingerprint, rule) =>
          onUpdateCustomRule(fingerprint, rule)
        }
      />
      <RuleSetStatusDialog
        open={ruleSetsOpen}
        onOpenChange={setRuleSetsOpen}
        statuses={ruleSets}
        onStatusesChange={setRuleSets}
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
