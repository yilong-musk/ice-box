// SPDX-License-Identifier: GPL-3.0-or-later

import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { t } from "@/lib/i18n";
import { WindowControls } from "./WindowControls";
import type { WindowChrome } from "@platform/windowChrome";

const runWindowCommand = vi.fn();
let chrome: WindowChrome = "macos-overlay";
let followFocus = true;

const shellEvents = vi.hoisted(() => ({
  hidden: new Set<() => void>(),
  shown: new Set<() => void>(),
}));

vi.mock("@platform/windowChrome", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@platform/windowChrome")>();
  return {
    ...actual,
    detectWindowChrome: () => chrome,
    controlsFollowWindowFocus: () => followFocus,
    runWindowCommand: (...args: unknown[]) => runWindowCommand(...args),
  };
});

vi.mock("@/api/client", () => ({
  api: {
    listenWindowHidden: (handler: () => void) => {
      shellEvents.hidden.add(handler);
      return Promise.resolve(() => {
        shellEvents.hidden.delete(handler);
      });
    },
    listenWindowShown: (handler: () => void) => {
      shellEvents.shown.add(handler);
      return Promise.resolve(() => {
        shellEvents.shown.delete(handler);
      });
    },
  },
}));

function emit(handlers: Set<() => void>) {
  for (const handler of handlers) handler();
}

/** The lights' group: hovering any light shows the glyphs on all three. */
function hoverGroup(button: HTMLElement) {
  fireEvent.pointerEnter(button.parentElement!);
}

function expectHovered(hovered: boolean) {
  const controls = screen.getByTestId("window-controls");
  if (hovered) expect(controls).toHaveAttribute("data-hovered", "true");
  else expect(controls).not.toHaveAttribute("data-hovered");
}

describe("WindowControls", () => {
  afterEach(() => {
    chrome = "macos-overlay";
    followFocus = true;
    runWindowCommand.mockReset();
    shellEvents.hidden.clear();
    shellEvents.shown.clear();
  });

  it("does not render caption buttons on macOS overlay chrome", () => {
    chrome = "macos-overlay";
    const { container } = render(<WindowControls />);
    expect(container).toBeEmptyDOMElement();
  });

  it("draws Windows buttons as traffic lights in macOS order and forwards clicks", () => {
    chrome = "windows-custom";
    render(<WindowControls />);

    const close = screen.getByRole("button", { name: t("window.close") });
    const minimize = screen.getByRole("button", { name: t("window.minimize") });
    const maximize = screen.getByRole("button", { name: t("window.maximize") });
    expect(close).toHaveAttribute("data-tone", "close");
    expect(minimize).toHaveAttribute("data-tone", "minimize");
    expect(maximize).toHaveAttribute("data-tone", "zoom");
    expect(close.compareDocumentPosition(minimize)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(minimize.compareDocumentPosition(maximize)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );

    fireEvent.click(minimize);
    fireEvent.click(maximize);
    fireEvent.click(close);

    expect(runWindowCommand).toHaveBeenCalledWith("minimize");
    expect(runWindowCommand).toHaveBeenCalledWith("toggleMaximize");
    expect(runWindowCommand).toHaveBeenCalledWith("close");
  });

  it("shows the glyphs on every light while the group is hovered", () => {
    chrome = "windows-custom";
    render(<WindowControls />);
    const close = screen.getByRole("button", { name: t("window.close") });
    const glyphs = () =>
      screen
        .getByTestId("window-controls")
        .querySelectorAll("svg.opacity-100");

    expect(glyphs()).toHaveLength(0);
    hoverGroup(close);
    expect(glyphs()).toHaveLength(3);
    fireEvent.pointerLeave(close.parentElement!);
    expect(glyphs()).toHaveLength(0);
  });

  it("releases the close light before the window hides", () => {
    chrome = "windows-custom";
    render(<WindowControls />);
    const close = screen.getByRole("button", { name: t("window.close") });

    hoverGroup(close);
    expectHovered(true);

    fireEvent.click(close);
    expectHovered(false);
    expect(runWindowCommand).toHaveBeenCalledWith("close");
  });

  it("clears the hover when the window blurs or hides", async () => {
    chrome = "windows-custom";
    render(<WindowControls />);
    await act(async () => {});

    const close = screen.getByRole("button", { name: t("window.close") });
    hoverGroup(close);
    expectHovered(true);

    fireEvent.blur(window);
    expectHovered(false);

    hoverGroup(close);
    expectHovered(true);
    act(() => emit(shellEvents.hidden));
    expectHovered(false);

    hoverGroup(close);
    expectHovered(true);
    act(() => emit(shellEvents.shown));
    expectHovered(false);
    // A move while the pointer is already treated as inside must not show
    // the hover again. A later enter still does.
    fireEvent.pointerMove(close);
    expectHovered(false);
    hoverGroup(close);
    expectHovered(true);
  });

  it("clears the hover when the document becomes hidden", () => {
    chrome = "windows-custom";
    render(<WindowControls />);
    const close = screen.getByRole("button", { name: t("window.close") });
    hoverGroup(close);
    expectHovered(true);

    const visibility = vi
      .spyOn(document, "visibilityState", "get")
      .mockReturnValue("hidden");
    try {
      act(() => {
        document.dispatchEvent(new Event("visibilitychange"));
      });
      expectHovered(false);
    } finally {
      visibility.mockRestore();
    }
  });

  it("greys the lights while the window is not focused", () => {
    chrome = "windows-custom";
    render(<WindowControls />);
    const close = screen.getByRole("button", { name: t("window.close") });
    const light = () => close.firstElementChild as HTMLElement;

    act(() => {
      fireEvent.focus(window);
    });
    expect(light()).toHaveClass("bg-[#ff5f57]");
    act(() => {
      fireEvent.blur(window);
    });
    expect(light()).toHaveClass("bg-foreground/15");
    expect(light()).not.toHaveClass("bg-[#ff5f57]");
  });

  it("keeps the lights coloured where the platform does not follow window focus", () => {
    // The website's Live Demo: its iframe has focus only after a click into
    // it, so the lights would sit grey while the visitor reads the page.
    chrome = "windows-custom";
    followFocus = false;
    render(<WindowControls />);
    const close = screen.getByRole("button", { name: t("window.close") });
    act(() => {
      fireEvent.blur(window);
    });
    expect(close.firstElementChild).toHaveClass("bg-[#ff5f57]");
  });
});
