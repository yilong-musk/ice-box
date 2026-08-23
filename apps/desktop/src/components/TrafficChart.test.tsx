import { render, waitFor, within } from "@testing-library/react";
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
    expect(view.getByText(/启动内核后显示/)).toBeInTheDocument();
  });

  it("samples traffic when running", async () => {
    const { container } = render(<TrafficChart running={true} />);
    const view = within(container);
    await waitFor(() => {
      expect(getTrafficSample).toHaveBeenCalled();
    });
    expect(view.getByText(/峰值刻度/)).toBeInTheDocument();
  });
});
