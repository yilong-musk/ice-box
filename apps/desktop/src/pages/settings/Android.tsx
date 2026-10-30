// SPDX-License-Identifier: GPL-3.0-or-later

import { useState } from "react";
import { api, formatInvokeError, type StatusResponse } from "../../api/client";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { FieldError } from "@/components/ui/field";
import { t } from "../../lib/i18n";

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
    <Card
      size="sm"
      className="w-full shrink-0 data-[size=sm]:[--card-spacing:--spacing(2)]"
    >
      <CardHeader>
        <CardTitle>{t("settings.system")}</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="flex flex-col gap-3">
          {showBattery ? (
            <div className="flex flex-col gap-2">
              <p className="text-xs">{t("settings.batteryTitle")}</p>
              <div>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={busy}
                  onClick={() => void run(() => api.requestBatteryExemption())}
                >
                  {t("settings.batteryAction")}
                </Button>
              </div>
            </div>
          ) : null}
          {showPrivateDns ? (
            <div className="flex flex-col gap-2">
              <p className="text-xs">{t("settings.privateDnsTitle")}</p>
              <div>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={busy}
                  onClick={() => void run(() => api.openNetworkSettings())}
                >
                  {t("settings.privateDnsAction")}
                </Button>
              </div>
            </div>
          ) : null}
          <div className="flex flex-col gap-2">
            <p className="text-xs">{t("settings.alwaysOnTitle")}</p>
            {alwaysOn ? (
              <p className="text-xs text-muted-foreground">
                {t("settings.alwaysOnEnabled")}
              </p>
            ) : null}
            {alwaysOn ? null : (
              <div>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={busy}
                  onClick={() => void run(() => api.openVpnSettings())}
                >
                  {t("settings.alwaysOnAction")}
                </Button>
              </div>
            )}
          </div>
          {error ? <FieldError>{error}</FieldError> : null}
        </div>
      </CardContent>
    </Card>
  );
}
