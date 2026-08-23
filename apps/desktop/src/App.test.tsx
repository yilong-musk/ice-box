import { render, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

const getStatus = vi.fn();

vi.mock("./api/tauri", () => ({
  api: {
    getStatus: (...args: unknown[]) => getStatus(...args),
    listNodes: vi.fn().mockResolvedValue([]),
    getSettings: vi.fn().mockResolvedValue({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: null,
      auto_set_system_proxy: false,
      allow_lan: false,
      proxy_mode: "rule",
    }),
    getConnectionStats: vi.fn().mockResolvedValue({ connection_count: 0 }),
    getTrafficSample: vi.fn().mockResolvedValue({ up: 0, down: 0 }),
    start: vi.fn(),
    stop: vi.fn(),
    listSubscriptions: vi.fn().mockResolvedValue([]),
    getLogView: vi.fn().mockResolvedValue([]),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

describe("App", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getStatus.mockResolvedValue({
      core: {
        status: "stopped",
        message: null,
        inbound_host: null,
        inbound_port: null,
      },
      subscription_count: 0,
      proxy_recovery_warning: null,
      system_proxy_applied: null,
    });
  });

  it("shows proxy recovery warning globally on any tab", async () => {
    getStatus.mockResolvedValue({
      core: {
        status: "error",
        message: "sing-box exited unexpectedly (code 1)",
        inbound_host: null,
        inbound_port: null,
      },
      subscription_count: 1,
      proxy_recovery_warning: "sing-box 意外退出后系统代理恢复失败: mock",
      system_proxy_applied: null,
    });

    const { container } = render(<App />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("alert")).toHaveTextContent(
        "sing-box 意外退出后系统代理恢复失败",
      );
    });
  });
});
