// SPDX-License-Identifier: GPL-3.0-or-later

import { useState } from "react";
import { api, formatInvokeError, type StatusResponse } from "../../api/client";
import { Button } from "@/components/ui/button";
import { t } from "../../lib/i18n";
import { SettingsRow, SettingsSection } from "./SettingsLayout";

type SystemAction = "battery" | "privateDns" | "alwaysOn";

/** System settings the VPN cannot change itself: battery exemption, Private
 * DNS, and always-on. Each row opens the platform screen; none of them is
 * stored in ice-box settings. */
export function AndroidSystemCard({
  status,
}: {
  status: StatusResponse | null;
}) {
  // Each action keeps its own error so a failure shows under the row whose
  // button caused it.
  const [errors, setErrors] = useState<Partial<Record<SystemAction, string>>>(
    {},
  );
  const [busy, setBusy] = useState(false);

  async function run(key: SystemAction, action: () => Promise<void>) {
    setErrors({});
    setBusy(true);
    try {
      await action();
    } catch (err) {
      setErrors({ [key]: formatInvokeError(err) });
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
        <SettingsRow
          label={t("settings.batteryTitle")}
          error={errors.battery}
        >
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() => void run("battery", () => api.requestBatteryExemption())}
          >
            {t("settings.batteryAction")}
          </Button>
        </SettingsRow>
      ) : null}
      {showPrivateDns ? (
        <SettingsRow
          label={t("settings.privateDnsTitle")}
          error={errors.privateDns}
        >
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() => void run("privateDns", () => api.openNetworkSettings())}
          >
            {t("settings.privateDnsAction")}
          </Button>
        </SettingsRow>
      ) : null}
      <SettingsRow
        label={t("settings.alwaysOnTitle")}
        description={alwaysOn ? t("settings.alwaysOnEnabled") : undefined}
        error={errors.alwaysOn}
      >
        {alwaysOn ? null : (
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() => void run("alwaysOn", () => api.openVpnSettings())}
          >
            {t("settings.alwaysOnAction")}
          </Button>
        )}
      </SettingsRow>
    </SettingsSection>
  );
}
