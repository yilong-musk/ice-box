// SPDX-License-Identifier: GPL-3.0-or-later

import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { CoreStatus, StatusResponse } from "@/api/contracts";
import { TooltipProvider } from "@/components/ui/tooltip";
import { t } from "@/lib/i18n";
import { logoCore, StatusLogo } from "./StatusLogo";

const phoneShell = vi.hoisted(() => ({ value: false }));

vi.mock("@platform/shell", () => ({
  isPhoneShell: () => phoneShell.value,
}));

/** Only the field the logo reads; the rest of the response is irrelevant. */
function status(core: CoreStatus): StatusResponse {
  return {
    core: { status: core, message: null, inbound_host: null, inbound_port: null },
  } as unknown as StatusResponse;
}

describe("StatusLogo", () => {
  it("is neutral before the first status arrives", () => {
    expect(logoCore(null)).toBe("unknown");
  });

  it.each(["stopped", "starting", "running", "stopping", "error"] as const)(
    "reflects the %s core state",
    (core) => {
      render(
        <TooltipProvider>
          <StatusLogo src="logo.png" status={status(core)} />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("status-logo")).toHaveAttribute("data-core", core);
      expect(screen.getByRole("button")).toHaveAccessibleName(
        t("app.logo.core", { state: t(`home.coreStatus.${core}`) }),
      );
    },
  );

  it("is a button so the label can open without a pointer", () => {
    render(
      <TooltipProvider>
        <StatusLogo src="logo.png" status={status("running")} />
      </TooltipProvider>,
    );
    const logo = screen.getByTestId("status-logo");
    expect(logo.tagName).toBe("BUTTON");
    expect(logo).toHaveAttribute("type", "button");
  });

  it("breathes only while the app is in the foreground", () => {
    const hasFocus = vi.spyOn(document, "hasFocus").mockReturnValue(true);
    try {
      render(
        <TooltipProvider>
          <StatusLogo src="logo.png" status={status("running")} />
        </TooltipProvider>,
      );
      const logo = screen.getByTestId("status-logo");
      expect(logo).toHaveAttribute("data-active");

      hasFocus.mockReturnValue(false);
      act(() => {
        window.dispatchEvent(new Event("blur"));
      });
      expect(logo).not.toHaveAttribute("data-active");

      hasFocus.mockReturnValue(true);
      act(() => {
        window.dispatchEvent(new Event("focus"));
      });
      expect(logo).toHaveAttribute("data-active");
    } finally {
      hasFocus.mockRestore();
    }
  });

  describe("on the phone", () => {
    afterEach(() => {
      phoneShell.value = false;
      vi.useRealTimers();
    });

    function renderPhone() {
      phoneShell.value = true;
      render(
        <TooltipProvider>
          <StatusLogo src="logo.png" status={status("running")} />
        </TooltipProvider>,
      );
      return t("app.logo.core", { state: t("home.coreStatus.running") });
    }

    it("toggles the label on tap", () => {
      const label = renderPhone();
      const logo = screen.getByTestId("status-logo");
      expect(screen.queryByRole("tooltip")).toBeNull();

      fireEvent.click(logo);
      expect(screen.getByRole("tooltip")).toHaveTextContent(label);

      fireEvent.click(logo);
      expect(screen.queryByRole("tooltip")).toBeNull();
    });

    // Pointer leave, outside presses and Escape all reach the logo as one
    // Radix close request; jsdom cannot drive the pointer-leave path (no
    // pointer coordinates), so the other two stand in for it. The mouse-away
    // case was checked in a real browser.
    it("ignores Radix close requests while the tapped label is up", () => {
      renderPhone();
      fireEvent.click(screen.getByTestId("status-logo"));
      fireEvent.pointerDown(document.body);
      fireEvent.keyDown(document.body, { key: "Escape" });
      expect(screen.getByRole("tooltip")).toBeInTheDocument();
    });

    it("hides the tapped label after a few seconds", () => {
      vi.useFakeTimers();
      renderPhone();
      fireEvent.click(screen.getByTestId("status-logo"));
      expect(screen.getByRole("tooltip")).toBeInTheDocument();

      act(() => {
        vi.advanceTimersByTime(3000);
      });
      expect(screen.queryByRole("tooltip")).toBeNull();
    });
  });
});
