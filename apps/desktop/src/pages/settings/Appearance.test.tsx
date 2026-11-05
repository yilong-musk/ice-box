// SPDX-License-Identifier: GPL-3.0-or-later

import type { ComponentProps } from "react";
import { fireEvent, render, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { t } from "../../lib/i18n";
import { AppearanceCard } from "./Appearance";

/** Every test starts from a loaded macOS card; `overrides` narrows it. */
function renderCard(
  overrides: Partial<ComponentProps<typeof AppearanceCard>> = {},
) {
  const setThemePreference = vi.fn();
  const onLanguageChange = vi.fn();
  const onTrayModeChange = vi.fn();
  const view = render(
    <AppearanceCard
      themePreference="system"
      setThemePreference={setThemePreference}
      language="system"
      trayMode="icon_and_speed"
      busy={false}
      loaded
      onLanguageChange={onLanguageChange}
      onTrayModeChange={onTrayModeChange}
      {...overrides}
    />,
  );
  return { ...view, setThemePreference, onLanguageChange, onTrayModeChange };
}

describe("AppearanceCard", () => {
  it("reports theme, language and menu-bar picks", () => {
    const { container, setThemePreference, onLanguageChange, onTrayModeChange } =
      renderCard();

    fireEvent.change(within(container).getByLabelText(t("settings.theme")), {
      target: { value: "dark" },
    });
    expect(setThemePreference).toHaveBeenCalledWith("dark");

    fireEvent.change(within(container).getByLabelText(t("settings.language")), {
      target: { value: "en" },
    });
    expect(onLanguageChange).toHaveBeenCalledWith("en");

    const tray = within(container).getByLabelText(t("settings.tray"));
    expect(tray).toHaveValue("icon_and_speed");
    fireEvent.change(tray, { target: { value: "speed" } });
    expect(onTrayModeChange).toHaveBeenCalledWith("speed");
  });

  it("hides the menu-bar row where there is no menu bar", () => {
    const { container } = renderCard({ trayMode: null });
    expect(within(container).queryByLabelText(t("settings.tray"))).toBeNull();
  });

  it("orders language first, then theme, then the menu bar item", () => {
    const labels = (container: HTMLElement) =>
      Array.from(
        container.querySelectorAll(
          "[data-slot='field-label'], [data-slot='settings-label']",
        ),
      ).map((node) => node.textContent);

    expect(labels(renderCard().container)).toEqual([
      t("settings.language"),
      t("settings.theme"),
      t("settings.tray"),
    ]);
    expect(labels(renderCard({ trayMode: null }).container)).toEqual([
      t("settings.language"),
      t("settings.theme"),
    ]);
  });

  it("locks the menu-bar picker before settings load and while a save runs", () => {
    const { container, rerender, onTrayModeChange } = renderCard();
    const tray = () => within(container).getByLabelText(t("settings.tray"));
    expect(tray()).toBeEnabled();

    rerender(
      <AppearanceCard
        themePreference="system"
        setThemePreference={vi.fn()}
        language="system"
        trayMode="icon"
        busy={false}
        loaded={false}
        onLanguageChange={vi.fn()}
        onTrayModeChange={onTrayModeChange}
      />,
    );
    expect(tray()).toBeDisabled();
    expect(tray()).toHaveValue("icon");

    rerender(
      <AppearanceCard
        themePreference="system"
        setThemePreference={vi.fn()}
        language="system"
        trayMode="icon"
        busy
        loaded
        onLanguageChange={vi.fn()}
        onTrayModeChange={onTrayModeChange}
      />,
    );
    expect(tray()).toBeDisabled();
    expect(tray()).toHaveValue("icon");
  });
});
