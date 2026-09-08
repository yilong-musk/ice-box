// SPDX-License-Identifier: GPL-3.0-or-later

import { Button } from "@/components/ui/button";
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
import { t } from "../../lib/i18n";
import { formatInvokeError, api, type AppSettings } from "../../api/tauri";

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
    <Card
      size="sm"
      className="w-full shrink-0 data-[size=sm]:[--card-spacing:--spacing(2)]"
    >
      <CardHeader>
        <CardTitle>{t("settings.data")}</CardTitle>
        <CardDescription>{t("settings.dataDesc")}</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="flex flex-col gap-3">
          <Field orientation="horizontal" className="w-auto gap-2">
            <Switch
              id="settings-log-debug"
              size="sm"
              checked={form.log_debug}
              disabled={busy || !loaded}
              aria-label={t("settings.logDebug")}
              onCheckedChange={(checked) => {
                setForm({
                  ...form,
                  log_debug: checked === true,
                });
              }}
            />
            <FieldLabel htmlFor="settings-log-debug">
              {t("settings.logDebug")}
            </FieldLabel>
          </Field>
          <FieldDescription>{t("settings.logDebugDesc")}</FieldDescription>
          <div className="flex flex-wrap gap-2">
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
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
