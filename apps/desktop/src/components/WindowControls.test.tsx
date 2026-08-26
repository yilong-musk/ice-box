import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WindowControls } from "./WindowControls";
import type { WindowChrome } from "@/lib/windowChrome";

const runWindowCommand = vi.fn();
let chrome: WindowChrome = "macos-overlay";

vi.mock("@/lib/windowChrome", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/windowChrome")>();
  return {
    ...actual,
    detectWindowChrome: () => chrome,
    runWindowCommand: (...args: unknown[]) => runWindowCommand(...args),
  };
});

describe("WindowControls", () => {
  afterEach(() => {
    chrome = "macos-overlay";
    runWindowCommand.mockReset();
  });

  it("does not render caption buttons on macOS overlay chrome", () => {
    chrome = "macos-overlay";
    const { container } = render(<WindowControls />);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders Windows caption buttons and forwards clicks", () => {
    chrome = "windows-custom";
    render(<WindowControls />);

    fireEvent.click(screen.getByRole("button", { name: "最小化" }));
    fireEvent.click(screen.getByRole("button", { name: "最大化" }));
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));

    expect(runWindowCommand).toHaveBeenCalledWith("minimize");
    expect(runWindowCommand).toHaveBeenCalledWith("toggleMaximize");
    expect(runWindowCommand).toHaveBeenCalledWith("close");
  });
});
