// SPDX-License-Identifier: GPL-3.0-or-later

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { THEME_STORAGE_KEY } from "../lib/theme";
import {
  CHANGED_EVENT,
  HIDE_EVENT,
  SHOW_EVENT,
  type PanelLayout,
  type PanelModel,
  type PanelPlacement,
} from "./api";

type Listener = (event: { payload: unknown }) => void;

const listeners = new Map<string, Listener>();
const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, cb: Listener) => {
    listeners.set(event, cb);
    return () => {
      listeners.delete(event);
    };
  }),
}));

import { TrayPanel } from "./TrayPanel";

const PLACEMENT: PanelPlacement = {
  seq: 1,
  scaleFactor: 1.25,
  side: "left",
  edge: "bottom",
  boundsTop: -820,
  boundsBottom: 0,
  mainWidth: 228,
  modeWidth: 152,
  subsWidth: 228,
  subWidth: 272,
  leafWidth: 264,
  gap: 4,
  radius: 10,
};

function makeModel(overrides: Partial<PanelModel> = {}): PanelModel {
  return {
    update: { id: "update", label: "有新版本 0.4.2", enabled: true },
    service: { id: "service", label: "停止代理服务", enabled: true },
    mode: { label: "代理模式", enabled: true },
    modeItems: [
      { id: "mode:rule", label: "规则", enabled: true, checked: true },
      { id: "mode:global", label: "全局", enabled: true, checked: false },
      { id: "mode:direct", label: "直连", enabled: true, checked: false },
    ],
    nodes: { label: "节点", enabled: true },
    nodesDelay: {
      id: '["delay","top"]',
      label: "延迟测试",
      enabled: true,
    },
    nodeEntries: [
      {
        kind: "group",
        tag: "节点选择",
        label: "节点选择 → 日本 02",
        delay: null,
        delayButton: {
          id: '["delay","group","节点选择"]',
          label: "延迟测试",
          enabled: true,
        },
        members: [
          {
            id: '["member","节点选择","香港 01"]',
            label: "香港 01",
            enabled: true,
            checked: false,
            delay: null,
          },
          {
            id: '["member","节点选择","日本 02"]',
            label: "日本 02",
            enabled: true,
            checked: true,
            delay: null,
          },
        ],
      },
    ],
    subs: { label: "订阅", enabled: true },
    subItems: [
      {
        id: '["sub","11111111-1111-4111-8111-111111111111"]',
        label: "机场 A",
        enabled: false,
        checked: true,
      },
      {
        id: '["sub","22222222-2222-4222-8222-222222222222"]',
        label: "机场 B",
        enabled: true,
        checked: false,
      },
    ],
    copyCliProxy: {
      id: "copy-cli-proxy",
      label: "复制代理命令",
      enabled: true,
    },
    openCliProxy: {
      id: "open-cli-proxy",
      label: "打开代理终端",
      enabled: true,
    },
    mcp: { id: "mcp-service", label: "启动 MCP 服务", enabled: true },
    copyMcpConfig: {
      id: "copy-mcp-config",
      label: "复制 MCP 配置",
      enabled: false,
    },
    show: { id: "show", label: "显示", enabled: true },
    quit: { id: "quit", label: "退出", enabled: true },
    ...overrides,
  };
}

function mockCardGeometry() {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      if (this.dataset.testid === "tray-panel-main") {
        return {
          x: 544,
          y: 440,
          left: 544,
          top: 440,
          right: 772,
          bottom: 760,
          width: 228,
          height: 320,
          toJSON: () => ({}),
        } as DOMRect;
      }
      if (this.dataset.testid?.startsWith("tray-panel-sub-")) {
        return {
          x: 268,
          y: 460,
          left: 268,
          top: 460,
          right: 540,
          bottom: 580,
          width: 272,
          height: 120,
          toJSON: () => ({}),
        } as DOMRect;
      }
      if (this.dataset.testid === "tray-panel-group") {
        return {
          x: 0,
          y: 480,
          left: 0,
          top: 480,
          right: 264,
          bottom: 600,
          width: 264,
          height: 120,
          toJSON: () => ({}),
        } as DOMRect;
      }
      return {
        x: 0,
        y: 480,
        left: 0,
        top: 480,
        right: 220,
        bottom: 505,
        width: 220,
        height: 25,
        toJSON: () => ({}),
      } as DOMRect;
    },
  );
}

describe("TrayPanel", () => {
  let currentModel: PanelModel;

  beforeEach(() => {
    listeners.clear();
    invokeMock.mockReset();
    currentModel = makeModel();
    mockCardGeometry();
    vi.stubGlobal("devicePixelRatio", 1.25);
    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
      cb(0);
      return 1;
    });
    vi.stubGlobal("cancelAnimationFrame", () => {});

    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "tray_panel_state") return currentModel;
      if (cmd === "tray_panel_placement") return null;
      return undefined;
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders macOS-aligned menu items and reports the initial single-card layout", async () => {
    window.localStorage.setItem(THEME_STORAGE_KEY, "dark");
    render(<TrayPanel />);

    await waitFor(() => expect(listeners.has(SHOW_EVENT)).toBe(true));
    await act(async () => {
      listeners.get(SHOW_EVENT)?.({ payload: PLACEMENT });
    });

    expect(await screen.findByText("有新版本 0.4.2")).toBeInTheDocument();
    expect(screen.getByText("停止代理服务")).toBeInTheDocument();
    expect(screen.getByText("代理模式")).toBeInTheDocument();
    expect(screen.getByText("节点")).toBeInTheDocument();
    expect(screen.getByText("订阅")).toBeInTheDocument();
    expect(screen.getByText("显示")).toBeInTheDocument();
    expect(screen.getByText("退出")).toBeInTheDocument();
    expect(document.documentElement.classList.contains("dark")).toBe(true);

    const layoutCall = invokeMock.mock.calls.find(
      ([cmd]) => cmd === "tray_panel_layout",
    );
    expect(layoutCall).toBeDefined();
    const { layout } = layoutCall![1] as { layout: PanelLayout };
    expect(layout.seq).toBe(1);
    expect(layout.dark).toBe(true);
    expect(layout.cards).toHaveLength(1);
    expect(layout.cards[0].width).toBe(228);
  });

  it("cascades nodes and group members across three cards and streams delay results", async () => {
    render(<TrayPanel />);
    await waitFor(() => expect(listeners.has(SHOW_EVENT)).toBe(true));
    await act(async () => {
      listeners.get(SHOW_EVENT)?.({ payload: PLACEMENT });
    });

    const nodesTrigger = await screen.findByRole("menuitem", { name: "节点" });
    fireEvent.click(nodesTrigger);
    expect(screen.getByTestId("tray-panel-sub-nodes")).toBeInTheDocument();

    const groupTrigger = screen.getByRole("menuitem", {
      name: /节点选择 → 日本 02/,
    });
    fireEvent.click(groupTrigger);
    expect(screen.getByTestId("tray-panel-group")).toBeInTheDocument();

    // Three cascading cards are reported to Win32 SetWindowRgn.
    const layouts = invokeMock.mock.calls
      .filter(([cmd]) => cmd === "tray_panel_layout")
      .map(([, args]) => (args as { layout: PanelLayout }).layout);
    expect(layouts[layouts.length - 1].cards).toHaveLength(3);

    // Click group delay button and stream a live result via tray-panel://changed.
    const groupButtons = screen.getAllByRole("menuitem", { name: "延迟测试" });
    fireEvent.click(groupButtons[1]);
    expect(invokeMock).toHaveBeenCalledWith("tray_panel_action", {
      id: '["delay","group","节点选择"]',
    });

    currentModel = makeModel({
      nodesDelay: {
        id: '["delay","top"]',
        label: "延迟测试",
        enabled: false,
      },
      nodeEntries: [
        {
          kind: "group",
          tag: "节点选择",
          label: "节点选择 → 日本 02",
          delay: { text: "失败", tone: "bad" },
          delayButton: {
            id: '["delay","group","节点选择"]',
            label: "延迟测试中 1/2",
            enabled: false,
          },
          members: [
            {
              id: '["member","节点选择","香港 01"]',
              label: "香港 01",
              enabled: true,
              checked: false,
              delay: { text: "45 ms", tone: "ok" },
            },
            {
              id: '["member","节点选择","日本 02"]',
              label: "日本 02",
              enabled: true,
              checked: true,
              delay: { text: "失败", tone: "bad" },
            },
          ],
        },
      ],
    });

    await act(async () => {
      listeners.get(CHANGED_EVENT)?.({ payload: undefined });
    });

    expect(await screen.findByText("延迟测试中 1/2")).toBeInTheDocument();
    expect(screen.getByText(/45 ms/)).toHaveAttribute("data-tone", "ok");
    expect(screen.getAllByText(/失败/)[0]).toHaveAttribute("data-tone", "bad");

    // Pick a member node and verify it forwards the native JSON action id.
    fireEvent.click(screen.getByRole("menuitemradio", { name: /香港 01/ }));
    expect(invokeMock).toHaveBeenCalledWith("tray_panel_action", {
      id: '["member","节点选择","香港 01"]',
    });
  });

  it("steps back through open submenus on Escape and hides when at root", async () => {
    render(<TrayPanel />);
    await waitFor(() => expect(listeners.has(SHOW_EVENT)).toBe(true));
    await act(async () => {
      listeners.get(SHOW_EVENT)?.({ payload: PLACEMENT });
    });

    fireEvent.click(await screen.findByRole("menuitem", { name: "节点" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /节点选择/ }));
    expect(screen.getByTestId("tray-panel-group")).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("tray-panel-group")).not.toBeInTheDocument();
    expect(screen.getByTestId("tray-panel-sub-nodes")).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(
      screen.queryByTestId("tray-panel-sub-nodes"),
    ).not.toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(invokeMock).toHaveBeenCalledWith("tray_panel_hide");

    await act(async () => {
      listeners.get(HIDE_EVENT)?.({ payload: undefined });
    });
  });

  it("keeps tray-panel.html inline theme script aligned with the Tauri CSP hash", () => {
    const desktopRoot = path.resolve(__dirname, "../..");
    const html = readFileSync(
      path.join(desktopRoot, "tray-panel.html"),
      "utf8",
    );
    const tauriConf = JSON.parse(
      readFileSync(
        path.join(desktopRoot, "src-tauri/tauri.conf.json"),
        "utf8",
      ),
    ) as { app: { security: { csp: string } } };
    const match = html.match(/<script>([\s\S]*?)<\/script>/);
    expect(match).not.toBeNull();
    const digest = createHash("sha256").update(match![1], "utf8").digest("base64");
    expect(tauriConf.app.security.csp).toContain(`'sha256-${digest}'`);
  });

  it("ignores a cold-open state resolution if HIDE_EVENT arrives mid-load", async () => {
    let resolveState!: (model: PanelModel) => void;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "tray_panel_state") {
        return new Promise<PanelModel>((resolve) => {
          resolveState = resolve;
        });
      }
      if (cmd === "tray_panel_placement") return Promise.resolve(null);
      return Promise.resolve(undefined);
    });

    render(<TrayPanel />);
    await waitFor(() => expect(listeners.has(SHOW_EVENT)).toBe(true));

    await act(async () => {
      listeners.get(SHOW_EVENT)?.({ payload: PLACEMENT });
    });
    await act(async () => {
      listeners.get(HIDE_EVENT)?.({ payload: undefined });
    });
    await act(async () => {
      resolveState(currentModel);
      await Promise.resolve();
    });

    expect(
      invokeMock.mock.calls.filter(([cmd]) => cmd === "tray_panel_layout"),
    ).toHaveLength(0);
  });

  it("hides the panel if the initial cold state fetch fails", async () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "tray_panel_state") {
        return Promise.reject(new Error("boom"));
      }
      if (cmd === "tray_panel_placement") return Promise.resolve(null);
      return Promise.resolve(undefined);
    });

    render(<TrayPanel />);
    await waitFor(() => expect(listeners.has(SHOW_EVENT)).toBe(true));

    await act(async () => {
      listeners.get(SHOW_EVENT)?.({ payload: PLACEMENT });
    });

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("tray_panel_hide"),
    );
    errorSpy.mockRestore();
  });

  it("strips cached delay badges on hide so warm reopen does not flash stale results", async () => {
    currentModel = makeModel({
      nodeEntries: [
        {
          kind: "group",
          tag: "节点选择",
          label: "节点选择 → 日本 02",
          delay: { text: "45 ms", tone: "ok" },
          delayButton: {
            id: '["delay","group","节点选择"]',
            label: "延迟测试",
            enabled: true,
          },
          members: [
            {
              id: '["member","节点选择","香港 01"]',
              label: "香港 01",
              enabled: true,
              checked: false,
              delay: { text: "45 ms", tone: "ok" },
            },
          ],
        },
      ],
    });

    render(<TrayPanel />);
    await waitFor(() => expect(listeners.has(SHOW_EVENT)).toBe(true));
    await act(async () => {
      listeners.get(SHOW_EVENT)?.({ payload: PLACEMENT });
    });
    fireEvent.click(await screen.findByRole("menuitem", { name: "节点" }));
    expect(screen.getByText(/45 ms/)).toBeInTheDocument();

    await act(async () => {
      listeners.get(HIDE_EVENT)?.({ payload: undefined });
    });

    // Simulate a slow state fetch (> WARM_DATA_WAIT_MS) on reopen.
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "tray_panel_state") return new Promise(() => {});
      if (cmd === "tray_panel_placement") return Promise.resolve(null);
      return Promise.resolve(undefined);
    });
    await act(async () => {
      listeners.get(SHOW_EVENT)?.({ payload: { ...PLACEMENT, seq: 2 } });
      await new Promise((resolve) => setTimeout(resolve, 70));
    });

    fireEvent.click(await screen.findByRole("menuitem", { name: "节点" }));
    expect(screen.queryByText(/45 ms/)).not.toBeInTheDocument();
  });
});
