// SPDX-License-Identifier: GPL-3.0-or-later

import { act, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { t } from "../lib/i18n";
import { Logs } from "./Logs";

const getLogView = vi.fn();
const setLogViewActive = vi.fn();

vi.mock("../api/client", () => ({
  api: {
    getLogView: (...args: unknown[]) => getLogView(...args),
    setLogViewActive: (...args: unknown[]) => setLogViewActive(...args),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

const POLL_MS = 2000;

const baseTail = [
  "INFO 08-23 13:47:02 ice_core: sing-box ready",
  "WARN 08-23 13:47:04 ice_proxy_sys: proxy apply slow",
  "ERROR 08-23 13:47:06 outbound: dial tcp: connection refused",
];

describe("Logs", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setLogViewActive.mockResolvedValue(undefined);
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
    expect(view.queryByRole("button", { name: t("common.refresh") })).toBeNull();
    expect(view.queryByText(t("app.nav.logs"))).toBeNull();
    expect(view.queryByLabelText(t("settings.logDebug"))).toBeNull();
    expect(container.querySelector("[data-slot='card']")).toBeNull();
    const logView = view.getByTestId("log-view");
    expect(logView.parentElement).toBe(view.getByTestId("logs-panel"));
  });

  it("colors warn and error lines without changing info", async () => {
    const { container } = render(<Logs />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText(/proxy apply slow/)).toBeInTheDocument();
    });
    const info = view.getByText(/sing-box ready/);
    const warn = view.getByText(/proxy apply slow/);
    const error = view.getByText(/connection refused/);
    expect(info).not.toHaveClass("text-warn");
    expect(info).not.toHaveClass("text-destructive");
    expect(warn).toHaveClass("text-warn");
    expect(error).toHaveClass("text-destructive");
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

  it("drops a read that finishes after the page is left", async () => {
    let resolveTail: (lines: string[]) => void = () => {};
    getLogView.mockImplementationOnce(
      () =>
        new Promise<string[]>((resolve) => {
          resolveTail = resolve;
        }),
    );
    const { container, rerender } = render(<Logs active />);
    await waitFor(() => {
      expect(getLogView).toHaveBeenCalled();
    });

    rerender(<Logs active={false} />);
    await act(async () => {
      resolveTail(["ERROR 08-23 13:47:09 late line that should not appear"]);
      await Promise.resolve();
    });

    expect(container).not.toHaveTextContent("late line that should not appear");
    expect(container).toHaveTextContent(t("logs.empty"));
    expect(setLogViewActive).toHaveBeenCalledWith(false);
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
