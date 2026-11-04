// SPDX-License-Identifier: GPL-3.0-or-later

import { useState } from "react";
import { api, formatInvokeError } from "../../api/client";
import { Button } from "@/components/ui/button";
import { t } from "../../lib/i18n";
import { SettingsRow, SettingsSection } from "./SettingsLayout";

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
    <SettingsSection title={t("app.nav.logs")}>
      <SettingsRow
        description={
          <>
            {t("settings.exportLogsHint")}
            {saved ? (
              <span className="mt-0.5 block text-foreground" role="status">
                {t("settings.exportLogsSaved")}
              </span>
            ) : null}
          </>
        }
        error={error}
      >
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={busy}
          onClick={() => void exportLog()}
        >
          {t("settings.exportLogs")}
        </Button>
      </SettingsRow>
    </SettingsSection>
  );
}
