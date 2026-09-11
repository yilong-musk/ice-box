// SPDX-License-Identifier: GPL-3.0-or-later

import { fireEvent, render, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { t } from "../../lib/i18n";
import { TrayCard } from "./Tray";

describe("TrayCard", () => {
  it("marks the stored mode and reports a pick", () => {
    const onChange = vi.fn();
    const { container } = render(
      <TrayCard mode="icon_and_speed" busy={false} loaded onChange={onChange} />,
    );
    const group = within(container).getByLabelText(t("settings.tray"));
    expect(
      within(group).getByRole("radio", { name: t("settings.trayIconAndSpeed") }),
    ).toHaveAttribute("data-state", "on");

    fireEvent.click(
      within(group).getByRole("radio", { name: t("settings.traySpeedOnly") }),
    );
    expect(onChange).toHaveBeenCalledWith("speed");
  });

  it("ignores picks before settings load and while a save runs", () => {
    const onChange = vi.fn();
    const { container, rerender } = render(
      <TrayCard mode="icon" busy={false} loaded={false} onChange={onChange} />,
    );
    const group = () => within(container).getByLabelText(t("settings.tray"));

    fireEvent.click(
      within(group()).getByRole("radio", {
        name: t("settings.trayIconAndSpeed"),
      }),
    );
    expect(onChange).not.toHaveBeenCalled();

    rerender(<TrayCard mode="icon" busy loaded onChange={onChange} />);
    fireEvent.click(
      within(group()).getByRole("radio", { name: t("settings.traySpeedOnly") }),
    );
    expect(onChange).not.toHaveBeenCalled();
  });
});
