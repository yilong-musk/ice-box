// SPDX-License-Identifier: GPL-3.0-or-later

import { useState } from "react";
import { api, formatInvokeError, type StatusResponse } from "../../api/client";
import { Button } from "@/components/ui/button";
import { t } from "../../lib/i18n";
import { SettingsRow, SettingsSection } from "./SettingsLayout";

/** System settings the VPN cannot change itself: battery exemption, Private
 * DNS, and always-on. Each row opens the platform screen; none of them is
 * stored in ice-box settings. */
export function AndroidSystemCard({
  status,
}: {
  status: StatusResponse | null;
}) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function run(action: () => Promise<void>) {
    setError(null);
    setBusy(true);
    try {
      await action();
    } catch (err) {
      setError(formatInvokeError(err));
    } finally {
      setBusy(false);
    }
  }

  const showBattery = status?.battery_unrestricted === false;
  const showPrivateDns = status?.private_dns_strict === true;
  const alwaysOn = status?.always_on_vpn === true;

  return (
    <SettingsSection title={t("settings.system")}>
      {showBattery ? (
        <SettingsRow label={t("settings.batteryTitle")}>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() => void run(() => api.requestBatteryExemption())}
          >
            {t("settings.batteryAction")}
          </Button>
        </SettingsRow>
      ) : null}
      {showPrivateDns ? (
        <SettingsRow label={t("settings.privateDnsTitle")}>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() => void run(() => api.openNetworkSettings())}
          >
            {t("settings.privateDnsAction")}
          </Button>
        </SettingsRow>
      ) : null}
      <SettingsRow
        label={t("settings.alwaysOnTitle")}
        description={alwaysOn ? t("settings.alwaysOnEnabled") : undefined}
        error={error}
      >
        {alwaysOn ? null : (
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() => void run(() => api.openVpnSettings())}
          >
            {t("settings.alwaysOnAction")}
          </Button>
        )}
      </SettingsRow>
    </SettingsSection>
  );
}
