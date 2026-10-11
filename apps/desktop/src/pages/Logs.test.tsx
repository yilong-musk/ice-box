// SPDX-License-Identifier: GPL-3.0-or-later

import { act, fireEvent, render, waitFor, within } from "@testing-library/react";
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

  it("keeps the same row nodes when the tail slides forward", async () => {
    vi.useFakeTimers();
    getLogView.mockImplementation(async () => ["line-a", "line-b", "line-c"]);
    const { container } = render(<Logs />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    const pre = container.querySelector("pre")!;
    const before = [...pre.children];
    const kept = before[1];
    const keptText = kept.firstChild;
    expect(kept.textContent).toBe("line-b");

    getLogView.mockImplementation(async () => ["line-b", "line-c", "line-d"]);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS);
    });

    const after = [...pre.children];
    expect(after.map((node) => node.textContent)).toEqual(["line-b", "line-c", "line-d"]);
    expect(after[0]).toBe(kept);
    expect(after[0].firstChild).toBe(keptText);
  });

  it("does not stick to the bottom while a log line is selected", async () => {
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
    const selected = pre.querySelector("div")!.firstChild;
    let selecting = true;
    vi.spyOn(window, "getSelection").mockImplementation(
      () =>
        ({
          isCollapsed: !selecting,
          rangeCount: selecting ? 1 : 0,
          anchorNode: selecting ? selected : null,
          focusNode: selecting ? selected : null,
        }) as Selection,
    );

    getLogView.mockImplementation(async () => [...baseTail, "INFO extra line while selected"]);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS);
    });
    expect(scrollTop).toBe(0);

    selecting = false;
    getLogView.mockImplementation(async () => [
      ...baseTail,
      "INFO extra line while selected",
      "INFO line after selection cleared",
    ]);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS);
    });
    expect(scrollTop).toBe(2000);
  });

  describe("select to copy", () => {
    const writeText = vi.fn();

    beforeEach(() => {
      writeText.mockReset();
      writeText.mockResolvedValue(undefined);
      Object.defineProperty(navigator, "clipboard", {
        configurable: true,
        value: { writeText },
      });
    });

    async function renderLines() {
      const view = render(<Logs />);
      await waitFor(() => {
        expect(within(view.container).getByText(/sing-box ready/)).toBeInTheDocument();
      });
      return view;
    }

    function selectIn(pre: HTMLElement, start: number, end: number) {
      const node = pre.querySelector("div")!.firstChild as Text;
      const range = document.createRange();
      range.setStart(node, start);
      range.setEnd(node, end);
      const selection = window.getSelection();
      if (selection == null) throw new Error("missing selection");
      selection.removeAllRanges();
      selection.addRange(range);
    }

    it("copies a selection made by a drag", async () => {
      const { container } = await renderLines();
      const pre = container.querySelector("pre")!;
      const line = pre.querySelector("div")!;
      fireEvent.mouseDown(line, { button: 0 });
      selectIn(pre, 0, 4);
      fireEvent.mouseUp(line, { button: 0, detail: 1 });
      await waitFor(() => {
        expect(writeText).toHaveBeenCalledWith("INFO");
      });
    });

    it("copies a double-click of an existing selection", async () => {
      const { container } = await renderLines();
      const pre = container.querySelector("pre")!;
      const line = pre.querySelector("div")!;
      selectIn(pre, 0, 4);
      fireEvent.mouseDown(line, { button: 0 });
      fireEvent.mouseUp(line, { button: 0, detail: 2 });
      await waitFor(() => {
        expect(writeText).toHaveBeenCalledWith("INFO");
      });
    });

    it("does not copy a click that leaves the selection unchanged", async () => {
      const { container } = await renderLines();
      const pre = container.querySelector("pre")!;
      const line = pre.querySelector("div")!;
      selectIn(pre, 0, 4);
      fireEvent.mouseDown(line, { button: 0 });
      fireEvent.mouseUp(line, { button: 0, detail: 1 });
      expect(writeText).not.toHaveBeenCalled();
    });

    it("does not copy a selection that started outside the log view", async () => {
      const { container } = await renderLines();
      const pre = container.querySelector("pre")!;
      selectIn(pre, 0, 4);
      fireEvent.mouseDown(document.documentElement, { button: 0 });
      fireEvent.mouseUp(pre, { button: 0, detail: 1 });
      expect(writeText).not.toHaveBeenCalled();
    });

    it("shows a failure when the clipboard rejects the selection", async () => {
      writeText.mockRejectedValue(new Error("denied"));
      document.execCommand = () => false;
      const { container } = await renderLines();
      const pre = container.querySelector("pre")!;
      const line = pre.querySelector("div")!;
      fireEvent.mouseDown(line, { button: 0 });
      selectIn(pre, 0, 4);
      fireEvent.mouseUp(line, { button: 0, detail: 1 });
      await waitFor(() => {
        expect(within(container).getByRole("alert")).toHaveTextContent(t("logs.copyFailed"));
      });
      expect(within(container).queryByTestId("log-copy-hint")).toBeNull();
    });

    it("shows a capsule after a successful copy and hides it", async () => {
      vi.useFakeTimers();
      const view = render(<Logs />);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      const pre = view.container.querySelector("pre")!;
      const line = pre.querySelector("div")!;
      fireEvent.mouseDown(line, { button: 0 });
      selectIn(pre, 0, 4);
      fireEvent.mouseUp(line, { button: 0, detail: 1 });
      await act(async () => {
        await Promise.resolve();
      });
      const hint = within(view.container).getByTestId("log-copy-hint");
      expect(hint).toHaveTextContent(t("logs.copied"));
      expect(hint).toHaveAttribute("role", "status");
      expect(within(view.container).queryByRole("alert")).toBeNull();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(1599);
      });
      expect(within(view.container).getByTestId("log-copy-hint")).toBeInTheDocument();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1);
      });
      expect(within(view.container).queryByTestId("log-copy-hint")).toBeNull();
    });

    it("replaces a copy failure with the capsule after a later success", async () => {
      writeText.mockRejectedValueOnce(new Error("denied"));
      document.execCommand = () => false;
      const { container } = await renderLines();
      const pre = container.querySelector("pre")!;
      const line = pre.querySelector("div")!;
      fireEvent.mouseDown(line, { button: 0 });
      selectIn(pre, 0, 4);
      fireEvent.mouseUp(line, { button: 0, detail: 1 });
      await waitFor(() => {
        expect(within(container).getByRole("alert")).toHaveTextContent(t("logs.copyFailed"));
      });

      writeText.mockResolvedValueOnce(undefined);
      fireEvent.mouseDown(line, { button: 0 });
      selectIn(pre, 0, 8);
      fireEvent.mouseUp(line, { button: 0, detail: 1 });
      await waitFor(() => {
        expect(within(container).getByTestId("log-copy-hint")).toHaveTextContent(t("logs.copied"));
      });
      expect(within(container).queryByRole("alert")).toBeNull();
    });

    it("hides the capsule when the log page is left", async () => {
      const view = await renderLines();
      const pre = view.container.querySelector("pre")!;
      const line = pre.querySelector("div")!;
      fireEvent.mouseDown(line, { button: 0 });
      selectIn(pre, 0, 4);
      fireEvent.mouseUp(line, { button: 0, detail: 1 });
      await waitFor(() => {
        expect(within(view.container).getByTestId("log-copy-hint")).toBeInTheDocument();
      });

      view.rerender(<Logs active={false} />);
      await waitFor(() => {
        expect(within(view.container).queryByTestId("log-copy-hint")).toBeNull();
      });
    });
  });
});
