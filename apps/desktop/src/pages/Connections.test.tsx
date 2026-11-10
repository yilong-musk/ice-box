// SPDX-License-Identifier: GPL-3.0-or-later

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ConnectionRow, ConnectionsResponse } from "../api/contracts";
import { t } from "../lib/i18n";
import { Connections } from "./Connections";

const listConnections = vi.fn();
const closeConnection = vi.fn();
const closeAllConnections = vi.fn();

vi.mock("../api/client", () => ({
  api: {
    listConnections: (...args: unknown[]) => listConnections(...args),
    closeConnection: (...args: unknown[]) => closeConnection(...args),
    closeAllConnections: (...args: unknown[]) => closeAllConnections(...args),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

const POLL_MS = 1000;
const NOW = Date.parse("2026-11-10T04:00:00Z");

function row(patch: Partial<ConnectionRow> = {}): ConnectionRow {
  return {
    id: "c-github",
    network: "tcp",
    inbound: "mixed/mixed-in",
    source: "127.0.0.1:51234",
    host: "api.github.com",
    destination_ip: "",
    destination_port: "443",
    process_path: "/usr/bin/curl",
    upload: 1024,
    download: 4096,
    start_ms: NOW - 65_000,
    chains: ["node-a", "proxy"],
    rule: "domain_suffix=github.com => route(proxy)",
    ...patch,
  };
}

const dnsRow = row({
  id: "c-dns",
  network: "udp",
  host: "",
  destination_ip: "1.1.1.1",
  destination_port: "53",
  process_path: "",
  upload: 100,
  download: 200,
  start_ms: NOW - 5_000,
  chains: ["direct"],
  rule: "final",
});

function snapshot(connections: ConnectionRow[], patch: Partial<ConnectionsResponse> = {}) {
  return {
    running: true,
    upload_total: 2048,
    download_total: 5 * 1024 * 1024,
    connections,
    ...patch,
  };
}

function targets(container: HTMLElement): string[] {
  return within(container)
    .getAllByTestId("connection-row")
    .map((item) => item.querySelector("[data-slot='item-title'] span")?.textContent ?? "");
}

describe("Connections", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(NOW);
    listConnections.mockImplementation(async () => snapshot([row(), dnsRow]));
    closeConnection.mockResolvedValue(undefined);
    closeAllConnections.mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("lists each connection with its route, rule, process and totals", async () => {
    const { container } = render(<Connections />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getAllByTestId("connection-row")).toHaveLength(2);
    });
    // Newest first.
    expect(targets(container)).toEqual(["1.1.1.1:53", "api.github.com:443"]);
    const github = view.getAllByTestId("connection-row")[1];
    expect(within(github).getByTestId("connection-chain")).toHaveTextContent("proxy → node-a");
    expect(github).toHaveTextContent("domain_suffix=github.com");
    expect(github).not.toHaveTextContent("route(proxy)");
    expect(github).toHaveTextContent("curl");
    expect(github).toHaveTextContent("TCP");
    expect(within(github).getByTestId("connection-traffic")).toHaveTextContent(
      "↓ 4.0 KB ↑ 1.0 KB",
    );
    expect(github).not.toHaveTextContent("/s");
    expect(within(github).getByTestId("connection-age")).toHaveTextContent("1m 05s");
    expect(view.getByTestId("connections-summary")).toHaveTextContent(
      `${t("connections.count", { count: 2 })} · ${t("connections.totals")} ↓ 5.00 MB ↑ 2.0 KB`,
    );
    // Download and upload use the Home chart colours.
    const traffic = within(github).getByTestId("connection-traffic");
    expect(within(traffic).getByTestId("traffic-down")).toHaveClass("text-ok");
    expect(within(traffic).getByTestId("traffic-down")).toHaveTextContent("↓ 4.0 KB");
    expect(within(traffic).getByTestId("traffic-up")).toHaveClass("text-primary");
    expect(within(traffic).getByTestId("traffic-up")).toHaveTextContent("↑ 1.0 KB");
  });

  it("scales each row's background to the largest download", async () => {
    listConnections.mockImplementation(async () =>
      snapshot([
        row({ id: "big", host: "big.example", download: 4000, start_ms: NOW - 3_000 }),
        row({ id: "small", host: "small.example", download: 1000, start_ms: NOW - 2_000 }),
        row({ id: "idle", host: "idle.example", download: 0, start_ms: NOW - 1_000 }),
      ]),
    );
    const { container } = render(<Connections />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getAllByTestId("connection-row")).toHaveLength(3);
    });
    const usage = (target: string) => {
      const item = view
        .getAllByTestId("connection-row")
        .find((el) => el.textContent?.includes(target));
      return item?.querySelector<HTMLElement>("[data-testid='connection-usage']") ?? null;
    };
    expect(usage("big.example")?.style.width).toBe("100%");
    expect(usage("small.example")?.style.width).toBe("25%");
    expect(usage("idle.example")).toBeNull();

    // Narrowing the list rescales to the largest row still shown.
    fireEvent.change(view.getByLabelText(t("connections.searchAria")), {
      target: { value: "small" },
    });
    expect(usage("small.example")?.style.width).toBe("100%");
  });

  it("filters by keyword and reports how many match", async () => {
    const { container } = render(<Connections />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getAllByTestId("connection-row")).toHaveLength(2);
    });
    const search = view.getByLabelText(t("connections.searchAria"));
    fireEvent.change(search, { target: { value: "GITHUB" } });
    expect(targets(container)).toEqual(["api.github.com:443"]);
    expect(view.getByTestId("connections-summary")).toHaveTextContent(
      t("connections.countFiltered", { shown: 1, count: 2 }),
    );

    fireEvent.change(search, { target: { value: "no-such-host" } });
    expect(view.getByTestId("connections-empty")).toHaveTextContent(
      t("connections.emptyFiltered"),
    );
  });

  it("sorts by bytes transferred", async () => {
    const { container } = render(<Connections />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getAllByTestId("connection-row")).toHaveLength(2);
    });
    fireEvent.click(view.getByRole("radio", { name: t("connections.sort.traffic") }));
    expect(targets(container)).toEqual(["api.github.com:443", "1.1.1.1:53"]);
  });

  it("closes one connection and removes its row at once", async () => {
    const { container } = render(<Connections />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getAllByTestId("connection-row")).toHaveLength(2);
    });
    listConnections.mockImplementation(async () => snapshot([dnsRow]));
    fireEvent.click(
      view.getByRole("button", {
        name: t("connections.close", { target: "api.github.com:443" }),
      }),
    );
    expect(closeConnection).toHaveBeenCalledWith("c-github");
    expect(targets(container)).toEqual(["1.1.1.1:53"]);
  });

  it("drops a poll that was in flight when a connection was closed", async () => {
    const { container } = render(<Connections />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getAllByTestId("connection-row")).toHaveLength(2);
    });
    let resolveStale: (value: ConnectionsResponse) => void = () => {};
    listConnections.mockImplementationOnce(
      () =>
        new Promise<ConnectionsResponse>((resolve) => {
          resolveStale = resolve;
        }),
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS);
    });
    listConnections.mockImplementation(async () => snapshot([dnsRow]));
    fireEvent.click(
      view.getByRole("button", {
        name: t("connections.close", { target: "api.github.com:443" }),
      }),
    );
    // The earlier read still lists the closed connection.
    await act(async () => {
      resolveStale(snapshot([row(), dnsRow]));
      await Promise.resolve();
    });
    expect(targets(container)).toEqual(["1.1.1.1:53"]);
  });

  it("reports a failed close and shows the row again on the next poll", async () => {
    closeConnection.mockRejectedValue("core.api_failed");
    const { container } = render(<Connections />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getAllByTestId("connection-row")).toHaveLength(2);
    });
    fireEvent.click(
      view.getByRole("button", {
        name: t("connections.close", { target: "api.github.com:443" }),
      }),
    );
    await waitFor(() => {
      expect(view.getByRole("alert")).toHaveTextContent("core.api_failed");
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS);
    });
    await waitFor(() => {
      expect(view.getAllByTestId("connection-row")).toHaveLength(2);
    });
  });

  it("asks before closing every connection", async () => {
    const { container } = render(<Connections />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getAllByTestId("connection-row")).toHaveLength(2);
    });
    fireEvent.click(view.getByRole("button", { name: t("connections.closeAll") }));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent(t("connections.closeAllConfirm", { count: 2 }));
    expect(closeAllConnections).not.toHaveBeenCalled();

    listConnections.mockImplementation(async () => snapshot([]));
    fireEvent.click(within(dialog).getByRole("button", { name: t("connections.closeAll") }));
    await waitFor(() => {
      expect(closeAllConnections).toHaveBeenCalledTimes(1);
      expect(view.getByTestId("connections-empty")).toHaveTextContent(t("connections.empty"));
    });
    expect(view.getByRole("button", { name: t("connections.closeAll") })).toBeDisabled();
  });

  it("says so when the core is stopped", async () => {
    listConnections.mockImplementation(async () =>
      snapshot([], { running: false, upload_total: 0, download_total: 0 }),
    );
    const { container } = render(<Connections />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByTestId("connections-empty")).toHaveTextContent(
        t("connections.stopped"),
      );
    });
    expect(view.getByTestId("connections-summary")).toHaveTextContent(
      t("connections.count", { count: 0 }),
    );
    expect(view.getByTestId("connections-summary")).not.toHaveTextContent("↓");
  });

  it("shows a read error and keeps the last list", async () => {
    const { container } = render(<Connections />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getAllByTestId("connection-row")).toHaveLength(2);
    });
    listConnections.mockRejectedValueOnce("core.api_failed");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS);
    });
    await waitFor(() => {
      expect(view.getByRole("alert")).toHaveTextContent("core.api_failed");
    });
    expect(view.getAllByTestId("connection-row")).toHaveLength(2);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS);
    });
    await waitFor(() => {
      expect(view.queryByRole("alert")).toBeNull();
    });
  });

  it("does not poll while inactive or while the window is hidden", async () => {
    const { rerender } = render(<Connections active={false} />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS * 3);
    });
    expect(listConnections).not.toHaveBeenCalled();

    rerender(<Connections active />);
    await waitFor(() => {
      expect(listConnections).toHaveBeenCalledTimes(1);
    });
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS * 3);
    });
    expect(listConnections).toHaveBeenCalledTimes(1);

    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS);
    });
    expect(listConnections.mock.calls.length).toBeGreaterThan(1);
  });

  it("waits for a slow read instead of stacking another one", async () => {
    let resolveSlow: (value: ConnectionsResponse) => void = () => {};
    listConnections.mockImplementation(
      () =>
        new Promise<ConnectionsResponse>((resolve) => {
          resolveSlow = resolve;
        }),
    );
    const { container } = render(<Connections />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_MS * 5);
    });
    expect(listConnections).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveSlow(snapshot([row()]));
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(targets(container)).toEqual(["api.github.com:443"]);
    });
  });

  it("draws a bounded number of rows and says the list is cut", async () => {
    const many = Array.from({ length: 320 }, (_, i) =>
      row({ id: `c-${i}`, host: `host-${i}.example`, start_ms: NOW - i * 1000 }),
    );
    listConnections.mockImplementation(async () => snapshot(many));
    const { container } = render(<Connections />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getAllByTestId("connection-row")).toHaveLength(300);
    });
    expect(container).toHaveTextContent(t("connections.truncated", { limit: 300 }));
    expect(view.getByTestId("connections-summary")).toHaveTextContent(
      t("connections.count", { count: 320 }),
    );
  });
});
