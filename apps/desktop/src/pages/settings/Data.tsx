// SPDX-License-Identifier: GPL-3.0-or-later

import { Button } from "@/components/ui/button";
import { t } from "../../lib/i18n";
import { formatInvokeError, api } from "../../api/client";
import { SettingsRow, SettingsSection } from "./SettingsLayout";

export function DataCard({
  busy,
  loaded,
  setError,
}: {
  busy: boolean;
  loaded: boolean;
  setError: (error: string | null) => void;
}) {
  return (
    <SettingsSection title={t("settings.data")}>
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
