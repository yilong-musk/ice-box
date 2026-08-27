import { fireEvent, render, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { APP_VERSION } from "./lib/appVersion";

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
    getTrafficSnapshot: vi.fn().mockResolvedValue({ points: [], latest: null }),
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
    expect(logView!.parentElement).toBe(panel);
    expect(view.queryByRole("button", { name: "刷新" })).toBeNull();
    expect(panel!.querySelector("[data-slot='card']")).toBeNull();

    fireEvent.click(view.getByRole("button", { name: "主页" }));
    expect(container.querySelector(".home-panel")).not.toBeNull();
    expect(container.querySelector("main")?.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["content-fill", "overflow-hidden"]),
    );

    fireEvent.click(view.getByRole("button", { name: "节点" }));
    expect(container.querySelector(".nodes-panel")).not.toBeNull();
    expect(container.querySelector("main")?.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["content-fill", "overflow-hidden"]),
    );

    fireEvent.click(view.getByRole("button", { name: "规则" }));
    expect(container.querySelector(".rules-panel")).not.toBeNull();
    expect(container.querySelector("main")?.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["content-fill", "overflow-hidden"]),
    );

    fireEvent.click(view.getByRole("button", { name: "订阅" }));
    expect(container.querySelector(".subs-panel")).not.toBeNull();
    expect(container.querySelector("main")?.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["content-fill", "overflow-hidden"]),
    );

    fireEvent.click(view.getByRole("button", { name: "设置" }));
    expect(container.querySelector(".settings-panel")).not.toBeNull();
    expect(container.querySelector("main")?.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["content-fill", "overflow-hidden"]),
    );
  });

  it("applies appearance changes from settings to the document", async () => {
    const { container } = render(<App />);
    const view = within(container);
    fireEvent.click(view.getByRole("button", { name: "设置" }));

    await waitFor(() => {
      expect(view.getByRole("radio", { name: "跟随系统" })).toBeInTheDocument();
    });

    fireEvent.click(view.getByRole("radio", { name: "深色" }));
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    fireEvent.click(view.getByRole("radio", { name: "浅色" }));
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    fireEvent.click(view.getByRole("radio", { name: "跟随系统" }));
    expect(view.getByRole("radio", { name: "跟随系统" })).toHaveAttribute(
      "data-state",
      "on",
    );
  });

  it("places the app brand at the bottom of the sidebar", () => {
    const { container } = render(<App />);
    const aside = container.querySelector("aside");
    expect(aside).not.toBeNull();
    const nav = aside!.querySelector("nav");
    const brand = aside!.querySelector("h1");
    const version = aside!.querySelector('[aria-label^="版本"]');
    expect(nav).not.toBeNull();
    expect(brand).toHaveTextContent("ice-box");
    expect(brand!.parentElement?.className.split(/\s+/)).toContain("justify-center");
    expect(version).toHaveTextContent(APP_VERSION);
    expect(brand!.compareDocumentPosition(version!)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(nav!.compareDocumentPosition(brand!)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(aside!.querySelector('[data-slot="separator"]')).toBeNull();
  });

  it("draws the titlebar divider as one shared edge", () => {
    const { container } = render(<App />);
    const titlebar = container.querySelector("[data-titlebar]");
    expect(titlebar).not.toBeNull();
    expect(titlebar!.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["flex", "h-12", "border-b"]),
    );
    expect(titlebar!.querySelector("h2")).toHaveTextContent("主页");
    const aside = container.querySelector("aside");
    expect(aside).not.toBeNull();
    expect(titlebar!.compareDocumentPosition(aside!)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
  });

  it("keeps overlay drag regions in the sidebar chrome and page header", () => {
    const { container } = render(<App />);
    const regions = container.querySelectorAll("[data-tauri-drag-region]");
    expect(regions.length).toBe(3);
    expect(regions[1]).toHaveTextContent("主页");
    expect(regions[2]).toHaveTextContent("ice-box");
    expect(regions[2]).toHaveTextContent(APP_VERSION);
  });
});
