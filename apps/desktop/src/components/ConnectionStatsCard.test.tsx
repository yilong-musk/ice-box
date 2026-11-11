// SPDX-License-Identifier: GPL-3.0-or-later

import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { t } from "../lib/i18n";
import { ConnectionStatsCard } from "./ConnectionStatsCard";

const getConnectionStats = vi.fn();
const listConnections = vi.fn();

vi.mock("../api/client", () => ({
  api: {
    getConnectionStats: (...args: unknown[]) => getConnectionStats(...args),
    listConnections: (...args: unknown[]) => listConnections(...args),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

const MB = 1024 * 1024;

async function flush(ms = 0) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

describe("ConnectionStatsCard", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    getConnectionStats.mockReset();
    listConnections.mockReset();
    getConnectionStats.mockResolvedValue({
      running: true,
      count: 8,
      upload_total: 50 * MB,
      download_total: 2 * 1024 * MB,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows the count and totals from the light stats call", async () => {
    const view = render(<ConnectionStatsCard running active />);
    await flush();
    expect(view.getByTestId("home-stats-connections")).toHaveTextContent("8");
    expect(view.getByTestId("home-stats-download")).toHaveTextContent("2.00 GB");
    expect(view.getByTestId("home-stats-upload")).toHaveTextContent("50.00 MB");
    // The strip never pulls the full connection list.
    expect(listConnections).not.toHaveBeenCalled();
  });

  it("does not poll while paused and keeps the last figures", async () => {
    const view = render(<ConnectionStatsCard running active />);
    await flush();
    expect(getConnectionStats).toHaveBeenCalledTimes(1);

    view.rerender(<ConnectionStatsCard running active paused />);
    await flush(5000);
    expect(getConnectionStats).toHaveBeenCalledTimes(1);
    expect(view.getByTestId("home-stats-connections")).toHaveTextContent("8");

    view.rerender(<ConnectionStatsCard running active />);
    await flush();
    expect(getConnectionStats).toHaveBeenCalledTimes(2);
  });

  it("drops stale figures after repeated failures and recovers", async () => {
    const view = render(<ConnectionStatsCard running active />);
    await flush();
    getConnectionStats.mockRejectedValue("core gone");

    // One or two misses keep the last figures.
    await flush(2000);
    expect(view.getByTestId("home-stats-connections")).toHaveTextContent("8");

    await flush(1000);
    expect(view.getByTestId("home-stats-connections")).toHaveTextContent("—");
    expect(view.getByTestId("home-connection-stats")).toHaveAttribute(
      "title",
      t("traffic.samplingInterrupted", { error: "core gone" }),
    );

    getConnectionStats.mockResolvedValue({
      running: true,
      count: 3,
      upload_total: 0,
      download_total: 0,
    });
    await flush(1000);
    expect(view.getByTestId("home-stats-connections")).toHaveTextContent("3");
    expect(view.getByTestId("home-connection-stats")).not.toHaveAttribute("title");
  });

  it("shows dashes and does not poll while the core is stopped", async () => {
    const view = render(<ConnectionStatsCard running={false} active />);
    await flush(3000);
    expect(getConnectionStats).not.toHaveBeenCalled();
    expect(view.getByTestId("home-stats-download")).toHaveTextContent("—");
  });
});
