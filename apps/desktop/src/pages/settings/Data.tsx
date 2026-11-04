// SPDX-License-Identifier: GPL-3.0-or-later

import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { t } from "../../lib/i18n";
import { formatInvokeError, api, type AppSettings } from "../../api/client";
import { SettingsRow, SettingsSection } from "./SettingsLayout";

export function DataCard({
  form,
  setForm,
  busy,
  loaded,
  setError,
}: {
  form: AppSettings;
  setForm: (next: AppSettings) => void;
  busy: boolean;
  loaded: boolean;
  setError: (error: string | null) => void;
}) {
  return (
    <SettingsSection title={t("settings.data")}>
      <SettingsRow label={t("settings.logDebug")} htmlFor="settings-log-debug">
        <Switch
          id="settings-log-debug"
          checked={form.log_debug}
          disabled={busy || !loaded}
          aria-label={t("settings.logDebug")}
          onCheckedChange={(checked) => {
            setForm({ ...form, log_debug: checked === true });
          }}
        />
      </SettingsRow>
      <SettingsRow label={t("settings.dataDir")}>
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
      </SettingsRow>
    </SettingsSection>
  );
}
