// SPDX-License-Identifier: GPL-3.0-or-later

import {
  Field,
  FieldLabel,
} from "@/components/ui/field";
import {
  NativeSelect,
  NativeSelectOption,
} from "@/components/ui/native-select";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import {
  t,
  type LanguagePreference,
  type MessageKey,
} from "../../lib/i18n";
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

export function AppearanceCard({
  themePreference,
  setThemePreference,
  language,
  busy,
  loaded,
  onLanguageChange,
}: {
  themePreference: ThemePreference;
  setThemePreference: (value: ThemePreference) => void;
  language: LanguagePreference;
  busy: boolean;
  loaded: boolean;
  onLanguageChange: (value: LanguagePreference) => void;
}) {
  return (
    <Card
      size="sm"
      className="w-full shrink-0 data-[size=sm]:[--card-spacing:--spacing(2)]"
    >
      <CardContent>
        <div className="flex flex-col gap-3">
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
        </div>
      </CardContent>
    </Card>
  );
}
