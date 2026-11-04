// SPDX-License-Identifier: GPL-3.0-or-later

import { Switch } from "@/components/ui/switch";
import { t } from "../../lib/i18n";
import { SettingsRow, SettingsSection } from "./SettingsLayout";

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
    <SettingsSection title={t("settings.startup")}>
      <SettingsRow
        label={t("settings.launchAtLogin")}
        htmlFor="settings-launch-at-login"
      >
        <Switch
          id="settings-launch-at-login"
          checked={enabled}
          disabled={busy || !loaded}
          aria-label={t("settings.launchAtLogin")}
          onCheckedChange={(checked) => onChange(checked === true)}
        />
      </SettingsRow>
    </SettingsSection>
  );
}
