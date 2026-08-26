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
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { NativeSelect } from "@/components/ui/native-select";
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
    <div className="flex flex-col gap-4">
      {error && <ErrorAlert>{error}</ErrorAlert>}
      {applyWarning && (
        <WarnAlert role="alert">
          已保存，但应用失败：{applyWarning}
        </WarnAlert>
      )}

      <div className="rule-stats" aria-label="规则统计">
        <span className="rule-stat">
          规则 <strong>{overview.total}</strong>
        </span>
        <Button
          type="button"
          size="xs"
          variant={filters.status === "disabled" ? "secondary" : "outline"}
          onClick={() =>
            changeFilters({
              status: filters.status === "disabled" ? "all" : "disabled",
            })
          }
        >
          已禁用 {overview.disabled}
        </Button>
        <span className="rule-stat">
          自定义 <strong>{overview.custom}</strong>
        </span>
        <span className="rule-stat">
          规则集 <strong>{overview.rule_sets}</strong>
        </span>
      </div>

      {overview.types.length > 0 && (
        <div className="rule-type-chips" aria-label="规则类型筛选">
          <Button
            type="button"
            size="xs"
            variant={filters.type === "" ? "secondary" : "outline"}
            onClick={() => changeFilters({ type: "" })}
          >
            全部
          </Button>
          {overview.types.map((t) => (
            <Button
              key={t.rule_type}
              type="button"
              size="xs"
              variant={filters.type === t.rule_type ? "secondary" : "outline"}
              onClick={() =>
                changeFilters({ type: filters.type === t.rule_type ? "" : t.rule_type })
              }
            >
              {ruleTypeLabel(t.rule_type)} {t.count}
            </Button>
          ))}
        </div>
      )}

      <div className="rule-toolbar">
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
          value={filters.status}
          onChange={(e) =>
            changeFilters({ status: e.target.value as StatusFilter })
          }
        >
          <option value="all">全部状态</option>
          <option value="enabled">仅启用</option>
          <option value="disabled">仅禁用</option>
        </NativeSelect>
        <NativeSelect
          aria-label="每页条数"
          className="w-auto"
          value={limit}
          onChange={(e) => {
            setLimit(Number(e.target.value));
            setOffset(0);
          }}
        >
          {PAGE_SIZES.map((n) => (
            <option key={n} value={n}>
              每页 {n}
            </option>
          ))}
        </NativeSelect>
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
      </div>

      {showCustomForm && (
        <div className="rule-custom-form">
          <div className="rule-custom-fields">
            <label>
              匹配类型
              <NativeSelect
                aria-label="匹配类型"
                value={matcherKey}
                onChange={(e) => {
                  setMatcherKey(e.target.value);
                  setMatcherValue("");
                  setCustomError(null);
                }}
              >
                {RULE_MATCHER_DEFS.map((d) => (
                  <option key={d.key} value={d.key}>
                    {d.label}
                  </option>
                ))}
              </NativeSelect>
            </label>
            {previewDef?.kind === "boolean" ? (
              <label>
                {previewDef.label}
                <span className="rule-custom-bool">
                  <input
                    type="checkbox"
                    aria-label={previewDef.label}
                    checked={matcherBool}
                    onChange={(e) => setMatcherBool(e.target.checked)}
                  />
                  匹配{previewDef.label}
                </span>
              </label>
            ) : (
              <label>
                匹配值
                <Input
                  type="text"
                  aria-label="匹配值"
                  placeholder={previewDef?.placeholder}
                  value={matcherValue}
                  onChange={(e) => {
                    setMatcherValue(e.target.value);
                    setCustomError(null);
                  }}
                />
              </label>
            )}
            <label>
              出口
              <NativeSelect
                aria-label="出口"
                value={outbound}
                onChange={(e) => setOutbound(e.target.value)}
              >
                <option value="direct">direct（直连）</option>
                <option value="block">block（拦截）</option>
                {nodeOptions.map((n) => (
                  <option key={n.tag} value={n.tag}>
                    {n.tag}
                    {STRATEGY_GROUP_TYPES.includes(n.outbound_type)
                      ? "（策略组）"
                      : ""}
                  </option>
                ))}
              </NativeSelect>
            </label>
          </div>
          {customError && <p className="error">{customError}</p>}
          {previewRule && (
            <p className="text-xs">
              <span className="muted">预览：</span>
              <code>{JSON.stringify(previewRule)}</code>
            </p>
          )}
          <div className="flex gap-2">
            <Button
              type="button"
              size="sm"
              disabled={busy || !previewRule}
              onClick={() => void onAddCustomRule()}
            >
              添加
            </Button>
          </div>
          <p className="hint">
            自定义规则优先于订阅规则生效，出口需为 direct / block 或现有节点 / 策略组标签。
          </p>
        </div>
      )}

      {!showList ? (
        <EmptyState
          title="暂无规则"
          description="当前没有订阅规则，也没有自定义规则。导入含规则的订阅后可在此查询、禁用规则或添加自定义规则。"
          actionLabel="前往订阅页导入"
          onAction={() => onNavigate?.("subs")}
        />
      ) : (
        <Card size="sm">
          <CardContent className="space-y-3">
          <ul className="node-table rule-table" aria-label="规则列表">
            <li className="node-table-head rule-table-head">
              <span>#</span>
              <span>类型</span>
              <span>匹配</span>
              <span>出口</span>
              <span>操作</span>
            </li>
            {rows.map((row) => {
              const summary = ruleMatchSummary(row);
              return (
                <li
                  key={row.fingerprint}
                  className={row.disabled ? "node-table-row rule-disabled" : "node-table-row"}
                >
                  <span className="rule-index muted">
                    {row.custom ? (
                      <span className="rule-custom-badge">自定义</span>
                    ) : (
                      `#${(row.index ?? 0) + 1}`
                    )}
                  </span>
                  <span className="rule-type">
                    {ruleTypeLabel(row.rule_type)}
                  </span>
                  <span
                    className="rule-match"
                    title={`${summary}\n${JSON.stringify(row.rule)}`}
                  >
                    {summary || JSON.stringify(row.rule)}
                  </span>
                  <span className="rule-outbound" title={ruleOutbound(row)}>
                    {ruleOutbound(row) || "—"}
                  </span>
                  <span className="node-row-actions">
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      disabled={busy}
                      onClick={() => void onToggleDisabled(row)}
                    >
                      {row.disabled ? "启用" : "禁用"}
                    </Button>
                    {row.custom && (
                      <Button
                        type="button"
                        size="sm"
                        variant="destructive"
                        disabled={busy}
                        onClick={() => void onRemoveCustom(row)}
                      >
                        删除
                      </Button>
                    )}
                  </span>
                </li>
              );
            })}
          </ul>

          <div className="rule-pager">
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy || offset === 0}
              onClick={() => setOffset((o) => Math.max(0, o - limit))}
            >
              上一页
            </Button>
            <span className="muted text-sm">
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
          </CardContent>
        </Card>
      )}
    </div>
  );
}