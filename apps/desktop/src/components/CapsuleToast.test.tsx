// SPDX-License-Identifier: GPL-3.0-or-later

import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { t } from "../lib/i18n";
import { CapsuleToast } from "./CapsuleToast";

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
