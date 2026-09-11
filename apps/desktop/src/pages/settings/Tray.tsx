// SPDX-License-Identifier: GPL-3.0-or-later

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import type { TrayDisplayMode } from "../../api/tauri";
import { t, type MessageKey } from "../../lib/i18n";

const TRAY_MODES = [
  ["icon_and_speed", "settings.trayIconAndSpeed"],
  ["icon", "settings.trayIconOnly"],
  ["speed", "settings.traySpeedOnly"],
] as const satisfies ReadonlyArray<readonly [TrayDisplayMode, MessageKey]>;

/** macOS only: what the menu-bar item shows. The Rust tray watchdog reads the
 * persisted value every second, so a pick lands on the bar without further IPC. */
export function TrayCard({
  mode,
  busy,
  loaded,
  onChange,
}: {
  mode: TrayDisplayMode;
  busy: boolean;
  loaded: boolean;
  onChange: (mode: TrayDisplayMode) => void;
}) {
  return (
    <Card
      size="sm"
      className="w-full shrink-0 data-[size=sm]:[--card-spacing:--spacing(2)]"
    >
      <CardHeader>
        <CardTitle>{t("settings.tray")}</CardTitle>
        <CardDescription>{t("settings.trayDesc")}</CardDescription>
      </CardHeader>
      <CardContent>
        <ToggleGroup
          type="single"
          variant="outline"
          size="sm"
          spacing={2}
          value={mode}
          disabled={busy || !loaded}
          onValueChange={(value) => {
            if (value === "icon_and_speed" || value === "icon" || value === "speed") {
              onChange(value);
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
      </CardContent>
    </Card>
  );
}
