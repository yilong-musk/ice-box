// SPDX-License-Identifier: GPL-3.0-or-later

import {
  Field,
  FieldDescription,
  FieldLabel,
} from "@/components/ui/field";
import {
  NativeSelect,
  NativeSelectOption,
} from "@/components/ui/native-select";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import {
  t,
  type LanguagePreference,
  type MessageKey,
} from "../../lib/i18n";
import type { TrayDisplayMode } from "../../api/tauri";
import type { ThemePreference } from "../../lib/theme";

const APPEARANCE_OPTIONS = [
  ["system", "settings.appearance.system"],
  ["light", "settings.appearance.light"],
  ["dark", "settings.appearance.dark"],
] as const satisfies ReadonlyArray<readonly [ThemePreference, MessageKey]>;

const LANGUAGE_OPTIONS = [
  ["system", "settings.language.system"],
  ["zh", "settings.language.zh"],
  ["en", "settings.language.en"],
] as const satisfies ReadonlyArray<readonly [LanguagePreference, MessageKey]>;

/** macOS only: what the menu-bar item draws. The Rust tray watchdog reads the
 * persisted value every second, so a pick lands on the bar without further IPC. */
const TRAY_MODES = [
  ["icon_and_speed", "settings.trayIconAndSpeed"],
  ["icon", "settings.trayIconOnly"],
  ["speed", "settings.traySpeedOnly"],
] as const satisfies ReadonlyArray<readonly [TrayDisplayMode, MessageKey]>;

export function AppearanceCard({
  themePreference,
  setThemePreference,
  language,
  trayMode,
  busy,
  loaded,
  onLanguageChange,
  onTrayModeChange,
}: {
  themePreference: ThemePreference;
  setThemePreference: (value: ThemePreference) => void;
  language: LanguagePreference;
  /** macOS only; `null` hides the row (`settings.tray` is not a Windows concept). */
  trayMode: TrayDisplayMode | null;
  busy: boolean;
  loaded: boolean;
  onLanguageChange: (value: LanguagePreference) => void;
  onTrayModeChange: (value: TrayDisplayMode) => void;
}) {
  return (
    <Card
      size="sm"
      className="w-full shrink-0 data-[size=sm]:[--card-spacing:--spacing(2)]"
    >
      <CardHeader>
        <CardTitle>{t("settings.appearance")}</CardTitle>
        <CardDescription>{t("settings.appearanceDesc")}</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="flex flex-col gap-3">
          <Field>
            <FieldLabel htmlFor="settings-language">
              {t("settings.language")}
            </FieldLabel>
            <NativeSelect
              id="settings-language"
              aria-label={t("settings.language")}
              size="sm"
              className="w-full max-w-60"
              value={language}
              disabled={busy || !loaded}
              onChange={(e) => {
                const value = e.target.value;
                if (value === "system" || value === "zh" || value === "en") {
                  onLanguageChange(value);
                }
              }}
            >
              {LANGUAGE_OPTIONS.map(([value, labelKey]) => (
                <NativeSelectOption key={value} value={value}>
                  {t(labelKey)}
                </NativeSelectOption>
              ))}
            </NativeSelect>
          </Field>
          <Field>
            <FieldLabel>{t("settings.theme")}</FieldLabel>
            <ToggleGroup
              type="single"
              variant="outline"
              size="sm"
              spacing={2}
              value={themePreference}
              onValueChange={(value) => {
                if (value === "system" || value === "light" || value === "dark") {
                  setThemePreference(value);
                }
              }}
              className="w-full"
              aria-label={t("settings.theme")}
            >
              {APPEARANCE_OPTIONS.map(([value, labelKey]) => (
                <ToggleGroupItem key={value} value={value} className="flex-1">
                  {t(labelKey)}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </Field>
          {trayMode !== null && (
            <Field>
              <FieldLabel>{t("settings.tray")}</FieldLabel>
              <ToggleGroup
                type="single"
                variant="outline"
                size="sm"
                spacing={2}
                value={trayMode}
                disabled={busy || !loaded}
                onValueChange={(value) => {
                  if (
                    value === "icon_and_speed" ||
                    value === "icon" ||
                    value === "speed"
                  ) {
                    onTrayModeChange(value);
                  }
                }}
                className="w-full"
                aria-label={t("settings.tray")}
              >
                {TRAY_MODES.map(([value, labelKey]) => (
                  <ToggleGroupItem key={value} value={value} className="flex-1">
                    {t(labelKey)}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
              <FieldDescription>{t("settings.trayDesc")}</FieldDescription>
            </Field>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
