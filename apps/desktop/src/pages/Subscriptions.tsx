import { useCallback, useEffect, useState } from "react";
import {
  api,
  formatInvokeError,
  type SubscriptionMeta,
} from "../api/tauri";
import { useGenerationGuard } from "../lib/generationGuard";
import {
  isInsecureSubscriptionUrl,
  extractApplyWarning,
  formatApplyWarning,
  extractUpdateResults,
  formatUpdateFailures,
} from "../lib/subscriptions";
import { ErrorAlert, WarnAlert } from "../components/StatusAlert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
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
import { Switch } from "@/components/ui/switch";

export function Subscriptions() {
  const { nextGeneration, isStale } = useGenerationGuard();
  const [items, setItems] = useState<SubscriptionMeta[]>([]);
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [warning, setWarning] = useState<string | null>(null);
  const [updateFailures, setUpdateFailures] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [updating, setUpdating] = useState(false);

  const refresh = useCallback(async () => {
    const gen = nextGeneration();
    try {
      const next = await api.listSubscriptions();
      if (isStale(gen)) return;
      setItems(next);
      setError(null);
    } catch (e) {
      if (!isStale(gen)) setError(formatInvokeError(e));
    }
  }, [isStale, nextGeneration]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function run(action: () => Promise<unknown>, isUpdate = false) {
    nextGeneration();
    setBusy(true);
    if (isUpdate) setUpdating(true);
    setError(null);
    setWarning(null);
    setUpdateFailures(null);
    try {
      const result = await action();
      const applyWarning = extractApplyWarning(result);
      if (applyWarning) {
        setWarning(formatApplyWarning(applyWarning));
      }
      const results = extractUpdateResults(result);
      if (results) {
        const failures = formatUpdateFailures(results);
        if (failures) setUpdateFailures(failures);
      }
      await refresh();
    } catch (e) {
      setError(formatInvokeError(e));
      await refresh();
    } finally {
      setUpdating(false);
      setBusy(false);
    }
  }

  const httpWarn = isInsecureSubscriptionUrl(url);

  function subscriptionSummary(s: SubscriptionMeta): string {
    const parts = [
      s.format,
      `${s.node_count} 节点`,
      `${s.group_count ?? 0} 策略组`,
      `${s.rule_count ?? 0} 规则`,
    ];
    if (s.has_dns) parts.push("DNS");
    if (s.last_updated) {
      parts.push(new Date(s.last_updated).toLocaleString());
    }
    return parts.join(" · ");
  }

  return (
    <div className="subs-panel flex min-h-0 flex-1 flex-col gap-3">
      {error && <ErrorAlert className="shrink-0">{error}</ErrorAlert>}
      {warning && <WarnAlert className="shrink-0">{warning}</WarnAlert>}
      {updateFailures && (
        <ErrorAlert className="shrink-0">
          部分订阅更新失败：{updateFailures}
        </ErrorAlert>
      )}

      <Card size="sm" className="shrink-0">
        <CardHeader>
          <CardTitle>导入</CardTitle>
          <CardDescription>粘贴订阅 URL，可选填写名称</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <form
            className="flex flex-col gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              const u = url.trim();
              if (!u) return;
              void run(async () => {
                await api.addSubscription(u, name.trim() || undefined);
                setUrl("");
                setName("");
              });
            }}
          >
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="sub-url">订阅 URL</FieldLabel>
                <Input
                  id="sub-url"
                  type="url"
                  placeholder="订阅 URL（https 优先）"
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  disabled={busy}
                  required
                />
              </Field>
              <div className="flex flex-wrap items-end gap-2">
                <Field className="min-w-48 flex-1">
                  <FieldLabel htmlFor="sub-name">名称</FieldLabel>
                  <Input
                    id="sub-name"
                    type="text"
                    placeholder="名称（可选）"
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    disabled={busy}
                  />
                </Field>
                <Button
                  type="submit"
                  size="sm"
                  disabled={busy || !url.trim()}
                >
                  导入
                </Button>
              </div>
            </FieldGroup>
          </form>
          {httpWarn ? (
            <WarnAlert>
              当前为 http://，传输未加密，建议改用 https。
            </WarnAlert>
          ) : null}
        </CardContent>
      </Card>

      <Card size="sm" className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <CardHeader className="shrink-0">
          <CardTitle>订阅</CardTitle>
          <CardDescription>
            {items.length === 0
              ? "尚未导入订阅"
              : `${items.length} 条`}
          </CardDescription>
          <CardAction className="flex flex-wrap items-center justify-end gap-2">
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy || items.length === 0}
              onClick={() =>
                void run(() => api.updateAllSubscriptions(), true)
              }
            >
              {updating ? "更新中" : "全部更新"}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy || items.length === 0}
              onClick={() => void run(() => api.applySubscriptions())}
            >
              应用配置
            </Button>
          </CardAction>
        </CardHeader>
        <CardContent className="flex min-h-0 flex-1 flex-col overflow-auto">
          {items.length === 0 ? (
            <div className="my-auto flex flex-col items-start gap-1">
              <CardTitle>暂无订阅</CardTitle>
              <CardDescription>
                打开软件会自动启动内核；需要时在主页用大按钮接管系统代理，也可导入订阅 URL。
              </CardDescription>
            </div>
          ) : (
            <ItemGroup aria-label="订阅列表" className="gap-0">
              {items.map((s, index) => {
                const warnings = s.parse_warnings ?? [];
                return (
                  <div key={s.id}>
                    {index > 0 ? <ItemSeparator className="my-0" /> : null}
                    <Item
                      size="sm"
                      variant={s.active ? "muted" : "default"}
                      className="px-0"
                    >
                      <ItemContent className="min-w-0">
                        <ItemTitle title={s.name}>
                          <span className="truncate">{s.name}</span>
                          {s.active ? <Badge>已激活</Badge> : null}
                        </ItemTitle>
                        <ItemDescription>
                          {subscriptionSummary(s)}
                        </ItemDescription>
                        {s.last_error ? (
                          <ItemDescription className="text-destructive">
                            {s.last_error}
                          </ItemDescription>
                        ) : null}
                        {warnings.length > 0 ? (
                          <ItemDescription className="text-warn">
                            {warnings.join("；")}
                          </ItemDescription>
                        ) : null}
                      </ItemContent>
                      <ItemActions className="flex-wrap">
                        <Field orientation="horizontal" className="w-auto gap-1.5">
                          <Switch
                            id={`sub-active-${s.id}`}
                            size="sm"
                            checked={!!s.active}
                            disabled={busy}
                            aria-label="激活"
                            onCheckedChange={(checked) =>
                              void run(() =>
                                api.setSubscriptionActive(s.id, checked),
                              )
                            }
                          />
                          <FieldLabel
                            htmlFor={`sub-active-${s.id}`}
                            className="text-muted-foreground"
                          >
                            激活
                          </FieldLabel>
                        </Field>
                        <Button
                          type="button"
                          size="sm"
                          variant="outline"
                          disabled={busy}
                          onClick={() =>
                            void run(() => api.updateSubscription(s.id), true)
                          }
                        >
                          {updating ? "更新中" : "更新"}
                        </Button>
                        <Button
                          type="button"
                          size="sm"
                          variant="destructive"
                          disabled={busy}
                          onClick={() => {
                            if (!window.confirm(`删除订阅「${s.name}」？`)) {
                              return;
                            }
                            void run(() => api.removeSubscription(s.id));
                          }}
                        >
                          删除
                        </Button>
                      </ItemActions>
                    </Item>
                  </div>
                );
              })}
            </ItemGroup>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
