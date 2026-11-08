// SPDX-License-Identifier: GPL-3.0-or-later

import { act, fireEvent, render } from "@testing-library/react";
import { Activity } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { t } from "../lib/i18n";
import { CapsuleToast, useCapsuleToast } from "./CapsuleToast";

describe("CapsuleToast", () => {
  it("renders no capsule without a toast", () => {
    const { queryByTestId, queryByRole, getByRole } = render(
      <CapsuleToast toast={null} onDismiss={() => {}} />,
    );
    expect(queryByTestId("capsule-toast")).toBeNull();
    expect(queryByTestId("error-toast")).toBeNull();
    expect(queryByTestId("success-toast")).toBeNull();
    expect(queryByRole("alert")).toBeNull();
    expect(getByRole("status")).toBeEmptyDOMElement();
  });

  it("announces an error through an assertive live region", () => {
    const { getByTestId } = render(
      <CapsuleToast
        toast={{ id: 1, message: "boom", tone: "error" }}
        onDismiss={() => {}}
      />,
    );
    const live = getByTestId("error-toast");
    expect(live).toHaveTextContent("boom");
    expect(live).toHaveAttribute("role", "alert");
    expect(live).toHaveAttribute("aria-live", "assertive");
    expect(getByTestId("capsule-toast")).toHaveTextContent("boom");
  });

  it("announces a success through a polite live region", () => {
    const { getByTestId } = render(
      <CapsuleToast
        toast={{ id: 1, message: "updated", tone: "success" }}
        onDismiss={() => {}}
      />,
    );
    const live = getByTestId("success-toast");
    expect(live).toHaveTextContent("updated");
    expect(live).toHaveAttribute("role", "status");
    expect(live).toHaveAttribute("aria-live", "polite");
    expect(getByTestId("capsule-toast")).toHaveTextContent("updated");
  });

  it("is keyboard dismissable and labelled", () => {
    const onDismiss = vi.fn();
    const { getByTestId } = render(
      <CapsuleToast
        toast={{ id: 1, message: "boom", tone: "error" }}
        onDismiss={onDismiss}
      />,
    );
    const toast = getByTestId("capsule-toast");
    expect(toast).toHaveAttribute("role", "button");
    expect(toast).toHaveAttribute("aria-label", t("common.dismiss"));

    fireEvent.click(toast);
    fireEvent.keyDown(toast, { key: "Enter" });
    fireEvent.keyDown(toast, { key: " " });
    fireEvent.keyDown(toast, { key: "Escape" });
    expect(onDismiss).toHaveBeenCalledTimes(4);
  });
});

describe("useCapsuleToast", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  // A page as App mounts it: kept inside <Activity>, which tears down effects
  // while hidden but keeps state. The notice comes from a user action, as on
  // the subscriptions page.
  function Page() {
    const { toast, show, dismiss } = useCapsuleToast();
    return (
      <>
        <button type="button" onClick={() => show("Updated", "success")}>
          update
        </button>
        <CapsuleToast toast={toast} onDismiss={dismiss} />
      </>
    );
  }

  function renderPage() {
    const view = render(
      <Activity mode="visible">
        <Page />
      </Activity>,
    );
    const setVisible = (visible: boolean) =>
      view.rerender(
        <Activity mode={visible ? "visible" : "hidden"}>
          <Page />
        </Activity>,
      );
    fireEvent.click(view.getByRole("button", { name: "update" }));
    return { ...view, setVisible };
  }

  it("goes away on its own", () => {
    vi.useFakeTimers();
    const { queryByTestId } = renderPage();
    expect(queryByTestId("capsule-toast")).not.toBeNull();
    act(() => {
      vi.advanceTimersByTime(2400);
    });
    expect(queryByTestId("capsule-toast")).toBeNull();
  });

  it("still goes away after a tab switch, with only the time it had left", () => {
    vi.useFakeTimers();
    const { queryByTestId, setVisible } = renderPage();
    act(() => {
      vi.advanceTimersByTime(1000);
    });

    // Hidden time does not count, however long.
    setVisible(false);
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    setVisible(true);
    expect(queryByTestId("capsule-toast")).not.toBeNull();

    act(() => {
      vi.advanceTimersByTime(1399);
    });
    expect(queryByTestId("capsule-toast")).not.toBeNull();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(queryByTestId("capsule-toast")).toBeNull();
  });
});
