// SPDX-License-Identifier: GPL-3.0-or-later

import { act, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { t } from "../lib/i18n";
import { formatRate } from "../lib/traffic";
import { TrafficChart } from "./TrafficChart";

const getTrafficSince = vi.fn();

vi.mock("../api/tauri", () => ({
  api: {
    getTrafficSince: (...args: unknown[]) => getTrafficSince(...args),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

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

describe("TrafficChart", () => {
  beforeEach(() => {
    vi.clearAllMocks();
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
});
