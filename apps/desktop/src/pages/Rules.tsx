import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  formatInvokeError,
  type ListRulesRequest,
  type NodeInfo,
  type RuleOverview,
  type RuleRow,
} from "../api/tauri";
import { EmptyState } from "../components/EmptyState";
import { ErrorAlert, WarnAlert } from "../components/StatusAlert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Field,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field";
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
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";
import { useGenerationGuard } from "../lib/generationGuard";
import {
  buildCustomRule,
  pageCount,
  ruleMatchSummary,
  ruleOutbound,
  ruleTypeLabel,
  RULE_MATCHER_DEFS,
  STRATEGY_GROUP_TYPES,
} from "../lib/rules";

const PAGE_SIZES = [50, 100, 200];
const MAX_KEYWORD_DEBOUNCE_MS = 300;

type StatusFilter = "all" | "disabled" | "enabled";

type Filters = {
  keyword: string;
  type: string;
  status: StatusFilter;
};

type Props = {
  onNavigate?: (tab: "subs") => void;
};

const EMPTY_OVERVIEW: RuleOverview = {
  total: 0,
  disabled: 0,
  custom: 0,
  rule_sets: 0,
  types: [],
};

export function Rules({ onNavigate }: Props) {
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
  });
  const [debouncedKeyword, setDebouncedKeyword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [applyWarning, setApplyWarning] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showCustomForm, setShowCustomForm] = useState(false);
  const [matcherKey, setMatcherKey] = useState("domain_suffix");
  const [matcherValue, setMatcherValue] = useState("");
  const [matcherBool, setMatcherBool] = useState(true);
  const [outbound, setOutbound] = useState("direct");
  const [nodeOptions, setNodeOptions] = useState<NodeInfo[]>([]);
  const [customError, setCustomError] = useState<string | null>(null);
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

  // Outbound options for the custom rule editor: current subscription's nodes/groups.
  useEffect(() => {
    if (!showCustomForm) return;
    let cancelled = false;
    api
      .listNodes()
      .then((nodes) => {
        if (!cancelled) setNodeOptions(nodes);
      })
      .catch(() => {
        if (!cancelled) setNodeOptions([]);
      });
    return () => {
      cancelled = true;
    };
  }, [showCustomForm]);

  const load = useCallback(async () => {
    const gen = nextGeneration();
    try {
      const req: ListRulesRequest = {
        keyword: debouncedKeyword || null,
        type: filters.type || null,
        disabled: filters.status,
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
    void load();
  }, [load]);

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
    if (!window.confirm(`删除自定义规则：${ruleMatchSummary(row)}？`)) return;
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

  async function onAddCustomRule() {
    const def = RULE_MATCHER_DEFS.find((d) => d.key === matcherKey);
    const value = def?.kind === "boolean" ? matcherBool : matcherValue;
    const rule = buildCustomRule(matcherKey, value, outbound);
    if (!rule) return;
    nextGeneration();
    setBusy(true);
    setApplyWarning(null);
    setCustomError(null);
    try {
      const r = await api.addCustomRule(rule);
      if (mountedRef.current && r.apply_warning) {
        setApplyWarning(
          `${r.apply_warning.code}: ${r.apply_warning.message}`,
        );
      }
      if (mountedRef.current) {
        setMatcherValue("");
        setMatcherBool(true);
        setShowCustomForm(false);
      }
      await reloadAfterMutation();
    } catch (e) {
      if (mountedRef.current) setCustomError(formatInvokeError(e));
    } finally {
      if (mountedRef.current) setBusy(false);
    }
  }

  const previewDef = RULE_MATCHER_DEFS.find((d) => d.key === matcherKey);
  const previewRule = buildCustomRule(
    matcherKey,
    previewDef?.kind === "boolean" ? matcherBool : matcherValue,
    outbound,
  );

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

      <div
        className="grid shrink-0 grid-cols-4 items-stretch gap-3"
        aria-label="规则统计"
      >
        <Card size="sm" className="min-w-0 data-[size=sm]:[--card-spacing:--spacing(2)]">
          <CardHeader>
            <CardTitle>规则</CardTitle>
          </CardHeader>
          <CardContent>
            <p className="font-heading text-sm font-medium tabular-nums">
              {overview.total}
            </p>
          </CardContent>
        </Card>
        <Card size="sm" className="min-w-0 data-[size=sm]:[--card-spacing:--spacing(2)]">
          <CardHeader>
            <CardTitle>已禁用</CardTitle>
          </CardHeader>
          <CardContent>
            <Button
              type="button"
              size="sm"
              className="w-full"
              variant={filters.status === "disabled" ? "secondary" : "outline"}
              aria-pressed={filters.status === "disabled"}
              onClick={() =>
                changeFilters({
                  status: filters.status === "disabled" ? "all" : "disabled",
                })
              }
            >
              已禁用 {overview.disabled}
            </Button>
          </CardContent>
        </Card>
        <Card size="sm" className="min-w-0 data-[size=sm]:[--card-spacing:--spacing(2)]">
          <CardHeader>
            <CardTitle>自定义</CardTitle>
          </CardHeader>
          <CardContent>
            <p className="font-heading text-sm font-medium tabular-nums">
              {overview.custom}
            </p>
          </CardContent>
        </Card>
        <Card size="sm" className="min-w-0 data-[size=sm]:[--card-spacing:--spacing(2)]">
          <CardHeader>
            <CardTitle>规则集</CardTitle>
          </CardHeader>
          <CardContent>
            <p className="font-heading text-sm font-medium tabular-nums">
              {overview.rule_sets}
            </p>
          </CardContent>
        </Card>
      </div>

      <Card size="sm" className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <CardHeader className="shrink-0">
          <CardTitle>规则</CardTitle>
          <CardDescription>查询、禁用或添加自定义规则</CardDescription>
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
              onClick={() => {
                setShowCustomForm((v) => !v);
                setCustomError(null);
              }}
            >
              {showCustomForm ? "收起" : "+ 自定义规则"}
            </Button>
          </CardAction>
        </CardHeader>
        <CardContent className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden">
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
              className="h-auto w-full min-w-0 shrink-0 flex-wrap items-start justify-start"
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

          {showCustomForm ? (
            <Card size="sm" className="shrink-0 overflow-visible">
              <CardHeader>
                <CardTitle>自定义规则</CardTitle>
                <CardDescription>
                  自定义规则优先于订阅规则生效，出口需为 direct / block
                  或现有节点 / 策略组标签。
                </CardDescription>
              </CardHeader>
              <CardContent className="flex flex-col gap-3">
                <FieldGroup className="grid grid-cols-1 gap-2.5 sm:grid-cols-3 sm:*:min-w-0">
                  <Field>
                    <FieldLabel htmlFor="rule-matcher-type">匹配类型</FieldLabel>
                    <NativeSelect
                      id="rule-matcher-type"
                      aria-label="匹配类型"
                      value={matcherKey}
                      onChange={(e) => {
                        setMatcherKey(e.target.value);
                        setMatcherValue("");
                        setCustomError(null);
                      }}
                    >
                      {RULE_MATCHER_DEFS.map((d) => (
                        <NativeSelectOption key={d.key} value={d.key}>
                          {d.label}
                        </NativeSelectOption>
                      ))}
                    </NativeSelect>
                  </Field>
                  {previewDef?.kind === "boolean" ? (
                    <Field>
                      <FieldLabel htmlFor="rule-matcher-bool">
                        {previewDef.label}
                      </FieldLabel>
                      <Field orientation="horizontal">
                        <Checkbox
                          id="rule-matcher-bool"
                          checked={matcherBool}
                          onCheckedChange={(checked) =>
                            setMatcherBool(checked === true)
                          }
                          aria-label={previewDef.label}
                        />
                        <FieldDescription>
                          匹配{previewDef.label}
                        </FieldDescription>
                      </Field>
                    </Field>
                  ) : (
                    <Field>
                      <FieldLabel htmlFor="rule-match-value">匹配值</FieldLabel>
                      <Input
                        id="rule-match-value"
                        type="text"
                        aria-label="匹配值"
                        placeholder={previewDef?.placeholder}
                        value={matcherValue}
                        onChange={(e) => {
                          setMatcherValue(e.target.value);
                          setCustomError(null);
                        }}
                      />
                    </Field>
                  )}
                  <Field>
                    <FieldLabel htmlFor="rule-outbound">出口</FieldLabel>
                    <NativeSelect
                      id="rule-outbound"
                      aria-label="出口"
                      value={outbound}
                      onChange={(e) => setOutbound(e.target.value)}
                    >
                      <NativeSelectOption value="direct">
                        direct（直连）
                      </NativeSelectOption>
                      <NativeSelectOption value="block">
                        block（拦截）
                      </NativeSelectOption>
                      {nodeOptions.map((n) => (
                        <NativeSelectOption key={n.tag} value={n.tag}>
                          {n.tag}
                          {STRATEGY_GROUP_TYPES.includes(n.outbound_type)
                            ? "（策略组）"
                            : ""}
                        </NativeSelectOption>
                      ))}
                    </NativeSelect>
                  </Field>
                </FieldGroup>
                {customError ? <FieldError>{customError}</FieldError> : null}
                {previewRule ? (
                  <FieldDescription className="break-all">
                    预览：<code>{JSON.stringify(previewRule)}</code>
                  </FieldDescription>
                ) : null}
                <div>
                  <Button
                    type="button"
                    size="sm"
                    disabled={busy || !previewRule}
                    onClick={() => void onAddCustomRule()}
                  >
                    添加
                  </Button>
                </div>
              </CardContent>
            </Card>
          ) : null}

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
              aria-label="规则列表"
              className="min-h-0 flex-1 gap-0 overflow-auto"
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
                            onClick={() => void onRemoveCustom(row)}
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
        </CardContent>
        {showList ? (
          <CardFooter className="shrink-0 justify-center gap-3">
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
          </CardFooter>
        ) : null}
      </Card>
    </div>
  );
}
