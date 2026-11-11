// SPDX-License-Identifier: GPL-3.0-or-later

import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ProxyMode } from "../api/contracts";
import { t } from "../lib/i18n";
import { ProxyModePicker } from "./ProxyModePicker";

describe("ProxyModePicker", () => {
  it.each(["segmented", "list"] as const)("%s variant switches modes", (variant) => {
    const onChange = vi.fn();
    const view = render(<ProxyModePicker variant={variant} value="rule" onChange={onChange} />);
    expect(view.getByRole("radio", { name: new RegExp(t("home.mode.rule")) })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    fireEvent.click(view.getByRole("radio", { name: new RegExp(t("home.mode.global")) }));
    expect(onChange).toHaveBeenCalledWith("global");
  });

  it("shows the hint for the selected mode on the phone", () => {
    const view = render(<ProxyModePicker variant="segmented" value="direct" onChange={() => {}} />);
    expect(view.getByTestId("home-mode-hint")).toHaveTextContent(t("home.modeHint.direct"));
  });

  it("selects nothing for a mode it does not know instead of crashing", () => {
    const view = render(
      <ProxyModePicker variant="segmented" value={"Rule" as ProxyMode} onChange={() => {}} />,
    );
    expect(view.queryByTestId("home-mode-hint")).toBeNull();
    for (const radio of view.getAllByRole("radio")) {
      expect(radio).toHaveAttribute("aria-checked", "false");
    }
  });
});
