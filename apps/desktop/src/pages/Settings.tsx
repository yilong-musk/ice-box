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
import { ErrorAlert, OkAlert } from "../components/StatusAlert";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Field,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { useThemePreference, type ThemePreference } from "../lib/theme";

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

const APPEARANCE_OPTIONS = [
  ["system", "跟随系统"],
  ["light", "浅色"],
  ["dark", "深色"],
] as const satisfies ReadonlyArray<readonly [ThemePreference, string]>;

export function Settings({ active = true }: { active?: boolean }) {
  const [form, setForm] = useState<AppSettings>(defaults);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const { preference, setPreference } = useThemePreference();

  useEffect(() => {
    setLoaded(false);
    if (!active) return;
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
  }, [active]);

  function clearFieldError(key: string) {
    setFieldErrors((prev) => {
      const next = { ...prev };
      delete next[key];
      return next;
    });
  }

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
    <div className="settings-panel flex min-h-0 flex-1 flex-col gap-3">
      {error && <ErrorAlert className="shrink-0">{error}</ErrorAlert>}
      {saved && <OkAlert className="shrink-0">已保存</OkAlert>}

      <Card size="sm" className="w-full shrink-0 data-[size=sm]:[--card-spacing:--spacing(2)]">
        <CardHeader>
          <CardTitle>外观</CardTitle>
          <CardDescription>
            默认跟随系统深浅色。切换后立即生效，不必点保存。
          </CardDescription>
        </CardHeader>
        <CardContent>
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            spacing={2}
            value={preference}
            onValueChange={(value) => {
              if (value === "system" || value === "light" || value === "dark") {
                setPreference(value);
              }
            }}
            className="w-full"
            aria-label="外观"
          >
            {APPEARANCE_OPTIONS.map(([value, label]) => (
              <ToggleGroupItem key={value} value={value} className="flex-1">
                {label}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </CardContent>
      </Card>

      <Card
        size="sm"
        className="flex min-h-0 w-full flex-1 flex-col overflow-hidden"
      >
        <CardHeader className="shrink-0">
          <CardTitle>入站</CardTitle>
          <CardDescription>Mixed 与 Clash API 监听地址</CardDescription>
        </CardHeader>
        <CardContent className="flex min-h-0 flex-1 flex-col overflow-auto">
          <form
            className="flex flex-col gap-3"
            onSubmit={(e) => void onSave(e)}
          >
            <FieldGroup className="grid grid-cols-1 gap-3 min-[560px]:grid-cols-2">
              <Field data-invalid={!!fieldErrors.mixed_listen || undefined}>
                <FieldLabel htmlFor="settings-mixed-listen">
                  Mixed 监听
                </FieldLabel>
                <Input
                  id="settings-mixed-listen"
                  value={form.mixed_listen}
                  aria-invalid={!!fieldErrors.mixed_listen || undefined}
                  onChange={(e) => {
                    clearFieldError("mixed_listen");
                    setForm({ ...form, mixed_listen: e.target.value });
                  }}
                  disabled={busy || !loaded || form.allow_lan}
                />
                {fieldErrors.mixed_listen ? (
                  <FieldError>{fieldErrors.mixed_listen}</FieldError>
                ) : null}
              </Field>
              <Field data-invalid={!!fieldErrors.mixed_port || undefined}>
                <FieldLabel htmlFor="settings-mixed-port">Mixed 端口</FieldLabel>
                <Input
                  id="settings-mixed-port"
                  type="number"
                  min={1024}
                  max={65535}
                  value={Number.isFinite(form.mixed_port) ? form.mixed_port : ""}
                  aria-invalid={!!fieldErrors.mixed_port || undefined}
                  onChange={(e) => {
                    const raw = e.target.value;
                    clearFieldError("mixed_port");
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
                {fieldErrors.mixed_port ? (
                  <FieldError>{fieldErrors.mixed_port}</FieldError>
                ) : null}
              </Field>
              <Field data-invalid={!!fieldErrors.clash_api_listen || undefined}>
                <FieldLabel htmlFor="settings-clash-listen">
                  Clash API 监听
                </FieldLabel>
                <Input
                  id="settings-clash-listen"
                  value={form.clash_api_listen}
                  aria-invalid={!!fieldErrors.clash_api_listen || undefined}
                  onChange={(e) => {
                    clearFieldError("clash_api_listen");
                    setForm({ ...form, clash_api_listen: e.target.value });
                  }}
                  disabled={busy || !loaded}
                />
                {fieldErrors.clash_api_listen ? (
                  <FieldError>{fieldErrors.clash_api_listen}</FieldError>
                ) : null}
              </Field>
              <Field data-invalid={!!fieldErrors.clash_api_port || undefined}>
                <FieldLabel htmlFor="settings-clash-port">
                  Clash API 端口
                </FieldLabel>
                <Input
                  id="settings-clash-port"
                  type="number"
                  min={1024}
                  max={65535}
                  value={
                    Number.isFinite(form.clash_api_port)
                      ? form.clash_api_port
                      : ""
                  }
                  aria-invalid={!!fieldErrors.clash_api_port || undefined}
                  onChange={(e) => {
                    const raw = e.target.value;
                    clearFieldError("clash_api_port");
                    if (raw.trim() === "") {
                      setFieldErrors((prev) => ({
                        ...prev,
                        clash_api_port:
                          formatPortValidationError("Clash API 端口"),
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
                {fieldErrors.clash_api_port ? (
                  <FieldError>{fieldErrors.clash_api_port}</FieldError>
                ) : null}
              </Field>
            </FieldGroup>
            <Field orientation="horizontal" className="w-auto gap-2">
              <Switch
                id="settings-allow-lan"
                size="sm"
                checked={form.allow_lan}
                disabled={busy || !loaded}
                aria-label="允许局域网共享（Allow LAN）"
                onCheckedChange={(checked) => {
                  clearFieldError("mixed_listen");
                  setForm({ ...form, allow_lan: checked === true });
                }}
              />
              <FieldLabel htmlFor="settings-allow-lan">
                允许局域网共享（Allow LAN）
              </FieldLabel>
            </Field>
            {form.allow_lan ? (
              <FieldDescription>
                局域网共享时 Mixed 入站监听 0.0.0.0，其他设备可通过本机局域网 IP
                连接；Clash API 仍仅限本机
              </FieldDescription>
            ) : null}
            <div className="flex flex-wrap gap-2">
              <Button type="submit" size="sm" disabled={busy || !loaded}>
                保存
              </Button>
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={busy || !loaded}
                onClick={() =>
                  void api
                    .revealDataDir()
                    .catch((err) => setError(formatInvokeError(err)))
                }
              >
                打开数据目录
              </Button>
            </div>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}
