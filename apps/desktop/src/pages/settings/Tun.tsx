// SPDX-License-Identifier: GPL-3.0-or-later

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
  FieldLabel,
} from "@/components/ui/field";
import { Switch } from "@/components/ui/switch";
import { formatInvokeError, formatUiMessage, api, type AppSettings, type StatusResponse } from "../../api/tauri";
import { t, type MessageKey } from "../../lib/i18n";
import { HelperActions } from "./Helper";

/** TUN lifecycle labels shown in the settings card while a transition runs. */
const TUN_TRANSITION_KEYS: Record<string, MessageKey> = {
  preparing: "settings.tunTransition.preparing",
  stopping: "settings.tunTransition.stopping",
};

export function TunCard({
  form,
  setForm,
  status,
  busy,
  loaded,
  persistTunEnabled,
  flashSaved,
  setError,
  onRequestHelperInstall,
  onInstallHelper,
  onUninstallHelper,
}: {
  form: AppSettings;
  setForm: (next: AppSettings) => void;
  status: StatusResponse | null;
  busy: boolean;
  loaded: boolean;
  persistTunEnabled: (enabled: boolean) => Promise<void>;
  flashSaved: () => void;
  setError: (error: string | null) => void;
  onRequestHelperInstall: () => void;
  onInstallHelper: () => void;
  onUninstallHelper: () => void;
}) {
  return (
    <Card
      size="sm"
      className="w-full shrink-0 data-[size=sm]:[--card-spacing:--spacing(2)]"
    >
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
                if (
                  checked === true &&
                  status?.helper_supported === true &&
                  status?.helper_installed !== true
                ) {
                  onRequestHelperInstall();
                  return;
                }
                if (checked === true && status?.helper_supported === false) {
                  setError(null);
                  void (async () => {
                    try {
                      if (status?.tun_elevation_ready !== true) {
                        await api.ensureTunElevation();
                      }
                      await persistTunEnabled(true);
                      flashSaved();
                    } catch (e) {
                      setError(formatInvokeError(e));
                    }
                  })();
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
              {formatUiMessage(status?.tun_unavailable_reason) ||
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
          ) : status?.helper_supported === true ? (
            <FieldDescription>
              {status?.helper_installed
                ? t("settings.helperReady")
                : t("settings.helperNeeded")}
            </FieldDescription>
          ) : (
            <FieldDescription>{t("settings.tunElevationDesc")}</FieldDescription>
          )}
          <HelperActions
            status={status}
            busy={busy}
            onInstall={onInstallHelper}
            onUninstall={onUninstallHelper}
          />
        </div>
      </CardContent>
    </Card>
  );
}
