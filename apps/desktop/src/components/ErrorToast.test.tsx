// SPDX-License-Identifier: GPL-3.0-or-later

import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ErrorToast } from "./ErrorToast";

describe("ErrorToast", () => {
  it("renders nothing without a toast", () => {
    const { container } = render(
      <ErrorToast toast={null} onDismiss={() => {}} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("dismisses on click and keyboard", () => {
    const onDismiss = vi.fn();
    const { getByTestId } = render(
      <ErrorToast toast={{ id: 1, message: "boom" }} onDismiss={onDismiss} />,
    );
    const toast = getByTestId("error-toast");
    expect(toast).toHaveTextContent("boom");

    fireEvent.click(toast);
    fireEvent.keyDown(toast, { key: "Enter" });
    fireEvent.keyDown(toast, { key: " " });
    fireEvent.keyDown(toast, { key: "Escape" });
    expect(onDismiss).toHaveBeenCalledTimes(4);
  });
});
