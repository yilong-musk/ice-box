// SPDX-License-Identifier: GPL-3.0-or-later

import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ChartContainer, type ChartConfig } from "./chart";

describe("ChartContainer", () => {
  it("keeps its color <style> in place across re-renders", () => {
    const config: ChartConfig = { down: { label: "Down", color: "var(--ok)" } };
    const chart = (n: number) => (
      <ChartContainer config={config} data-n={n}>
        <svg />
      </ChartContainer>
    );
    const { container, rerender } = render(chart(1));
    const style = container.querySelector("style")!;
    expect(style.textContent).toContain("--color-down: var(--ok);");
    const text = style.firstChild;

    // Each sample re-renders the chart. Rewriting the <style> would restyle
    // the whole document every time.
    rerender(chart(2));
    expect(container.querySelector("style")).toBe(style);
    expect(style.firstChild).toBe(text);

    rerender(
      <ChartContainer config={{ down: { label: "Down", color: "red" } }}>
        <svg />
      </ChartContainer>,
    );
    expect(style.textContent).toContain("--color-down: red;");
  });
});
