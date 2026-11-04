// SPDX-License-Identifier: GPL-3.0-or-later

import { Switch } from "@/components/ui/switch";
import { formatInvokeError, formatUiMessage, api, type AppSettings, type StatusResponse } from "../../api/client";
import { t, type MessageKey } from "../../lib/i18n";
import { HelperActions } from "./Helper";
import { SettingsRow, SettingsSection } from "./SettingsLayout";

/** TUN lifecycle labels shown in the settings card while a transition runs. */
const TUN_TRANSITION_KEYS: Record<string, MessageKey> = {
  preparing: "settings.tunTransition.preparing",
  stopping: "settings.tunTransition.stopping",
};

export function TunCard({
  form,
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
  const transitionKey = TUN_TRANSITION_KEYS[status?.tun_status ?? ""];
  const tunHint = transitionKey
    ? t(transitionKey)
    : status?.tun_status === "recovery_required"
      ? t("settings.tunRecoveryRequired")
      : status?.tun_available === false
        ? formatUiMessage(status?.tun_unavailable_reason) ||
          t("settings.tunNotSupported")
        : status?.traffic_capture === "tun"
          ? t("settings.tunActiveWithIface", {
              interface: status.tun_interface
                ? `（${t("common.withIfaceLabel", {
                    iface: status.tun_interface,
                  })}）`
                : "",
            })
          : status?.helper_stale === true
            ? t("settings.helperStale")
            : status?.helper_supported === true &&
                status?.helper_installed !== true
              ? t("settings.helperNeeded")
              : null;

  return (
    <SettingsSection title={t("settings.tun")}>
      <SettingsRow
        label={t("settings.tunEnable")}
        htmlFor="settings-tun-enabled"
        description={tunHint}
      >
        <Switch
          id="settings-tun-enabled"
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
            void (async () => {
              try {
                await persistTunEnabled(checked === true);
                flashSaved();
              } catch (e) {
                setError(formatInvokeError(e));
              }
            })();
          }}
        />
      </SettingsRow>
      <HelperActions
        status={status}
        busy={busy}
        onInstall={onInstallHelper}
        onUninstall={onUninstallHelper}
      />
    </SettingsSection>
  );
}
