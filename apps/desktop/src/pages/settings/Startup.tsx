// SPDX-License-Identifier: GPL-3.0-or-later

import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field";
import { Switch } from "@/components/ui/switch";
import { t } from "../../lib/i18n";

/** Login item. The shell owns the OS registration (macOS LaunchAgent plist /
 * Windows per-user `Run` value), so the card only reports the persisted
 * `launch_at_login` value: the page persists the pick and rolls the switch
 * back when the OS write is refused. */
export function StartupCard({
  enabled,
  busy,
  loaded,
  onChange,
}: {
  enabled: boolean;
  busy: boolean;
  loaded: boolean;
  onChange: (enabled: boolean) => void;
}) {
  return (
    <Card
      size="sm"
      className="w-full shrink-0 data-[size=sm]:[--card-spacing:--spacing(2)]"
    >
      <CardHeader>
        <CardTitle>{t("settings.startup")}</CardTitle>
        <CardDescription>{t("settings.startupDesc")}</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="flex flex-col gap-3">
          <Field orientation="horizontal" className="w-auto gap-2">
            <Switch
              id="settings-launch-at-login"
              size="sm"
              checked={enabled}
              disabled={busy || !loaded}
              aria-label={t("settings.launchAtLogin")}
              onCheckedChange={(checked) => onChange(checked === true)}
            />
            <FieldLabel htmlFor="settings-launch-at-login">
              {t("settings.launchAtLogin")}
            </FieldLabel>
          </Field>
          <FieldDescription>{t("settings.launchAtLoginDesc")}</FieldDescription>
        </div>
      </CardContent>
    </Card>
  );
}
