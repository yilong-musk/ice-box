// SPDX-License-Identifier: GPL-3.0-or-later

import { useState } from "react";
import { api, formatInvokeError } from "../../api/client";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { FieldError } from "@/components/ui/field";
import { t } from "../../lib/i18n";

/** Phone only: the phone has no Logs page, so the core log (warnings and
 * errors, capped at 5 MiB by the tunnel process) is exported as a file for a
 * bug report instead. */
export function PhoneLogsCard() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  async function exportLog() {
    setBusy(true);
    setError(null);
    setSaved(false);
    try {
      setSaved((await api.exportCoreLog()) === "saved");
    } catch (err) {
      setError(formatInvokeError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card
      size="sm"
      className="w-full shrink-0 data-[size=sm]:[--card-spacing:--spacing(2)]"
    >
      <CardHeader>
        <CardTitle>{t("app.nav.logs")}</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="flex flex-col gap-2">
          <p className="text-xs text-muted-foreground">
            {t("settings.exportLogsHint")}
          </p>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => void exportLog()}
            >
              {t("settings.exportLogs")}
            </Button>
            {saved ? (
              <span className="text-xs text-muted-foreground" role="status">
                {t("settings.exportLogsSaved")}
              </span>
            ) : null}
          </div>
          {error ? <FieldError>{error}</FieldError> : null}
        </div>
      </CardContent>
    </Card>
  );
}
