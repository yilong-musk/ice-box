// SPDX-License-Identifier: GPL-3.0-or-later

import { act, fireEvent, render, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { McpStatus } from "../api/contracts";
import { t } from "../lib/i18n";
import { Mcp } from "./Mcp";

const getMcpStatus = vi.fn();
const setMcpEnabled = vi.fn();
const restartMcp = vi.fn();
const copyText = vi.fn();

vi.mock("../api/client", () => ({
  api: {
    getMcpStatus: (...args: unknown[]) => getMcpStatus(...args),
    setMcpEnabled: (...args: unknown[]) => setMcpEnabled(...args),
    restartMcp: (...args: unknown[]) => restartMcp(...args),
  },
  formatInvokeError: (error: unknown) => String(error),
}));

vi.mock("../lib/logCopy", () => ({
  copyText: (...args: unknown[]) => copyText(...args),
}));

const clientConfig = [
  "{",
  '  "mcpServers": {',
  '    "ice-box": {',
  '      "command": "/Applications/ice-box.app/Contents/MacOS/ice-box",',
  '      "args": ["--mcp-stdio", "--data-dir", "/tmp/ice-box"]',
  "    }",
  "  }",
  "}",
].join("\n");

function status(overrides: Partial<McpStatus> = {}): McpStatus {
  return {
    supported: true,
    enabled: true,
    running: true,
    endpoint: "127.0.0.1:4567",
    client_count: 1,
    tool_count: 29,
    bridge_executable: "/Applications/ice-box.app/Contents/MacOS/ice-box",
    bridge_args: ["--mcp-stdio", "--data-dir", "/tmp/ice-box"],
    client_config: clientConfig,
    error: null,
    ...overrides,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

describe("Mcp", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getMcpStatus.mockResolvedValue(status());
    setMcpEnabled.mockResolvedValue(status());
    restartMcp.mockResolvedValue(status());
    copyText.mockResolvedValue(undefined);
  });

  it("loads runtime status and the Agent configuration", async () => {
    const view = within(render(<Mcp />).container);

    await waitFor(() => {
      expect(view.getByText("127.0.0.1:4567")).toBeInTheDocument();
    });
    expect(view.getByRole("switch", { name: t("mcp.enable") })).toBeChecked();
    expect(view.getByText(t("mcp.running"))).toBeInTheDocument();
    expect(view.getByText("29")).toBeInTheDocument();
    expect(view.getByTestId("mcp-client-config")).toHaveTextContent(
      '"mcpServers"',
    );
  });

  it("starts and stops the shared service through the switch", async () => {
    getMcpStatus.mockResolvedValue(status({ enabled: false, running: false, endpoint: null }));
    setMcpEnabled
      .mockResolvedValueOnce(status())
      .mockResolvedValueOnce(status({ enabled: false, running: false, endpoint: null }));
    const view = within(render(<Mcp />).container);
    const toggle = await view.findByRole("switch", { name: t("mcp.enable") });
    expect(toggle).not.toBeChecked();

    fireEvent.click(toggle);
    await waitFor(() => expect(setMcpEnabled).toHaveBeenLastCalledWith(true));
    expect(toggle).toBeChecked();

    fireEvent.click(toggle);
    await waitFor(() => expect(setMcpEnabled).toHaveBeenLastCalledWith(false));
    expect(toggle).not.toBeChecked();
  });

  it("does not let an older status poll overwrite a completed toggle", async () => {
    const stale = deferred<McpStatus>();
    getMcpStatus.mockResolvedValue(
      status({ enabled: false, running: false, endpoint: null }),
    );
    setMcpEnabled.mockResolvedValue(status());
    const view = within(render(<Mcp />).container);
    const toggle = await view.findByRole("switch", { name: t("mcp.enable") });

    getMcpStatus.mockReturnValueOnce(stale.promise);
    await waitFor(() => expect(getMcpStatus).toHaveBeenCalledTimes(2), {
      timeout: 3000,
    });

    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toBeChecked());

    await act(async () => {
      stale.resolve(status({ enabled: false, running: false, endpoint: null }));
      await stale.promise;
    });

    expect(toggle).toBeChecked();
  });

  it("does not let a poll started during an action overwrite its result", async () => {
    const action = deferred<McpStatus>();
    const staleDuringAction = deferred<McpStatus>();
    getMcpStatus.mockResolvedValue(
      status({ enabled: false, running: false, endpoint: null }),
    );
    setMcpEnabled.mockReturnValue(action.promise);
    const view = within(render(<Mcp />).container);
    const toggle = await view.findByRole("switch", { name: t("mcp.enable") });

    getMcpStatus.mockReturnValueOnce(staleDuringAction.promise);
    fireEvent.click(toggle);
    await waitFor(() => expect(setMcpEnabled).toHaveBeenCalledWith(true));
    await waitFor(() => expect(getMcpStatus).toHaveBeenCalledTimes(2), {
      timeout: 3000,
    });

    await act(async () => {
      action.resolve(status());
      await action.promise;
    });
    await act(async () => {
      staleDuringAction.resolve(
        status({ enabled: false, running: false, endpoint: null }),
      );
      await staleDuringAction.promise;
    });

    expect(toggle).toBeChecked();
  });

  it("restarts a running service", async () => {
    restartMcp.mockResolvedValue(status({ client_count: 0 }));
    const view = within(render(<Mcp />).container);
    const restart = await view.findByRole("button", { name: t("mcp.restart") });

    await waitFor(() => expect(restart).toBeEnabled());
    fireEvent.click(restart);
    await waitFor(() => expect(restartMcp).toHaveBeenCalledOnce());
    expect(view.getByText("0")).toBeInTheDocument();
  });

  it("copies the client configuration", async () => {
    const view = within(render(<Mcp />).container);
    await view.findByTestId("mcp-client-config");

    fireEvent.click(
      view.getByRole("button", { name: t("mcp.copyConfig") }),
    );
    await waitFor(() => expect(copyText).toHaveBeenCalledWith(clientConfig));
    expect(
      view.getByRole("button", { name: t("mcp.copied") }),
    ).toBeInTheDocument();
  });

  it("rolls back to the persisted state and keeps an action error visible", async () => {
    getMcpStatus.mockResolvedValue(status({ enabled: false, running: false, endpoint: null }));
    setMcpEnabled.mockRejectedValue(new Error("persist failed"));
    const view = within(render(<Mcp />).container);
    const toggle = await view.findByRole("switch", { name: t("mcp.enable") });

    fireEvent.click(toggle);
    await waitFor(() => {
      expect(view.getByRole("alert")).toHaveTextContent("persist failed");
      expect(toggle).not.toBeChecked();
    });
    expect(getMcpStatus.mock.calls.length).toBeGreaterThanOrEqual(2);
  });

  it("renders unsupported platforms read-only", async () => {
    getMcpStatus.mockResolvedValue(
      status({
        supported: false,
        enabled: false,
        running: false,
        endpoint: null,
        tool_count: 0,
        client_config: "",
      }),
    );
    const view = within(render(<Mcp />).container);

    await waitFor(() => {
      expect(view.getByText(t("mcp.unavailable"))).toBeInTheDocument();
    });
    expect(view.getByRole("switch", { name: t("mcp.enable") })).toBeDisabled();
    expect(view.getByRole("button", { name: t("mcp.copyConfig") })).toBeDisabled();
  });
});
