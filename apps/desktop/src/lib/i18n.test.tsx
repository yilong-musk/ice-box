// SPDX-License-Identifier: GPL-3.0-or-later

import { Activity } from "react";
import { act, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  LANGUAGE_CHANGE_EVENT,
  persistLanguagePreference,
  t,
  useLanguagePreference,
} from "./i18n";

function Label() {
  const { resolved } = useLanguagePreference();
  return <span data-testid={`label-${resolved}`}>{t("nodes.select")}</span>;
}

function Pages({ showSecond }: { showSecond: boolean }) {
  return (
    <>
      <Activity mode={showSecond ? "hidden" : "visible"}>
        <Label />
      </Activity>
      <Activity mode={showSecond ? "visible" : "hidden"}>
        <Label />
      </Activity>
    </>
  );
}

describe("useLanguagePreference", () => {
  it("does not revert the language when a hidden page becomes visible", () => {
    act(() => persistLanguagePreference("zh"));
    const { rerender } = render(<Pages showSecond={false} />);

    // The second page is hidden (its effects are torn down) while the
    // language changes from the visible one.
    act(() => persistLanguagePreference("en"));
    rerender(<Pages showSecond />);

    expect(document.documentElement.lang).toBe("en");
    expect(t("nodes.select")).toBe("Select");
    expect(screen.getAllByTestId("label-en")).toHaveLength(2);
    expect(screen.queryByTestId("label-zh")).toBeNull();
  });

  it("follows a language change event even while every page is hidden", () => {
    act(() => persistLanguagePreference("zh"));
    const { rerender } = render(
      <Activity mode="hidden">
        <Label />
      </Activity>,
    );

    act(() => {
      window.dispatchEvent(
        new CustomEvent(LANGUAGE_CHANGE_EVENT, { detail: "en" }),
      );
    });
    rerender(
      <Activity mode="visible">
        <Label />
      </Activity>,
    );

    expect(screen.getByTestId("label-en")).toHaveTextContent("Select");
  });

  it("ignores a missing preference instead of breaking t()", () => {
    act(() => persistLanguagePreference("zh"));
    act(() =>
      persistLanguagePreference(undefined as unknown as "zh"),
    );
    expect(t("nodes.select")).toBe("选用");
  });
});
