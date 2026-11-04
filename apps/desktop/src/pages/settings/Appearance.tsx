// SPDX-License-Identifier: GPL-3.0-or-later

import {
  NativeSelect,
  NativeSelectOption,
} from "@/components/ui/native-select";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import {
  t,
  type LanguagePreference,
  type MessageKey,
} from "../../lib/i18n";
import type { TrayDisplayMode } from "../../api/client";
import type { ThemePreference } from "../../lib/theme";
import { SettingsRow, SettingsSection } from "./SettingsLayout";

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
    <SettingsSection title={t("settings.appearance")}>
      <SettingsRow label={t("settings.language")} htmlFor="settings-language">
        <NativeSelect
          id="settings-language"
          aria-label={t("settings.language")}
          size="sm"
          className="w-40"
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
      </SettingsRow>
      <SettingsRow label={t("settings.theme")} htmlFor="settings-theme">
        <NativeSelect
          id="settings-theme"
          aria-label={t("settings.theme")}
          size="sm"
          className="w-40"
          value={themePreference}
          onChange={(e) => {
            const value = e.target.value;
            if (value === "system" || value === "light" || value === "dark") {
              setThemePreference(value);
            }
          }}
        >
          {APPEARANCE_OPTIONS.map(([value, labelKey]) => (
            <NativeSelectOption key={value} value={value}>
              {t(labelKey)}
            </NativeSelectOption>
          ))}
        </NativeSelect>
      </SettingsRow>
      {trayMode !== null && (
        <SettingsRow label={t("settings.tray")}>
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            spacing={0}
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
            aria-label={t("settings.tray")}
          >
            {TRAY_MODES.map(([value, labelKey]) => (
              <ToggleGroupItem key={value} value={value} className="px-3">
                {t(labelKey)}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </SettingsRow>
      )}
    </SettingsSection>
  );
}
