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
  CardTitle,
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
import {
  NativeSelect,
  NativeSelectOption,
} from "@/components/ui/native-select";
import { Toggle } from "@/components/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";
import { useGenerationGuard } from "../lib/generationGuard";
import {
  pageCount,
  ruleMatchSummary,
  ruleOutbound,
  ruleTypeLabel,
} from "../lib/rules";

const PAGE_SIZES = [50, 100, 200];
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
  const { nextGeneration, isStale } = useGenerationGuard();
  const [overview, setOverview] = useState<RuleOverview>(EMPTY_OVERVIEW);
  const [rows, setRows] = useState<RuleRow[]>([]);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);
  const [limit, setLimit] = useState(50);
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
          已保存，但应用失败：{applyWarning}
        </WarnAlert>
      )}

      <Card size="sm" className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <CardHeader className="shrink-0">
          <CardTitle>规则</CardTitle>
          <CardAction className="flex flex-wrap items-center justify-end gap-2">
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() => void load()}
              disabled={busy}
            >
              刷新
            </Button>
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() => setShowCustomForm(true)}
            >
              + 自定义规则
            </Button>
          </CardAction>
        </CardHeader>
        <CardContent className="relative flex min-h-0 flex-1 flex-col gap-3 overflow-hidden">
          <div className="flex shrink-0 flex-wrap items-center gap-2">
            <Input
              type="search"
              className="min-w-48 flex-1"
              placeholder="搜索域名 / 出口 / 规则集…"
              aria-label="搜索规则"
              value={filters.keyword}
              onChange={(e) => changeFilters({ keyword: e.target.value })}
            />
            <NativeSelect
              aria-label="禁用状态筛选"
              className="w-auto"
              size="sm"
              value={filters.status}
              onChange={(e) =>
                changeFilters({ status: e.target.value as StatusFilter })
              }
            >
              <NativeSelectOption value="all">全部状态</NativeSelectOption>
              <NativeSelectOption value="enabled">仅启用</NativeSelectOption>
              <NativeSelectOption value="disabled">仅禁用</NativeSelectOption>
            </NativeSelect>
            <NativeSelect
              aria-label="每页条数"
              className="w-auto"
              size="sm"
              value={limit}
              onChange={(e) => {
                setLimit(Number(e.target.value));
                setOffset(0);
              }}
            >
              {PAGE_SIZES.map((n) => (
                <NativeSelectOption key={n} value={n}>
                  每页 {n}
                </NativeSelectOption>
              ))}
            </NativeSelect>
          </div>

          <div
            className="flex h-auto w-full min-w-0 shrink-0 flex-wrap items-start gap-2"
            aria-label="规则筛选"
          >
            {overview.types.length > 0 ? (
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
                aria-label="规则类型筛选"
              >
                <ToggleGroupItem value="all">全部</ToggleGroupItem>
                {overview.types.map((t) => (
                  <ToggleGroupItem key={t.rule_type} value={t.rule_type}>
                    {ruleTypeLabel(t.rule_type)} {t.count}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            ) : null}
            <Toggle
              variant="outline"
              size="sm"
              pressed={filters.custom}
              onPressedChange={(pressed) =>
                changeFilters({ custom: pressed })
              }
            >
              自定义 {overview.custom}
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
              已禁用 {overview.disabled}
            </Toggle>
          </div>

          {!showList ? (
            <EmptyState
              framed={false}
              className="my-auto"
              title="暂无规则"
              description="当前没有订阅规则，也没有自定义规则。导入含规则的订阅后可在此查询、禁用规则或添加自定义规则。"
              actionLabel="前往订阅页导入"
              onAction={() => onNavigate?.("subs")}
            />
          ) : (
            <ItemGroup
              ref={listRef}
              aria-label="规则列表"
              className="min-h-0 flex-1 gap-0 overflow-auto pb-14"
              onScroll={onListScroll}
            >
              {rows.map((row, index) => {
                const summary = ruleMatchSummary(row);
                const outboundLabel = ruleOutbound(row) || "—";
                return (
                  <div key={row.fingerprint}>
                    {index > 0 ? <ItemSeparator className="my-0" /> : null}
                    <Item
                      size="sm"
                      variant={row.disabled ? "muted" : "default"}
                      className={cn("px-0", row.disabled && "opacity-55")}
                    >
                      <ItemContent className="min-w-0">
                        <ItemTitle
                          title={`${summary}\n${JSON.stringify(row.rule)}`}
                        >
                          <span className="truncate">
                            {summary || JSON.stringify(row.rule)}
                          </span>
                          {row.custom ? (
                            <Badge>自定义</Badge>
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
                          {row.disabled ? "启用" : "禁用"}
                        </Button>
                        {row.custom ? (
                          <Button
                            type="button"
                            size="sm"
                            variant="destructive"
                            disabled={busy}
                            onClick={() => setPendingDelete(row)}
                          >
                            删除
                          </Button>
                        ) : null}
                      </ItemActions>
                    </Item>
                  </div>
                );
              })}
            </ItemGroup>
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
                上一页
              </Button>
              <span className="text-sm text-muted-foreground">
                第 {page} / {pages} 页 · 共 {total} 条
              </span>
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={busy || offset + limit >= total}
                onClick={() => setOffset((o) => o + limit)}
              >
                下一页
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
        title="删除自定义规则"
        description={
          pendingDelete
            ? `确认删除规则：${ruleMatchSummary(pendingDelete)}？`
            : undefined
        }
        confirmLabel="删除"
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
