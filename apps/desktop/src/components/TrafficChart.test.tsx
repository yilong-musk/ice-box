import { act, render, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TrafficChart } from "../components/TrafficChart";

const getTrafficSample = vi.fn();

vi.mock("../api/tauri", () => ({
  api: {
    getTrafficSample: (...args: unknown[]) => getTrafficSample(...args),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

describe("TrafficChart", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getTrafficSample.mockResolvedValue({ up: 100, down: 200 });
  });

  it("shows hint when core is not running", () => {
    const { container } = render(<TrafficChart running={false} />);
    const view = within(container);
    expect(view.getByText(/启动代理服务后显示/)).toBeInTheDocument();
  });

  it("samples traffic when running", async () => {
    const { container } = render(<TrafficChart running={true} />);
    const view = within(container);
    await waitFor(() => {
      expect(getTrafficSample).toHaveBeenCalled();
    });
    expect(view.getByText(/峰值刻度/)).toBeInTheDocument();
  });

  it("does not flash an error when a sample fails transiently", async () => {
    getTrafficSample.mockRejectedValueOnce("clash api down");
    getTrafficSample.mockResolvedValue({ up: 10, down: 20 });
    const { container } = render(<TrafficChart running={true} />);
    const view = within(container);
    await waitFor(() => {
      expect(getTrafficSample).toHaveBeenCalled();
    });
    expect(view.queryByText(/clash api down/i)).toBeNull();
    expect(container.querySelector(".error")).toBeNull();
  });

  it("shows error after consecutive sample failures", async () => {
    vi.useFakeTimers();
    getTrafficSample.mockRejectedValue("clash api down");
    const { container } = render(<TrafficChart running={true} />);
    const view = within(container);

    await act(async () => {
      await Promise.resolve();
    });
    expect(view.queryByText(/采样中断/)).toBeNull();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
      await Promise.resolve();
    });
    expect(view.queryByText(/采样中断/)).toBeNull();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
      await Promise.resolve();
    });
    expect(view.getByText(/采样中断：clash api down/i)).toBeInTheDocument();
    expect(container.querySelector(".error")).not.toBeNull();

    vi.useRealTimers();
  });

  it("clears error after a successful sample", async () => {
    vi.useFakeTimers();
    getTrafficSample.mockRejectedValue("clash api down");
    const { container } = render(<TrafficChart running={true} />);
    const view = within(container);

    await act(async () => {
      await Promise.resolve();
      await vi.advanceTimersByTimeAsync(2000);
      await Promise.resolve();
    });
    expect(view.getByText(/采样中断/)).toBeInTheDocument();

    getTrafficSample.mockResolvedValue({ up: 10, down: 20 });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
      await Promise.resolve();
    });
    expect(view.queryByText(/采样中断/)).toBeNull();
    expect(container.querySelector(".error")).toBeNull();

    vi.useRealTimers();
  });

  it("skips sampling while paused", async () => {
    render(<TrafficChart running={true} paused />);
    await new Promise((r) => setTimeout(r, 50));
    expect(getTrafficSample).not.toHaveBeenCalled();
  });

  it("resumes sampling after unpause even if a prior invoke hangs", async () => {
    let resolveHang: ((v: { up: number; down: number }) => void) | undefined;
    getTrafficSample.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveHang = resolve;
        }),
    );
    getTrafficSample.mockResolvedValue({ up: 5, down: 6 });

    const { rerender } = render(<TrafficChart running={true} paused={false} />);
    await waitFor(() => {
      expect(getTrafficSample).toHaveBeenCalledTimes(1);
    });

    rerender(<TrafficChart running={true} paused />);
    rerender(<TrafficChart running={true} paused={false} />);

    await waitFor(() => {
      expect(getTrafficSample).toHaveBeenCalledTimes(2);
    });

    // Abandoned hang must not block further samples once settled.
    resolveHang?.({ up: 1, down: 2 });
  });
});
