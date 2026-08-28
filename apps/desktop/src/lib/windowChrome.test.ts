import { describe, expect, it, vi } from "vitest";
import { detectWindowChrome, runWindowCommand } from "./windowChrome";

const minimize = vi.fn().mockResolvedValue(undefined);
const toggleMaximize = vi.fn().mockResolvedValue(undefined);
const close = vi.fn().mockResolvedValue(undefined);

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    minimize,
    toggleMaximize,
    close,
  }),
}));

describe("detectWindowChrome", () => {
  it("uses overlay chrome on macOS and jsdom darwin", () => {
    expect(detectWindowChrome("Mozilla/5.0 (darwin) jsdom/26", "")).toBe(
      "macos-overlay",
    );
    expect(
      detectWindowChrome(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
        "MacIntel",
      ),
    ).toBe("macos-overlay");
  });

  it("uses custom caption buttons on Windows", () => {
    expect(
      detectWindowChrome(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
        "Win32",
      ),
    ).toBe("windows-custom");
  });

  it("leaves Linux on the default decorated window", () => {
    expect(detectWindowChrome("Mozilla/5.0 (X11; Linux x86_64)", "Linux x86_64")).toBe(
      "plain",
    );
  });
});

describe("runWindowCommand", () => {
  it("forwards commands to the current Tauri window", async () => {
    await runWindowCommand("minimize");
    await runWindowCommand("toggleMaximize");
    await runWindowCommand("close");

    expect(minimize).toHaveBeenCalledTimes(1);
    expect(toggleMaximize).toHaveBeenCalledTimes(1);
    expect(close).toHaveBeenCalledTimes(1);
  });
});
