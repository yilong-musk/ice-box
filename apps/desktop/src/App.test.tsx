import { fireEvent, render, waitFor, within } from "@testing-library/react";
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
      system_proxy_recorded: null,
      system_proxy_available: true,
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
      system_proxy_recorded: null,
      system_proxy_available: true,
    });

    const { container } = render(<App />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("alert")).toHaveTextContent(
        "sing-box 意外退出后系统代理恢复失败",
      );
    });
  });

  it("lets the logs panel fill the content pane", async () => {
    const { container } = render(<App />);
    const view = within(container);
    fireEvent.click(view.getByRole("button", { name: "日志" }));
    await waitFor(() => {
      expect(container.querySelector(".logs-panel")).not.toBeNull();
    });

    const main = container.querySelector("main");
    const panel = container.querySelector(".logs-panel");
    const logView = container.querySelector(".log-view");
    expect(main).not.toBeNull();
    expect(panel).not.toBeNull();
    expect(logView).not.toBeNull();
    expect(main!.className.split(/\s+/)).toEqual(
      expect.arrayContaining([
        "content-fill",
        "min-h-0",
        "flex-1",
        "overflow-hidden",
      ]),
    );
    expect(main!.className.split(/\s+/)).not.toContain("overflow-auto");
    expect(panel!.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["logs-panel", "min-h-0", "flex-1", "overflow-hidden"]),
    );
    expect(logView!.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["log-view", "min-h-0", "flex-1", "overflow-auto"]),
    );
    expect(main!.contains(panel)).toBe(true);
    expect(panel!.contains(logView)).toBe(true);
    expect(logView!.parentElement?.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["min-h-0", "flex-1", "flex", "flex-col"]),
    );

    fireEvent.click(view.getByRole("button", { name: "主页" }));
    expect(container.querySelector(".content-fill")).toBeNull();
    expect(container.querySelector("main")?.className.split(/\s+/)).toContain(
      "overflow-auto",
    );
  });

  it("applies appearance changes from settings to the document", async () => {
    const { container } = render(<App />);
    const view = within(container);
    fireEvent.click(view.getByRole("button", { name: "设置" }));

    await waitFor(() => {
      expect(view.getByRole("button", { name: "跟随系统" })).toBeInTheDocument();
    });

    fireEvent.click(view.getByRole("button", { name: "深色" }));
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    fireEvent.click(view.getByRole("button", { name: "浅色" }));
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    fireEvent.click(view.getByRole("button", { name: "跟随系统" }));
    expect(view.getByRole("button", { name: "跟随系统" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });
});
