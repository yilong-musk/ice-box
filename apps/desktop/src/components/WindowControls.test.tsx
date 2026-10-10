// SPDX-License-Identifier: GPL-3.0-or-later

import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { t } from "@/lib/i18n";
import { WindowControls } from "./WindowControls";
import type { WindowChrome } from "@platform/windowChrome";

const runWindowCommand = vi.fn();
let chrome: WindowChrome = "macos-overlay";

const shellEvents = vi.hoisted(() => ({
  hidden: new Set<() => void>(),
  shown: new Set<() => void>(),
}));

vi.mock("@platform/windowChrome", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@platform/windowChrome")>();
  return {
    ...actual,
    detectWindowChrome: () => chrome,
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

describe("WindowControls", () => {
  afterEach(() => {
    chrome = "macos-overlay";
    runWindowCommand.mockReset();
    shellEvents.hidden.clear();
    shellEvents.shown.clear();
  });

  it("does not render caption buttons on macOS overlay chrome", () => {
    chrome = "macos-overlay";
    const { container } = render(<WindowControls />);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders Windows caption buttons and forwards clicks", () => {
    chrome = "windows-custom";
    render(<WindowControls />);

    fireEvent.click(screen.getByRole("button", { name: t("window.minimize") }));
    fireEvent.click(screen.getByRole("button", { name: t("window.maximize") }));
    fireEvent.click(screen.getByRole("button", { name: t("window.close") }));

    expect(runWindowCommand).toHaveBeenCalledWith("minimize");
    expect(runWindowCommand).toHaveBeenCalledWith("toggleMaximize");
    expect(runWindowCommand).toHaveBeenCalledWith("close");
  });

  it("releases the close button before the window hides", () => {
    chrome = "windows-custom";
    render(<WindowControls />);
    const close = screen.getByRole("button", { name: t("window.close") });

    fireEvent.pointerEnter(close);
    expect(close).toHaveClass("bg-destructive");

    fireEvent.click(close);
    expect(close).not.toHaveClass("bg-destructive");
    expect(runWindowCommand).toHaveBeenCalledWith("close");
  });

  it("clears caption hover when the window blurs or hides", async () => {
    chrome = "windows-custom";
    render(<WindowControls />);
    await act(async () => {});

    const close = screen.getByRole("button", { name: t("window.close") });
    const minimize = screen.getByRole("button", { name: t("window.minimize") });
    fireEvent.pointerEnter(close);
    fireEvent.pointerEnter(minimize);
    expect(close).toHaveClass("bg-destructive");
    expect(minimize).toHaveClass("bg-muted");

    fireEvent.blur(window);
    expect(close).not.toHaveClass("bg-destructive");
    expect(minimize).not.toHaveClass("bg-muted");

    fireEvent.pointerEnter(close);
    expect(close).toHaveClass("bg-destructive");
    act(() => emit(shellEvents.hidden));
    expect(close).not.toHaveClass("bg-destructive");

    fireEvent.pointerEnter(close);
    expect(close).toHaveClass("bg-destructive");
    act(() => emit(shellEvents.shown));
    expect(close).not.toHaveClass("bg-destructive");
    // A move while the pointer is already treated as inside must not paint
    // the button pressed again. A later enter still does.
    fireEvent.pointerMove(close);
    expect(close).not.toHaveClass("bg-destructive");
    fireEvent.pointerEnter(close);
    expect(close).toHaveClass("bg-destructive");
  });

  it("clears caption hover when the document becomes hidden", () => {
    chrome = "windows-custom";
    render(<WindowControls />);
    const close = screen.getByRole("button", { name: t("window.close") });
    fireEvent.pointerEnter(close);
    expect(close).toHaveClass("bg-destructive");

    const visibility = vi
      .spyOn(document, "visibilityState", "get")
      .mockReturnValue("hidden");
    try {
      act(() => {
        document.dispatchEvent(new Event("visibilitychange"));
      });
      expect(close).not.toHaveClass("bg-destructive");
    } finally {
      visibility.mockRestore();
    }
  });
});
