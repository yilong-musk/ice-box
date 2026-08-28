import { act, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Logs } from "./Logs";

const getLogView = vi.fn();

vi.mock("../api/tauri", () => ({
  api: {
    getLogView: (...args: unknown[]) => getLogView(...args),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

const POLL_MS = 2000;

const baseTail = [
  "INFO 08-23 13:47:02 ice_core: sing-box ready",
  "ERROR 08-23 13:47:06 outbound: dial tcp: connection refused",
];

describe("Logs", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getLogView.mockImplementation(async () => [...baseTail]);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("shows merged app and core lines without a source selector", async () => {
    const { container } = render(<Logs />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText(/sing-box ready/)).toBeInTheDocument();
      expect(view.getByText(/connection refused/)).toBeInTheDocument();
    });
    expect(getLogView).toHaveBeenCalledWith(500);
    expect(view.queryByRole("combobox")).toBeNull();
    expect(view.queryByRole("button", { name: "刷新" })).toBeNull();
    expect(view.queryByText("日志")).toBeNull();
    expect(container.querySelector("[data-slot='card']")).toBeNull();
    const logView = container.querySelector(".log-view");
    expect(logView?.parentElement?.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["logs-panel"]),
    );
    expect(logView?.className.split(/\s+/)).toEqual(
      expect.arrayContaining([
        "min-h-0",
        "flex-1",
        "overflow-auto",
        "bg-card",
        "text-foreground",
      ]),
    );
    expect(logView?.className.split(/\s+/)).not.toContain("bg-muted/40");
    expect(logView?.className.split(/\s+/)).not.toContain("text-muted-foreground");
  });

  it("polls automatically", async () => {
    vi.useFakeTimers();
    render(<Logs />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    const initial = getLogView.mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS);
    });
    expect(getLogView.mock.calls.length).toBeGreaterThan(initial);
  });

  it("replaces an old connection entry when the newest tail changes", async () => {
    vi.useFakeTimers();
    getLogView
      .mockResolvedValueOnce(["INFO 08-23 13:47:06 as.xiaohongshu.com:443 → 节点 A"])
      .mockResolvedValueOnce(["INFO 08-23 13:47:08 example.com:443 → 节点 B"]);
    const { container } = render(<Logs />);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(container).toHaveTextContent("as.xiaohongshu.com:443");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS);
    });
    expect(container).toHaveTextContent("example.com:443");
    expect(container).not.toHaveTextContent("as.xiaohongshu.com:443");
  });

  it("pauses polling when the tab is hidden", async () => {
    vi.useFakeTimers();
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    render(<Logs />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    const initial = getLogView.mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS);
    });
    expect(getLogView.mock.calls.length).toBe(initial);
  });

  it("auto-scrolls to the latest lines", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
      cb(0);
      return 0;
    });
    const { container } = render(<Logs />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    const pre = container.querySelector("pre");
    expect(pre).not.toBeNull();
    let scrollTop = 0;
    Object.defineProperty(pre!, "scrollHeight", { value: 2000, configurable: true });
    Object.defineProperty(pre!, "clientHeight", { value: 100, configurable: true });
    Object.defineProperty(pre!, "scrollTop", {
      get: () => scrollTop,
      set: (v: number) => {
        scrollTop = v;
      },
      configurable: true,
    });

    getLogView.mockImplementation(async () => [
      ...baseTail,
      "INFO 08-23 13:47:08 example.com:443 → 香港 1",
    ]);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS);
    });
    expect(scrollTop).toBe(2000);
  });

  it("stops forcing scroll once the user scrolls up", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
      cb(0);
      return 0;
    });
    const { container } = render(<Logs />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    const pre = container.querySelector("pre")!;
    let scrollTop = 0;
    Object.defineProperty(pre, "scrollHeight", { value: 2000, configurable: true });
    Object.defineProperty(pre, "clientHeight", { value: 100, configurable: true });
    Object.defineProperty(pre, "scrollTop", {
      get: () => scrollTop,
      set: (v: number) => {
        scrollTop = v;
      },
      configurable: true,
    });

    scrollTop = 500;
    act(() => {
      pre.dispatchEvent(new Event("scroll", { bubbles: true }));
    });

    getLogView.mockImplementation(async () => [
      ...baseTail,
      "INFO 08-23 13:47:09 another connection line",
    ]);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS);
    });
    expect(scrollTop).toBe(500);
  });
});
