import { useCallback, useEffect, useState } from "react";
import {
  api,
  formatInvokeError,
  type SubscriptionMeta,
} from "../api/tauri";
import { useGenerationGuard } from "../lib/generationGuard";
import { isInsecureSubscriptionUrl, extractApplyWarning, formatApplyWarning, extractUpdateResults, formatUpdateFailures } from "../lib/subscriptions";

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
    <section className="panel">
      <h2>订阅</h2>
      {error && <p className="error">{error}</p>}
      {warning && <p className="warn">{warning}</p>}
      {updateFailures && (
        <p className="error">部分订阅更新失败：{updateFailures}</p>
      )}

      <form
        className="import-row"
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
        <input
          type="url"
          placeholder="订阅 URL（https 优先）"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          disabled={busy}
          required
        />
        <input
          type="text"
          placeholder="名称（可选）"
          value={name}
          onChange={(e) => setName(e.target.value)}
          disabled={busy}
        />
        <button type="submit" disabled={busy || !url.trim()}>
          导入
        </button>
        <button
          type="button"
          disabled={busy || items.length === 0}
          onClick={() => void run(() => api.updateAllSubscriptions(), true)}
        >
          {updating ? "更新中" : "全部更新"}
        </button>
        <button
          type="button"
          disabled={busy || items.length === 0}
          onClick={() => void run(() => api.applySubscriptions())}
        >
          应用配置
        </button>
      </form>
      {httpWarn && (
        <p className="warn">当前为 http://，传输未加密，建议改用 https。</p>
      )}

      {items.length === 0 ? (
        <p className="muted">暂无订阅。可直接以直连模式启动内核，也可导入订阅 URL。</p>
      ) : (
        <ul className="sub-list">
          {items.map((s) => (
            <li
              key={s.id}
              className={s.active ? "sub-item active" : "sub-item"}
            >
              <div className="sub-main">
                <strong>{s.name}</strong>
                <span className="muted">
                  {s.format} · {s.node_count} 节点 ·{" "}
                  {(s.group_count ?? 0)} 策略组 · {(s.rule_count ?? 0)} 规则
                  {s.has_dns ? " · DNS" : ""}
                  {s.last_updated
                    ? ` · ${new Date(s.last_updated).toLocaleString()}`
                    : ""}
                </span>
                {s.last_error && <span className="error">{s.last_error}</span>}
                {(s.parse_warnings ?? []).length > 0 && (
                  <span className="warn">
                    {s.parse_warnings.join("；")}
                  </span>
                )}
              </div>
              <div className="sub-actions">
                <label className="toggle">
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
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void run(() => api.updateSubscription(s.id), true)}
                >
                  {updating ? "更新中" : "更新"}
                </button>
                <button
                  type="button"
                  className="danger"
                  disabled={busy}
                  onClick={() => {
                    if (!window.confirm(`删除订阅「${s.name}」？`)) return;
                    void run(() => api.removeSubscription(s.id));
                  }}
                >
                  删除
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
