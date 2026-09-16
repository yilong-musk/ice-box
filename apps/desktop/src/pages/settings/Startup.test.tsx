// SPDX-License-Identifier: GPL-3.0-or-later

import { fireEvent, render, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { t } from "../../lib/i18n";
import { StartupCard } from "./Startup";

describe("StartupCard", () => {
  it("reports the pick for the login item", () => {
    const onChange = vi.fn();
    const { container } = render(
      <StartupCard enabled={false} busy={false} loaded onChange={onChange} />,
    );
    const card = within(container);
    const toggle = card.getByRole("switch", { name: t("settings.launchAtLogin") });
    expect(toggle).toHaveAttribute("data-state", "unchecked");

    fireEvent.click(toggle);
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("marks an enabled login item and ignores picks while busy or unloaded", () => {
    const onChange = vi.fn();
    const { container, rerender } = render(
      <StartupCard enabled busy={false} loaded onChange={onChange} />,
    );
    const toggle = () =>
      within(container).getByRole("switch", { name: t("settings.launchAtLogin") });
    expect(toggle()).toHaveAttribute("data-state", "checked");

    rerender(<StartupCard enabled busy loaded onChange={onChange} />);
    fireEvent.click(toggle());
    expect(onChange).not.toHaveBeenCalled();

    rerender(<StartupCard enabled busy={false} loaded={false} onChange={onChange} />);
    fireEvent.click(toggle());
    expect(onChange).not.toHaveBeenCalled();
  });
});
