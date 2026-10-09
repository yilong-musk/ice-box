// SPDX-License-Identifier: GPL-3.0-or-later

import { Profiler } from "react";
import { act, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { t } from "../lib/i18n";
import { formatRate } from "../lib/traffic";
import { TrafficChart } from "./TrafficChart";

const getTrafficSince = vi.fn();
const listenTrafficSample = vi.fn();

vi.mock("../api/client", () => ({
  api: {
    getTrafficSince: (...args: unknown[]) => getTrafficSince(...args),
    listenTrafficSample: (...args: unknown[]) => listenTrafficSample(...args),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

type LiveSample = { up: number; down: number; t: number };

function snap(
  points: { up: number; down: number; t: number }[],
  latest?: { up: number; down: number } | null,
  peak?: { up: number; down: number } | null,
) {
  return {
    generation: 1,
    cursor: points.length ? points[points.length - 1].t : null,
    points,
    latest:
      latest === undefined ? (points[points.length - 1] ?? null) : latest,
    peak:
      peak === undefined
        ? points.reduce<{ up: number; down: number } | null>(
            (current, point) =>
              current === null
                ? point
                : {
                    up: Math.max(current.up, point.up),
                    down: Math.max(current.down, point.down),
                  },
            null,
          )
        : peak,
  };
}

async function flushMicrotasks() {
  await act(async () => {
    await Promise.resolve();
  });
}

function setVisibility(value: DocumentVisibilityState) {
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => value,
  });
}

describe("TrafficChart", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setVisibility("visible");
    listenTrafficSample.mockImplementation(async () => () => {});
    getTrafficSince.mockResolvedValue(
      snap([{ up: 100, down: 200, t: 1_000 }]),
    );
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows hint when core is not running", () => {
    const { container } = render(<TrafficChart running={false} />);
    const view = within(container);
    expect(view.getByText(t("traffic.idleHint", { n: 60 }))).toBeInTheDocument();
  });

  it("hydrates from backend history instead of starting empty", async () => {
    getTrafficSince.mockResolvedValue(
      snap([
        { up: 10, down: 20, t: 1_000 },
        { up: 30, down: 40, t: 2_000 },
        { up: 50, down: 80, t: 3_000 },
      ]),
    );
    const { container } = render(<TrafficChart running={true} />);
    const view = within(container);
    await waitFor(() => {
      expect(getTrafficSince).toHaveBeenCalled();
    });
    expect(
      view.getByText(t("traffic.peak", { rate: formatRate(80), n: 60 })),
    ).toBeInTheDocument();
    expect(view.getByText(`↓ ${formatRate(80)}`)).toBeInTheDocument();
  });

  it("uses the 60-second window peak for the y scale", async () => {
    getTrafficSince.mockResolvedValue(
      snap(
        [{ up: 80 * 1024, down: 40 * 1024, t: 1_000 }],
        { up: 80 * 1024, down: 40 * 1024 },
        { up: 2 * 1024 * 1024, down: 512 * 1024 },
      ),
    );
    const { container } = render(<TrafficChart running={true} />);
    const view = within(container);
    await waitFor(() => {
      expect(getTrafficSince).toHaveBeenCalled();
    });
    expect(
      view.getByText(
        t("traffic.peak", { rate: formatRate(2 * 1024 * 1024), n: 60 }),
      ),
    ).toBeInTheDocument();
  });

  it("does not flash an error when a snapshot fails transiently", async () => {
    getTrafficSince.mockRejectedValueOnce("clash api down");
    getTrafficSince.mockResolvedValue(snap([{ up: 10, down: 20, t: 1 }]));
    const { container } = render(<TrafficChart running={true} />);
    const view = within(container);
    await waitFor(() => {
      expect(getTrafficSince).toHaveBeenCalled();
    });
    expect(view.queryByText(/clash api down/i)).toBeNull();
    expect(container.querySelector(".error")).toBeNull();
  });

  it("shows error after consecutive snapshot failures", async () => {
    vi.useFakeTimers();
    getTrafficSince.mockRejectedValue("clash api down");
    const { container, unmount } = render(<TrafficChart running={true} />);
    const view = within(container);

    await flushMicrotasks();
    expect(view.queryByText(t("traffic.samplingInterrupted", { error: "clash api down" }))).toBeNull();

    await act(async () => {
      vi.advanceTimersByTime(1000);
      await Promise.resolve();
    });
    expect(view.queryByText(t("traffic.samplingInterrupted", { error: "clash api down" }))).toBeNull();

    await act(async () => {
      vi.advanceTimersByTime(1000);
      await Promise.resolve();
    });
    expect(view.getByText(t("traffic.samplingInterrupted", { error: "clash api down" }))).toBeInTheDocument();
    expect(container.querySelector(".error")).not.toBeNull();
    unmount();
  });

  it("clears error after a successful snapshot", async () => {
    vi.useFakeTimers();
    getTrafficSince.mockRejectedValue("clash api down");
    const { container, unmount } = render(<TrafficChart running={true} />);
    const view = within(container);

    await flushMicrotasks();
    await act(async () => {
      vi.advanceTimersByTime(1000);
      await Promise.resolve();
    });
    await act(async () => {
      vi.advanceTimersByTime(1000);
      await Promise.resolve();
    });
    expect(view.getByText(t("traffic.samplingInterrupted", { error: "clash api down" }))).toBeInTheDocument();

    getTrafficSince.mockResolvedValue(snap([{ up: 10, down: 20, t: 1 }]));
    await act(async () => {
      vi.advanceTimersByTime(1000);
      await Promise.resolve();
    });
    expect(view.queryByText(t("traffic.samplingInterrupted", { error: "clash api down" }))).toBeNull();
    expect(container.querySelector(".error")).toBeNull();
    unmount();
  });

  it("skips polling while paused", async () => {
    render(<TrafficChart running={true} paused />);
    await new Promise((r) => setTimeout(r, 50));
    expect(getTrafficSince).not.toHaveBeenCalled();
  });

  it("resumes polling after unpause even if a prior invoke hangs", async () => {
    let resolveHang:
      | ((v: {
          points: { up: number; down: number; t: number }[];
          latest: { up: number; down: number } | null;
        }) => void)
      | undefined;
    getTrafficSince.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveHang = resolve;
        }),
    );
    getTrafficSince.mockResolvedValue(snap([{ up: 5, down: 6, t: 1 }]));

    const { rerender } = render(<TrafficChart running={true} paused={false} />);
    await waitFor(() => {
      expect(getTrafficSince).toHaveBeenCalledTimes(1);
    });

    rerender(<TrafficChart running={true} paused />);
    rerender(<TrafficChart running={true} paused={false} />);

    await waitFor(() => {
      expect(getTrafficSince).toHaveBeenCalledTimes(2);
    });

    resolveHang?.(snap([{ up: 1, down: 2, t: 1 }]));
  });

  it("draws a live sample and raises the peak without another snapshot", async () => {
    let onSample: (sample: LiveSample) => void = () => {};
    listenTrafficSample.mockImplementation(async (handler: (sample: LiveSample) => void) => {
      onSample = handler;
      return () => {};
    });
    const { container } = render(<TrafficChart running />);
    const view = within(container);
    await waitFor(() => {
      expect(getTrafficSince).toHaveBeenCalledTimes(1);
    });

    const peak = 3 * 1024 * 1024;
    await act(async () => {
      onSample({ up: 50, down: peak, t: 2_000 });
    });

    expect(view.getByText(`↓ ${formatRate(peak)}`)).toBeInTheDocument();
    expect(
      view.getByText(t("traffic.peak", { rate: formatRate(peak), n: 60 })),
    ).toBeInTheDocument();
    expect(getTrafficSince).toHaveBeenCalledTimes(1);
  });

  it("does not render again when the same live sample is repeated", async () => {
    const now = Date.now();
    getTrafficSince.mockResolvedValue(snap([{ up: 100, down: 200, t: now }]));
    let onSample: (sample: LiveSample) => void = () => {};
    listenTrafficSample.mockImplementation(async (handler: (sample: LiveSample) => void) => {
      onSample = handler;
      return () => {};
    });
    let commits = 0;
    const { container } = render(
      <Profiler id="traffic" onRender={() => { commits += 1; }}>
        <TrafficChart running />
      </Profiler>,
    );
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText(`↓ ${formatRate(200)}`)).toBeInTheDocument();
    });
    await act(async () => {});
    const afterHydrate = commits;

    await act(async () => {
      onSample({ up: 100, down: 200, t: now });
    });

    expect(commits).toBe(afterHydrate);
    expect(view.getByText(`↓ ${formatRate(200)}`)).toBeInTheDocument();
  });

  it("polls forward from the live cursor instead of redrawing the same sample", async () => {
    vi.useFakeTimers();
    let onSample: (sample: LiveSample) => void = () => {};
    listenTrafficSample.mockImplementation(async (handler: (sample: LiveSample) => void) => {
      onSample = handler;
      return () => {};
    });
    getTrafficSince.mockResolvedValueOnce(
      snap([{ up: 10, down: 20, t: 1_000 }]),
    );
    getTrafficSince.mockResolvedValueOnce(
      snap([], { up: 40, down: 80 }, { up: 40, down: 80 }),
    );
    const { container, unmount } = render(<TrafficChart running />);
    const view = within(container);

    await flushMicrotasks();
    await act(async () => {
      onSample({ up: 40, down: 80, t: 4_000 });
    });
    expect(view.getByText(`↓ ${formatRate(80)}`)).toBeInTheDocument();

    await act(async () => {
      vi.advanceTimersByTime(1000);
      await Promise.resolve();
    });

    expect(getTrafficSince).toHaveBeenLastCalledWith(4_000);
    expect(view.getByText(`↓ ${formatRate(80)}`)).toBeInTheDocument();
    unmount();
  });

  it("keeps a live sample when an older snapshot resolves later", async () => {
    let resolveFirst: (value: ReturnType<typeof snap>) => void = () => {};
    getTrafficSince.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveFirst = resolve;
        }),
    );
    let onSample: (sample: LiveSample) => void = () => {};
    listenTrafficSample.mockImplementation(async (handler: (sample: LiveSample) => void) => {
      onSample = handler;
      return () => {};
    });
    const { container } = render(<TrafficChart running />);
    await waitFor(() => {
      expect(listenTrafficSample).toHaveBeenCalled();
    });

    const liveDown = 4 * 1024 * 1024;
    await act(async () => {
      onSample({ up: 10, down: liveDown, t: 5_000 });
    });
    await act(async () => {
      resolveFirst(snap([{ up: 1, down: 2, t: 1_000 }]));
    });

    const view = within(container);
    expect(view.getByText(`↓ ${formatRate(liveDown)}`)).toBeInTheDocument();
    expect(
      view.getByText(t("traffic.peak", { rate: formatRate(liveDown), n: 60 })),
    ).toBeInTheDocument();
  });

  it("does not lower the peak when a snapshot shares the live cursor", async () => {
    vi.useFakeTimers();
    const liveDown = 4 * 1024 * 1024;
    let onSample: (sample: LiveSample) => void = () => {};
    listenTrafficSample.mockImplementation(async (handler: (sample: LiveSample) => void) => {
      onSample = handler;
      return () => {};
    });
    getTrafficSince.mockResolvedValueOnce(snap([{ up: 10, down: 20, t: 1_000 }]));
    getTrafficSince.mockResolvedValueOnce(
      snap(
        [{ up: 10, down: 100, t: 5_000 }],
        { up: 10, down: 100 },
        { up: 10, down: 100 },
      ),
    );
    const { container, unmount } = render(<TrafficChart running />);
    const view = within(container);

    await flushMicrotasks();
    await act(async () => {
      onSample({ up: 10, down: liveDown, t: 5_000 });
    });
    expect(view.getByText(`↓ ${formatRate(liveDown)}`)).toBeInTheDocument();
    expect(
      view.getByText(t("traffic.peak", { rate: formatRate(liveDown), n: 60 })),
    ).toBeInTheDocument();

    await act(async () => {
      vi.advanceTimersByTime(1000);
      await Promise.resolve();
    });

    expect(getTrafficSince).toHaveBeenLastCalledWith(5_000);
    expect(view.getByText(`↓ ${formatRate(liveDown)}`)).toBeInTheDocument();
    expect(
      view.getByText(t("traffic.peak", { rate: formatRate(liveDown), n: 60 })),
    ).toBeInTheDocument();
    unmount();
  });

  it("replaces the series when the backend generation changes", async () => {
    vi.useFakeTimers();
    getTrafficSince.mockResolvedValueOnce(
      snap([{ up: 10, down: 20, t: 1_000 }]),
    );
    getTrafficSince.mockResolvedValueOnce({
      generation: 2,
      cursor: 4_000,
      points: [{ up: 5, down: 6, t: 4_000 }],
      latest: { up: 5, down: 6 },
      peak: { up: 5, down: 9 * 1024 },
    });
    const { container, unmount } = render(<TrafficChart running />);
    const view = within(container);

    await flushMicrotasks();
    expect(view.getByText(`↓ ${formatRate(20)}`)).toBeInTheDocument();

    await act(async () => {
      vi.advanceTimersByTime(1000);
      await Promise.resolve();
    });

    expect(view.getByText(`↓ ${formatRate(6)}`)).toBeInTheDocument();
    expect(
      view.getByText(t("traffic.peak", { rate: formatRate(9 * 1024), n: 60 })),
    ).toBeInTheDocument();
    unmount();
  });

  it("catches up when the tab becomes visible again", async () => {
    vi.useFakeTimers();
    const { unmount } = render(<TrafficChart running />);
    await flushMicrotasks();
    expect(getTrafficSince).toHaveBeenCalledTimes(1);

    setVisibility("hidden");
    await act(async () => {
      vi.advanceTimersByTime(3_000);
      await Promise.resolve();
    });
    expect(getTrafficSince).toHaveBeenCalledTimes(1);

    setVisibility("visible");
    document.dispatchEvent(new Event("visibilitychange"));
    await flushMicrotasks();
    expect(getTrafficSince).toHaveBeenCalledTimes(2);
    unmount();
  });

  it("unsubscribes a listener that resolves after unmount", async () => {
    let resolveListen: (unlisten: () => void) => void = () => {};
    const unlisten = vi.fn();
    listenTrafficSample.mockImplementation(
      () =>
        new Promise<() => void>((resolve) => {
          resolveListen = resolve;
        }),
    );
    const { unmount } = render(<TrafficChart running />);
    unmount();
    await act(async () => {
      resolveListen(unlisten);
    });
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("unsubscribes the traffic listener on unmount", async () => {
    const unlisten = vi.fn();
    listenTrafficSample.mockResolvedValue(unlisten);
    const { unmount } = render(<TrafficChart running />);
    await waitFor(() => {
      expect(listenTrafficSample).toHaveBeenCalled();
    });
    unmount();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});
