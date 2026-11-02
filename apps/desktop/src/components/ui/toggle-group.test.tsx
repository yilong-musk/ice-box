// SPDX-License-Identifier: GPL-3.0-or-later

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { ToggleGroup, ToggleGroupItem } from "./toggle-group";

function renderGroup(orientation: "horizontal" | "vertical") {
  render(
    <ToggleGroup
      type="single"
      orientation={orientation}
      value="one"
      aria-label="test group"
    >
      <ToggleGroupItem value="one">One</ToggleGroupItem>
      <ToggleGroupItem value="two">Two</ToggleGroupItem>
    </ToggleGroup>,
  );

  return {
    one: screen.getByRole("radio", { name: "One" }),
    two: screen.getByRole("radio", { name: "Two" }),
  };
}

describe("ToggleGroup", () => {
  it("moves focus with horizontal arrow keys when horizontal", async () => {
    const user = userEvent.setup();
    const { one, two } = renderGroup("horizontal");

    one.focus();
    await user.keyboard("{ArrowRight}");
    expect(two).toHaveFocus();

    await user.keyboard("{ArrowLeft}");
    expect(one).toHaveFocus();

    await user.keyboard("{ArrowDown}");
    expect(one).toHaveFocus();
  });

  it("moves focus with vertical arrow keys when vertical", async () => {
    const user = userEvent.setup();
    const { one, two } = renderGroup("vertical");

    one.focus();
    await user.keyboard("{ArrowDown}");
    expect(two).toHaveFocus();

    await user.keyboard("{ArrowUp}");
    expect(one).toHaveFocus();

    await user.keyboard("{ArrowRight}");
    expect(one).toHaveFocus();
  });
});
