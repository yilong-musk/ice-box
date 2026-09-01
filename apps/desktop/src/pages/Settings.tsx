import { useEffect, useRef, useState } from "react";
import {
  api,
  formatInvokeError,
  type AppSettings,
  type StatusResponse,
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
import { TunInstallDialog, useTunInstallDialog } from "../components/TunInstallDialog";
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
import {
  NativeSelect,
  NativeSelectOption,
} from "@/components/ui/native-select";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import {
  t,
  useLanguagePreference,
  type LanguagePreference,
  type MessageKey,
} from "../lib/i18n";
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
  auto_default_rules: true,
  language: "system",
  tun: {
    enabled: false,
    interface_name: null,
    ipv4_address: "10.0.0.1/30",
    ipv6_address: "fdfe:dcba:9876::1/126",
    mtu: 9000,
    auto_route: true,
    strict_route: true,
    stack: "gvisor",
    dns_hijack: true,
  },
};

const APPEARANCE_OPTIONS = [
  ["system", "settings.appearance.system"],
  ["light", "settings.appearance.light"],
  ["dark", "settings.appearance.dark"],
] as const satisfies ReadonlyArray<
  readonly [ThemePreference, MessageKey]
>;

const LANGUAGE_OPTIONS = [
  ["system", "settings.language.system"],
  ["zh", "settings.language.zh"],
  ["en", "settings.language.en"],
] as const satisfies ReadonlyArray<
  readonly [LanguagePreference, MessageKey]
>;

/** TUN lifecycle labels shown in the settings card while a transition runs. */
const TUN_TRANSITION_KEYS: Record<string, MessageKey> = {
  preparing: "settings.tunTransition.preparing",
  stopping: "settings.tunTransition.stopping",
};

/// Debounce before persisting a changed setting (typing coalesces; switches
/// and radios feel instant).
const SAVE_DEBOUNCE_MS = 500;

export function Settings({ active = true }: { active?: boolean }) {
  const [form, setForm] = useState<AppSettings>(defaults);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [status, setStatus] = useState<StatusResponse | null>(null);
  const tunInstall = useTunInstallDialog(installHelperThenEnableTun);
  const { preference, setPreference } = useLanguagePreference();
  const { preference: themePreference, setPreference: setThemePreference } =
    useThemePreference();
  const saveTimerRef = useRef<number | null>(null);
  const saveInFlightRef = useRef(false);
  const pendingSaveRef = useRef<AppSettings | null>(null);
  /// Skip the first post-load snapshot so opening the page never persists
  /// the just-read settings; re-armed on every reload cycle.
  const skipInitialSaveRef = useRef(true);

  useEffect(() => {
    setLoaded(false);
    skipInitialSaveRef.current = true;
    if (!active) return;
    let cancelled = false;
    void (async () => {
      try {
        const [settings, s] = await Promise.all([
          api.getSettings(),
          api.getStatus(),
        ]);
        if (!cancelled) {
          setForm(settings);
          setStatus(s);
          setLoaded(true);
          // settings.json is the authoritative language preference; re-apply
          // it when it differs from the cached one (e.g. first launch after a
          // storage reset, or a manual edit of the settings file).
          if (settings.language !== preference) {
            setPreference(settings.language);
          }
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
    // Only consulted when settings are (re)loaded, so `preference` is
    // intentionally not a dependency.
  }, [active]);

  function clearFieldError(key: string) {
    setFieldErrors((prev) => {
      const next = { ...prev };
      delete next[key];
      return next;
    });
  }

  /** In-app helper install/uninstall (unsigned elevation path): prompts the
   * system authorization dialog; cancel modifies nothing. After the action,
   * polls `getStatus` until the expected helper state is observed — launchd
   * bootstrap returns before the daemon binds its socket, so a single probe
   * right after install can still report the helper as missing. When the
   * state never converges, the action is reported as unconfirmed (fail-closed:
   * no success flash, no follow-up persistence). */
  async function runHelperAction(
    action: () => Promise<void>,
    expectedInstalled: boolean,
    afterReady?: () => void | Promise<void>,
  ) {
    setBusy(true);
    setError(null);
    try {
      await action();
      let s = await api.getStatus();
      for (
        let attempt = 0;
        attempt < 8 && s.helper_installed !== expectedInstalled;
        attempt++
      ) {
        await new Promise((resolve) => window.setTimeout(resolve, 400));
        s = await api.getStatus();
      }
      setStatus(s);
      if (s.helper_installed !== expectedInstalled) {
        setError(t("settings.helperStatusUnconfirmed"));
        return;
      }
      await afterReady?.();
      setSaved(true);
      window.setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(formatInvokeError(e));
    } finally {
      setBusy(false);
    }
  }

  /** Persist `tun.enabled` directly (bypassing the debounced auto-save) with
   * the same validation the save pipeline applies. An invalid form rejects
   * with a visible error instead of silently keeping the change unsaved —
   * e.g. a guided install must never flash「已保存」while the TUN-on setting
   * was dropped by validation. */
  async function persistTunEnabled(enabled: boolean) {
    const candidate = { ...form, tun: { ...form.tun, enabled } };
    const errs = validateForm(candidate);
    setFieldErrors(errs);
    if (Object.keys(errs).length > 0) {
      throw new Error(t("settings.tunNotSaved"));
    }
    await api.saveSettings(candidate);
    setForm(candidate);
  }

  /** Enabling TUN without an authorized helper: install first, then persist
   * the TUN-on setting. Cancel or a failed install leaves the switch off and
   * settings untouched. */
  function installHelperThenEnableTun() {
    void runHelperAction(
      () => api.installHelper(),
      true,
      () => persistTunEnabled(true),
    );
  }

  function validateForm(next: AppSettings): Record<string, string> {
    const errs: Record<string, string> = {};
    if (!next.allow_lan && !isLoopbackListenHost(next.mixed_listen)) {
      errs.mixed_listen = formatListenValidationError(t("settings.mixedListen"));
    }
    if (!isLoopbackListenHost(next.clash_api_listen)) {
      errs.clash_api_listen = formatListenValidationError(
        t("settings.clashListen"),
      );
    }
    if (parsePortInput(String(next.mixed_port)) === undefined) {
      errs.mixed_port = formatPortValidationError(t("settings.mixedPort"));
    }
    if (parsePortInput(String(next.clash_api_port)) === undefined) {
      errs.clash_api_port = formatPortValidationError(t("settings.clashPort"));
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

  /** Validate + persist one candidate. Serialized (latest-wins queue) so a
   * slow settings apply never interleaves with the next change. Invalid
   * candidates are rejected with field errors and the on-disk settings stay
   * untouched. */
  async function flushSave(candidate: AppSettings) {
    if (saveInFlightRef.current) {
      pendingSaveRef.current = candidate;
      return;
    }
    saveInFlightRef.current = true;
    try {
      const errs = validateForm(candidate);
      setFieldErrors(errs);
      if (Object.keys(errs).length > 0) {
        return;
      }
      setError(null);
      await api.saveSettings(candidate);
      setSaved(true);
      window.setTimeout(() => setSaved(false), 2000);
    } catch (err) {
      setError(formatInvokeError(err));
    } finally {
      saveInFlightRef.current = false;
      const next = pendingSaveRef.current;
      pendingSaveRef.current = null;
      if (next) void flushSave(next);
    }
  }

  function scheduleAutoSave(next: AppSettings) {
    if (saveTimerRef.current !== null) {
      window.clearTimeout(saveTimerRef.current);
    }
    saveTimerRef.current = window.setTimeout(() => {
      saveTimerRef.current = null;
      void flushSave(next);
    }, SAVE_DEBOUNCE_MS);
  }

  // Auto-save: any form change persists after the debounce; the just-loaded
  // snapshot is never written back (skipInitialSaveRef).
  useEffect(() => {
    if (!loaded) return;
    if (skipInitialSaveRef.current) {
      skipInitialSaveRef.current = false;
      return;
    }
    scheduleAutoSave(form);
    return () => {
      if (saveTimerRef.current !== null) {
        window.clearTimeout(saveTimerRef.current);
        saveTimerRef.current = null;
      }
    };
  }, [form, loaded]);

  /** Windows hides the TUN controls entirely (gate blocked upstream). */
  const tunUiHidden = status?.tun_ui_hidden === true;

  return (
    <div className="settings-panel flex min-h-0 flex-1 flex-col gap-3">
      {error && <ErrorAlert className="shrink-0">{error}</ErrorAlert>}
      {saved && <OkAlert className="shrink-0">{t("common.saved")}</OkAlert>}

      <ScrollArea
        type="scroll"
        scrollHideDelay={600}
        className="min-h-0 flex-1 overflow-hidden"
      >
        <div className="flex w-full flex-col gap-3">
          <Card size="sm" className="w-full shrink-0 data-[size=sm]:[--card-spacing:--spacing(2)]">
        <CardHeader>
          <CardTitle>{t("settings.appearance")}</CardTitle>
          <CardDescription>{t("settings.appearanceDesc")}</CardDescription>
        </CardHeader>
        <CardContent>
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            spacing={2}
            value={themePreference}
            onValueChange={(value) => {
              if (
                value === "system" ||
                value === "light" ||
                value === "dark"
              ) {
                setThemePreference(value);
              }
            }}
            className="w-full"
            aria-label={t("settings.appearance")}
          >
            {APPEARANCE_OPTIONS.map(([value, labelKey]) => (
              <ToggleGroupItem key={value} value={value} className="flex-1">
                {t(labelKey)}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </CardContent>
      </Card>

          <Card size="sm" className="w-full shrink-0 data-[size=sm]:[--card-spacing:--spacing(2)]">
        <CardHeader>
          <CardTitle>{t("settings.language")}</CardTitle>
          <CardDescription>{t("settings.languageDesc")}</CardDescription>
        </CardHeader>
        <CardContent>
          <Field>
            <NativeSelect
              id="settings-language"
              aria-label={t("settings.language")}
              size="sm"
              className="w-full max-w-60"
              value={form.language}
              onChange={(e) => {
                const value = e.target.value;
                if (value === "system" || value === "zh" || value === "en") {
                  // Apply immediately (localStorage cache + live re-render);
                  // the auto-save pipeline persists it in settings.json.
                  setPreference(value);
                  setForm({ ...form, language: value });
                }
              }}
            >
              {LANGUAGE_OPTIONS.map(([value, labelKey]) => (
                <NativeSelectOption key={value} value={value}>
                  {t(labelKey)}
                </NativeSelectOption>
              ))}
            </NativeSelect>
          </Field>
        </CardContent>
      </Card>

      {tunUiHidden ? null : (
        <Card size="sm" className="w-full shrink-0 data-[size=sm]:[--card-spacing:--spacing(2)]">
          <CardHeader>
            <CardTitle>{t("settings.tun")}</CardTitle>
            <CardDescription>{t("settings.tunDesc")}</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="flex flex-col gap-3">
            <Field orientation="horizontal" className="w-auto gap-2">
              <Switch
                id="settings-tun-enabled"
                size="sm"
                checked={form.tun.enabled}
                disabled={
                  busy ||
                  !loaded ||
                  status?.tun_available === false ||
                  status?.tun_status === "preparing" ||
                  status?.tun_status === "stopping" ||
                  status?.helper_stale === true
                }
                aria-label={t("settings.tunEnable")}
                onCheckedChange={(checked) => {
                  if (checked === true && status?.helper_installed !== true) {
                    // No authorized helper: guide the user to install it first;
                    // the TUN-on setting is persisted only after a successful
                    // install (cancel leaves the switch off).
                    tunInstall.setOpen(true);
                    return;
                  }
                  setForm({
                    ...form,
                    tun: { ...form.tun, enabled: checked === true },
                  });
                }}
              />
              <FieldLabel htmlFor="settings-tun-enabled">
                {t("settings.tunEnable")}
              </FieldLabel>
            </Field>
            {TUN_TRANSITION_KEYS[status?.tun_status ?? ""] ? (
              <FieldDescription>
                {t(TUN_TRANSITION_KEYS[status?.tun_status ?? ""])}
              </FieldDescription>
            ) : status?.tun_status === "recovery_required" ? (
              <FieldDescription>
                {t("settings.tunRecoveryRequired")}
              </FieldDescription>
            ) : status?.tun_available === false ? (
              <FieldDescription>
                {status?.tun_unavailable_reason ??
                  t("settings.tunNotSupported")}
              </FieldDescription>
            ) : status?.traffic_capture === "tun" ? (
              <FieldDescription>
                {t("settings.tunActiveWithIface", {
                  interface: status.tun_interface
                    ? `（${t("common.withIfaceLabel", {
                        iface: status.tun_interface,
                      })}）`
                    : "",
                })}
              </FieldDescription>
            ) : status?.helper_stale === true ? (
              <FieldDescription>{t("settings.helperStale")}</FieldDescription>
            ) : (
              <FieldDescription>
                {status?.helper_installed
                  ? t("settings.helperReady")
                  : t("settings.helperNeeded")}
              </FieldDescription>
            )}
            <div className="flex flex-wrap gap-2">
              <Button
                type="button"
                size="sm"
                onClick={() => void runHelperAction(() => api.installHelper(), true)}
                disabled={
                  busy ||
                  (status?.helper_installed === true &&
                    status?.helper_stale !== true) ||
                  status?.tun_status === "preparing" ||
                  status?.tun_status === "stopping" ||
                  status?.traffic_capture === "tun"
                }
              >
                {status?.helper_stale === true
                  ? t("settings.updateHelper")
                  : t("settings.installHelper")}
              </Button>
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={() =>
                    void runHelperAction(() => api.uninstallHelper(), false, () => {
                      // The helper is gone: the TUN-on setting can no longer be
                      // applied, so persist it off with the uninstall.
                      if (!form.tun.enabled) return;
                      return persistTunEnabled(false);
                    })
                  }
                disabled={
                  busy ||
                  status?.helper_installed !== true ||
                  status?.tun_status === "preparing" ||
                  status?.tun_status === "stopping" ||
                  status?.traffic_capture === "tun"
                }
              >
                {t("settings.uninstallHelper")}
              </Button>
            </div>
            </div>
          </CardContent>
        </Card>
      )}

      <Card size="sm" className="w-full">
        <CardHeader className="shrink-0">
          <CardTitle>{t("settings.inbound")}</CardTitle>
          <CardDescription>{t("settings.inboundDesc")}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex flex-col gap-3">
            <FieldGroup className="grid grid-cols-1 gap-3 min-[560px]:grid-cols-2">
              <Field data-invalid={!!fieldErrors.mixed_listen || undefined}>
                <FieldLabel htmlFor="settings-mixed-listen">
                  {t("settings.mixedListen")}
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
                <FieldLabel htmlFor="settings-mixed-port">
                  {t("settings.mixedPort")}
                </FieldLabel>
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
                        mixed_port: formatPortValidationError(
                          t("settings.mixedPort"),
                        ),
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
                  {t("settings.clashListen")}
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
                  {t("settings.clashPort")}
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
                        clash_api_port: formatPortValidationError(
                          t("settings.clashPort"),
                        ),
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
                aria-label={t("settings.allowLan")}
                onCheckedChange={(checked) => {
                  clearFieldError("mixed_listen");
                  setForm({ ...form, allow_lan: checked === true });
                }}
              />
              <FieldLabel htmlFor="settings-allow-lan">
                {t("settings.allowLan")}
              </FieldLabel>
            </Field>
            {form.allow_lan ? (
              <FieldDescription>
                {t("settings.allowLanDesc")}
              </FieldDescription>
            ) : null}
            <Field orientation="horizontal" className="w-auto gap-2">
              <Switch
                id="settings-auto-default-rules"
                size="sm"
                checked={form.auto_default_rules}
                disabled={busy || !loaded}
                aria-label={t("settings.autoDefaultRules")}
                onCheckedChange={(checked) => {
                  setForm({
                    ...form,
                    auto_default_rules: checked === true,
                  });
                }}
              />
              <FieldLabel htmlFor="settings-auto-default-rules">
                {t("settings.autoDefaultRules")}
              </FieldLabel>
            </Field>
            {form.auto_default_rules ? (
              <FieldDescription>
                {t("settings.autoDefaultRulesDesc")}
              </FieldDescription>
            ) : null}
            <div className="flex flex-wrap gap-2">
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
                {t("settings.openDataDir")}
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>
        </div>
      </ScrollArea>

      {!tunUiHidden && (
        <TunInstallDialog
          open={tunInstall.open}
          onOpenChange={tunInstall.setOpen}
          onConfirm={tunInstall.confirm}
          busy={busy}
        />
      )}
    </div>
  );
}
