// SPDX-License-Identifier: GPL-3.0-or-later

import { useEffect, useRef, useState } from "react";
import {
  api,
  formatInvokeError,
  type AppSettings,
  type SettingsPatch,
  type StatusResponse,
  type CheckAppUpdateResponse,
} from "../api/tauri";
import {
  formatListenValidationError,
  formatPortValidationError,
  formatPortsConflictError,
  isLoopbackListenHost,
  parsePortInput,
  portsConflict,
} from "../lib/listenValidation";
import { ErrorAlert, OkAlert } from "../components/StatusAlert";
import { TunInstallDialog, useTunInstallDialog } from "../components/TunInstallDialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { t, useLanguagePreference } from "../lib/i18n";
import { useThemePreference } from "../lib/theme";
import { useRuntimeStore } from "../lib/runtimeStore";
import { AppearanceCard } from "./settings/Appearance";
import { DataCard } from "./settings/Data";
import { PortsCard } from "./settings/Ports";
import { TrayCard } from "./settings/Tray";
import { TunCard } from "./settings/Tun";
import { formatUpdateError, UpdateCard } from "./settings/Update";
import { isMacosHost } from "../lib/windowChrome";

const defaults: AppSettings = {
  mixed_listen: "127.0.0.1",
  mixed_port: 17890,
  clash_api_listen: "127.0.0.1",
  clash_api_port: 19090,
  selected_tag: null,
  auto_set_system_proxy: false,
  proxy_service_enabled: false,
  allow_lan: false,
  proxy_mode: "rule",
  auto_default_rules: true,
  language: "system",
  check_app_updates: true,
  log_debug: false,
  tray_display_mode: "icon_and_speed",
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

/// Debounce before persisting a changed setting (typing coalesces; switches
/// and radios feel instant).
const SAVE_DEBOUNCE_MS = 500;

/** Fields this page owns. Omitting Home-owned keys avoids last-writer races (ORCH-3).
 * `tun.enabled` is persisted only by the TUN switch (`persistTunEnabled`), never
 * by the debounced auto-save of other fields. */
function settingsOwnedPatch(form: AppSettings): SettingsPatch {
  const { enabled: _enabled, ...tunRest } = form.tun;
  return {
    mixed_listen: form.mixed_listen,
    mixed_port: form.mixed_port,
    clash_api_listen: form.clash_api_listen,
    clash_api_port: form.clash_api_port,
    auto_set_system_proxy: form.auto_set_system_proxy,
    allow_lan: form.allow_lan,
    tun: tunRest,
    auto_default_rules: form.auto_default_rules,
    language: form.language,
    check_app_updates: form.check_app_updates,
    log_debug: form.log_debug,
    tray_display_mode: form.tray_display_mode,
  };
}

export function Settings({
  active = true,
  availableUpdate = null,
  onAvailableUpdate,
  focusUpdateNonce = 0,
}: {
  active?: boolean;
  availableUpdate?: CheckAppUpdateResponse | null;
  onAvailableUpdate?: (info: CheckAppUpdateResponse | null) => void;
  focusUpdateNonce?: number;
}) {
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
  const runtime = useRuntimeStore();
  const saveTimerRef = useRef<number | null>(null);
  const scheduledSaveRef = useRef<AppSettings | null>(null);
  const saveInFlightRef = useRef(false);
  const pendingSaveRef = useRef<AppSettings | null>(null);
  const saveIdleWaitersRef = useRef<Array<() => void>>([]);
  /// Reset timer for the "saved" flash; tracked so the pending reset is
  /// cancelled on unmount (a stray fire into a torn-down test environment
  /// crashes with `window is not defined`) and superseded on every new save.
  const savedResetTimerRef = useRef<number | null>(null);
  /// Skip the first post-load snapshot so opening the page never persists
  /// the just-read settings; re-armed on every reload cycle.
  const skipInitialSaveRef = useRef(true);
  const [updateInfo, setUpdateInfo] = useState<CheckAppUpdateResponse | null>(
    availableUpdate,
  );
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateProgress, setUpdateProgress] = useState<{
    downloaded: number;
    contentLength: number | null;
  } | null>(null);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const updateCardRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (runtime?.status) {
      setStatus(runtime.status);
    }
  }, [runtime?.status]);

  useEffect(() => {
    if (availableUpdate?.available && availableUpdate.version) {
      setUpdateInfo(availableUpdate);
    }
  }, [availableUpdate]);

  useEffect(() => {
    if (!active || focusUpdateNonce <= 0) return;
    const node = updateCardRef.current;
    if (!node) return;
    const id = window.requestAnimationFrame(() => {
      node.scrollIntoView?.({ behavior: "smooth", block: "nearest" });
    });
    return () => window.cancelAnimationFrame(id);
  }, [active, focusUpdateNonce]);

  useEffect(() => {
    // Subscribe on mount so install progress is not lost to a post-click race.
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void api
      .listenAppUpdateProgress((payload) => {
        if (!cancelled) {
          setUpdateProgress({
            downloaded: payload.downloaded,
            contentLength: payload.content_length,
          });
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {
        // Progress events are best-effort.
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  /// Sidebar arrow follows auto-check: off hides it even if Settings still
  /// knows a newer version and can install it.
  function publishSidebarUpdate(
    info: CheckAppUpdateResponse | null,
    autoCheck: boolean,
  ) {
    if (!autoCheck) {
      onAvailableUpdate?.(null);
      return;
    }
    onAvailableUpdate?.(info?.available && info.version ? info : null);
  }

  async function runUpdateCheck() {
    setUpdateError(null);
    setUpdateBusy(true);
    try {
      const result = await api.checkAppUpdate(false);
      setUpdateInfo(result);
      publishSidebarUpdate(result, form.check_app_updates);
    } catch (e) {
      setUpdateError(formatUpdateError(e));
      setUpdateInfo(null);
    } finally {
      setUpdateBusy(false);
    }
  }

  async function runUpdateInstall() {
    setUpdateError(null);
    setUpdateBusy(true);
    setUpdateProgress({ downloaded: 0, contentLength: null });
    try {
      await api.installAppUpdate();
    } catch (e) {
      setUpdateError(formatUpdateError(e));
      setUpdateBusy(false);
      setUpdateProgress(null);
    }
  }

  /// Briefly show the "saved" confirmation; re-armed on each save.
  function flashSaved() {
    setSaved(true);
    if (savedResetTimerRef.current !== null) {
      window.clearTimeout(savedResetTimerRef.current);
    }
    savedResetTimerRef.current = window.setTimeout(() => {
      savedResetTimerRef.current = null;
      setSaved(false);
    }, 2000);
  }

  useEffect(() => {
    return () => {
      if (savedResetTimerRef.current !== null) {
        window.clearTimeout(savedResetTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!active) {
      flushScheduledSave();
      setLoaded(false);
      skipInitialSaveRef.current = true;
      return;
    }
    setLoaded(false);
    skipInitialSaveRef.current = true;
    let cancelled = false;
    void (async () => {
      try {
        await waitForSaveIdle();
        if (cancelled) return;
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
      void runtime?.refreshStatus();
      if (s.helper_installed !== expectedInstalled) {
        setError(t("settings.helperStatusUnconfirmed"));
        return;
      }
      await afterReady?.();
      flashSaved();
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
   * was dropped by validation. Autosave omits this field, so a successful
   * save must refresh shared status (Home reads `configured_tun` from it)
   * and a failed save must roll the switch back. */
  async function persistTunEnabled(enabled: boolean) {
    const previous = form.tun.enabled;
    const candidate = { ...form, tun: { ...form.tun, enabled } };
    const errs = validateForm(candidate);
    setFieldErrors(errs);
    if (Object.keys(errs).length > 0) {
      throw new Error(t("settings.tunNotSaved"));
    }
    setForm((prev) => ({ ...prev, tun: { ...prev.tun, enabled } }));
    try {
      await api.saveSettings({ tun: { enabled } });
    } catch (err) {
      setForm((prev) => ({ ...prev, tun: { ...prev.tun, enabled: previous } }));
      throw err;
    }
    void runtime?.refreshStatus();
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
      await api.saveSettings(settingsOwnedPatch(candidate));
      flashSaved();
    } catch (err) {
      setError(formatInvokeError(err));
    } finally {
      saveInFlightRef.current = false;
      const next = pendingSaveRef.current;
      pendingSaveRef.current = null;
      if (next) {
        void flushSave(next);
      } else {
        const waiters = saveIdleWaitersRef.current.splice(0);
        for (const resolve of waiters) resolve();
      }
    }
  }

  function waitForSaveIdle(): Promise<void> {
    if (!saveInFlightRef.current && pendingSaveRef.current === null) {
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      saveIdleWaitersRef.current.push(resolve);
    });
  }

  function cancelScheduledSave() {
    if (saveTimerRef.current !== null) {
      window.clearTimeout(saveTimerRef.current);
      saveTimerRef.current = null;
    }
    scheduledSaveRef.current = null;
  }

  function flushScheduledSave() {
    const candidate = scheduledSaveRef.current;
    cancelScheduledSave();
    if (candidate) void flushSave(candidate);
  }

  function scheduleAutoSave(next: AppSettings) {
    cancelScheduledSave();
    scheduledSaveRef.current = next;
    saveTimerRef.current = window.setTimeout(() => {
      const candidate = scheduledSaveRef.current;
      saveTimerRef.current = null;
      scheduledSaveRef.current = null;
      if (candidate) void flushSave(candidate);
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
    return cancelScheduledSave;
  }, [form, loaded]);

  /** Windows hides the TUN controls entirely (gate blocked upstream). */
  const tunUiHidden = status?.tun_ui_hidden === true;

  return (
    <div className="settings-panel flex min-h-0 flex-1 flex-col gap-3" data-testid="settings-panel">
      {error && <ErrorAlert className="shrink-0">{error}</ErrorAlert>}
      {saved && <OkAlert className="shrink-0">{t("common.saved")}</OkAlert>}

      <ScrollArea
        type="scroll"
        scrollHideDelay={600}
        className="min-h-0 flex-1 overflow-hidden"
      >
        <div className="flex w-full flex-col gap-3" data-testid="settings-stack">
          <AppearanceCard
            themePreference={themePreference}
            setThemePreference={setThemePreference}
            language={form.language}
            busy={busy}
            loaded={loaded}
            onLanguageChange={(value) => {
              setPreference(value);
              setForm({ ...form, language: value });
            }}
          />

          {isMacosHost() && (
            <TrayCard
              mode={form.tray_display_mode}
              busy={busy}
              loaded={loaded}
              onChange={(value) => setForm({ ...form, tray_display_mode: value })}
            />
          )}

          <div ref={updateCardRef} id="settings-app-update">
            <UpdateCard
              checkAppUpdates={form.check_app_updates}
              busy={busy}
              loaded={loaded}
              updateBusy={updateBusy}
              updateInfo={updateInfo}
              updateProgress={updateProgress}
              updateError={updateError}
              onCheckAppUpdatesChange={(enabled) => {
                setForm({
                  ...form,
                  check_app_updates: enabled,
                });
                if (enabled) {
                  publishSidebarUpdate(updateInfo, true);
                } else {
                  onAvailableUpdate?.(null);
                }
              }}
              onCheck={() => void runUpdateCheck()}
              onInstall={() => void runUpdateInstall()}
            />
          </div>

          {tunUiHidden ? null : (
            <TunCard
              form={form}
              status={status}
              busy={busy}
              loaded={loaded}
              persistTunEnabled={persistTunEnabled}
              flashSaved={flashSaved}
              setError={setError}
              onRequestHelperInstall={() => tunInstall.setOpen(true)}
              onInstallHelper={() =>
                void runHelperAction(() => api.installHelper(), true)
              }
              onUninstallHelper={() =>
                void runHelperAction(() => api.uninstallHelper(), false, () => {
                  if (!form.tun.enabled) return;
                  return persistTunEnabled(false);
                })
              }
            />
          )}

          <PortsCard
            form={form}
            setForm={setForm}
            fieldErrors={fieldErrors}
            busy={busy}
            loaded={loaded}
            clearFieldError={clearFieldError}
            setFieldErrors={setFieldErrors}
          />

          <DataCard
            form={form}
            setForm={setForm}
            busy={busy}
            loaded={loaded}
            setError={setError}
          />
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
