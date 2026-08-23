import { useEffect, useState, type FormEvent } from "react";
import {
  api,
  formatInvokeError,
  type AppSettings,
} from "../api/tauri";
import {
  formatListenValidationError,
  formatPortValidationError,
  formatPortsConflictError,
  isLoopbackListenHost,
  parsePortInput,
  portsConflict,
} from "../lib/generationGuard";

const defaults: AppSettings = {
  mixed_listen: "127.0.0.1",
  mixed_port: 17890,
  clash_api_listen: "127.0.0.1",
  clash_api_port: 19090,
  selected_tag: null,
  auto_set_system_proxy: false,
  allow_lan: false,
  proxy_mode: "rule",
};

export function Settings() {
  const [form, setForm] = useState<AppSettings>(defaults);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const settings = await api.getSettings();
        if (!cancelled) {
          setForm(settings);
          setLoaded(true);
        }
      } catch (e) {
        if (!cancelled) {
          setError(formatInvokeError(e));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  function validateForm(next: AppSettings): Record<string, string> {
    const errs: Record<string, string> = {};
    if (!next.allow_lan && !isLoopbackListenHost(next.mixed_listen)) {
      errs.mixed_listen = formatListenValidationError("Mixed 监听");
    }
    if (!isLoopbackListenHost(next.clash_api_listen)) {
      errs.clash_api_listen = formatListenValidationError("Clash API 监听");
    }
    if (parsePortInput(String(next.mixed_port)) === undefined) {
      errs.mixed_port = formatPortValidationError("Mixed 端口");
    }
    if (parsePortInput(String(next.clash_api_port)) === undefined) {
      errs.clash_api_port = formatPortValidationError("Clash API 端口");
    }
    if (
      parsePortInput(String(next.mixed_port)) !== undefined &&
      parsePortInput(String(next.clash_api_port)) !== undefined &&
      portsConflict(next.mixed_port, next.clash_api_port)
    ) {
      const msg = formatPortsConflictError();
      errs.mixed_port = msg;
      errs.clash_api_port = msg;
    }
    return errs;
  }

  async function onSave(e: FormEvent) {
    e.preventDefault();
    if (!loaded) return;
    const errs = validateForm(form);
    setFieldErrors(errs);
    if (Object.keys(errs).length > 0) return;

    setBusy(true);
    setError(null);
    setSaved(false);
    try {
      await api.saveSettings(form);
      setSaved(true);
    } catch (err) {
      setError(formatInvokeError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel">
      <h2>设置</h2>
      {error && <p className="error">{error}</p>}
      {saved && <p className="ok">已保存</p>}
      <form className="settings-form" onSubmit={(e) => void onSave(e)}>
        <label>
          Mixed 监听
          <input
            value={form.mixed_listen}
            onChange={(e) => {
              setFieldErrors((prev) => {
                const next = { ...prev };
                delete next.mixed_listen;
                return next;
              });
              setForm({ ...form, mixed_listen: e.target.value });
            }}
            disabled={busy || !loaded || form.allow_lan}
          />
          {fieldErrors.mixed_listen && (
            <span className="error">{fieldErrors.mixed_listen}</span>
          )}
        </label>
        <label>
          Mixed 端口
          <input
            type="number"
            min={1024}
            max={65535}
            value={Number.isFinite(form.mixed_port) ? form.mixed_port : ""}
            onChange={(e) => {
              const raw = e.target.value;
              setFieldErrors((prev) => {
                const next = { ...prev };
                delete next.mixed_port;
                return next;
              });
              if (raw.trim() === "") {
                setFieldErrors((prev) => ({
                  ...prev,
                  mixed_port: formatPortValidationError("Mixed 端口"),
                }));
                setForm({ ...form, mixed_port: Number.NaN });
                return;
              }
              const n = Number(raw);
              if (Number.isFinite(n)) {
                setForm({ ...form, mixed_port: n });
              }
            }}
            disabled={busy || !loaded}
          />
          {fieldErrors.mixed_port && (
            <span className="error">{fieldErrors.mixed_port}</span>
          )}
        </label>
        <label>
          Clash API 监听
          <input
            value={form.clash_api_listen}
            onChange={(e) => {
              setFieldErrors((prev) => {
                const next = { ...prev };
                delete next.clash_api_listen;
                return next;
              });
              setForm({ ...form, clash_api_listen: e.target.value });
            }}
            disabled={busy || !loaded}
          />
          {fieldErrors.clash_api_listen && (
            <span className="error">{fieldErrors.clash_api_listen}</span>
          )}
        </label>
        <label>
          Clash API 端口
          <input
            type="number"
            min={1024}
            max={65535}
            value={Number.isFinite(form.clash_api_port) ? form.clash_api_port : ""}
            onChange={(e) => {
              const raw = e.target.value;
              setFieldErrors((prev) => {
                const next = { ...prev };
                delete next.clash_api_port;
                return next;
              });
              if (raw.trim() === "") {
                setFieldErrors((prev) => ({
                  ...prev,
                  clash_api_port: formatPortValidationError("Clash API 端口"),
                }));
                setForm({ ...form, clash_api_port: Number.NaN });
                return;
              }
              const n = Number(raw);
              if (Number.isFinite(n)) {
                setForm({ ...form, clash_api_port: n });
              }
            }}
            disabled={busy || !loaded}
          />
          {fieldErrors.clash_api_port && (
            <span className="error">{fieldErrors.clash_api_port}</span>
          )}
        </label>
        <label className="toggle">
          <input
            type="checkbox"
            checked={form.allow_lan}
            onChange={(e) => {
              setFieldErrors((prev) => {
                const next = { ...prev };
                delete next.mixed_listen;
                return next;
              });
              setForm({ ...form, allow_lan: e.target.checked });
            }}
            disabled={busy || !loaded}
          />
          允许局域网共享（Allow LAN）
        </label>
        {form.allow_lan && (
          <p className="hint">
            局域网共享时 Mixed 入站监听 0.0.0.0，其他设备可通过本机局域网 IP
            连接；Clash API 仍仅限本机
          </p>
        )}
        <label className="toggle">
          <input
            type="checkbox"
            checked={form.auto_set_system_proxy}
            onChange={(e) =>
              setForm({ ...form, auto_set_system_proxy: e.target.checked })
            }
            disabled={busy || !loaded}
          />
          启动时设置系统代理
        </label>
        <div className="actions">
          <button type="submit" disabled={busy || !loaded}>
            保存
          </button>
          <button
            type="button"
            disabled={busy || !loaded}
            onClick={() =>
              void api
                .revealDataDir()
                .catch((err) => setError(formatInvokeError(err)))
            }
          >
            打开数据目录
          </button>
        </div>
      </form>
    </section>
  );
}
