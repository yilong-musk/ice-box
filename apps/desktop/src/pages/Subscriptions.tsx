import { useCallback, useEffect, useState } from "react";
import {
  api,
  formatInvokeError,
  type SubscriptionMeta,
} from "../api/tauri";
import { useGenerationGuard } from "../lib/generationGuard";
import { isInsecureSubscriptionUrl, extractApplyWarning, formatApplyWarning, extractUpdateResults, formatUpdateFailures } from "../lib/subscriptions";
import { ErrorAlert, WarnAlert } from "../components/StatusAlert";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";

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

  return (
    <div className="flex flex-col gap-4">
      {error && <ErrorAlert>{error}</ErrorAlert>}
      {warning && <WarnAlert>{warning}</WarnAlert>}
      {updateFailures && (
        <ErrorAlert>部分订阅更新失败：{updateFailures}</ErrorAlert>
      )}

      <Card size="sm">
        <CardContent>
          <form
            className="grid grid-cols-[1fr_auto] gap-2"
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
            <Input
              type="url"
              className="col-span-2"
              placeholder="订阅 URL（https 优先）"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              disabled={busy}
              required
            />
            <Input
              type="text"
              placeholder="名称（可选）"
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={busy}
            />
            <div className="flex flex-wrap gap-2">
              <Button type="submit" size="sm" disabled={busy || !url.trim()}>
                导入
              </Button>
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={busy || items.length === 0}
                onClick={() => void run(() => api.updateAllSubscriptions(), true)}
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
            </div>
          </form>
          {httpWarn && (
            <p className="warn mt-2 text-sm">当前为 http://，传输未加密，建议改用 https。</p>
          )}
        </CardContent>
      </Card>

      {items.length === 0 ? (
        <p className="muted text-sm">
          暂无订阅。打开软件会自动启动内核；需要时在主页用大按钮接管系统代理，也可导入订阅 URL。
        </p>
      ) : (
        <ul className="sub-list">
          {items.map((s) => (
            <li
              key={s.id}
              className={s.active ? "sub-item active" : "sub-item"}
            >
              <div className="sub-main">
                <strong>{s.name}</strong>
                <span className="muted text-sm">
                  {s.format} · {s.node_count} 节点 ·{" "}
                  {(s.group_count ?? 0)} 策略组 · {(s.rule_count ?? 0)} 规则
                  {s.has_dns ? " · DNS" : ""}
                  {s.last_updated
                    ? ` · ${new Date(s.last_updated).toLocaleString()}`
                    : ""}
                </span>
                {s.last_error && <span className="error text-sm">{s.last_error}</span>}
                {(s.parse_warnings ?? []).length > 0 && (
                  <span className="warn text-sm">
                    {s.parse_warnings.join("；")}
                  </span>
                )}
              </div>
              <div className="sub-actions">
                <label className="inline-flex items-center gap-1.5 text-sm">
                  <input
                    type="checkbox"
                    checked={!!s.active}
                    disabled={busy}
                    onChange={(e) =>
                      void run(() =>
                        api.setSubscriptionActive(s.id, e.target.checked),
                      )
                    }
                  />
                  激活
                </label>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={busy}
                  onClick={() => void run(() => api.updateSubscription(s.id), true)}
                >
                  {updating ? "更新中" : "更新"}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="destructive"
                  disabled={busy}
                  onClick={() => {
                    if (!window.confirm(`删除订阅「${s.name}」？`)) return;
                    void run(() => api.removeSubscription(s.id));
                  }}
                >
                  删除
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
