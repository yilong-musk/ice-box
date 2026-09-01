import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../api/tauri";
import { Subscriptions } from "./Subscriptions";

const listSubscriptions = vi.fn();
const updateAllSubscriptions = vi.fn();
const removeSubscription = vi.fn();
const applySubscriptions = vi.fn();

vi.mock("../api/tauri", () => ({
  api: {
    listSubscriptions: (...args: unknown[]) => listSubscriptions(...args),
    addSubscription: vi.fn(),
    updateAllSubscriptions: (...args: unknown[]) =>
      updateAllSubscriptions(...args),
    updateSubscription: vi.fn(),
    setSubscriptionActive: vi.fn(),
    setSubscriptionAutoUpdate: vi.fn(),
    removeSubscription: (...args: unknown[]) => removeSubscription(...args),
    applySubscriptions: (...args: unknown[]) => applySubscriptions(...args),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

function sampleMeta(overrides: Partial<Record<string, unknown>> = {}) {
  return {
    id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
    name: "sub-a",
    url: "https://example.com/a",
    active: true,
    format: "singbox",
    node_count: 1,
    group_count: 0,
    rule_count: 0,
    has_dns: false,
    parse_warnings: [],
    last_updated: null,
    last_error: null,
    etag: null,
    last_modified: null,
    auto_update: false,
    ...overrides,
  };
}

describe("Subscriptions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listSubscriptions.mockResolvedValue([]);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("renders import form", async () => {
    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(
        view.getByPlaceholderText("订阅 URL（https 优先）"),
      ).toBeInTheDocument();
    });
    expect(view.getByText("暂无订阅")).toBeInTheDocument();
    expect(
      view.getByText(/打开软件会自动启动内核；需要时在主页用大按钮接管系统代理/),
    ).toBeInTheDocument();
    expect(container.querySelector(".sub-list")).toBeNull();
    expect(container.querySelector(".subs-panel")?.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["flex-1", "min-h-0", "flex-col"]),
    );
    expect(
      view.getByRole("button", { name: "导入" }).parentElement?.className.split(/\s+/),
    ).toEqual(expect.arrayContaining(["items-end"]));
  });

  it("shows partial update failures from updateAllSubscriptions", async () => {
    listSubscriptions.mockResolvedValue([sampleMeta()]);
    updateAllSubscriptions.mockResolvedValue({
      results: [
        {
          id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
          ok: false,
          error: "network down",
        },
      ],
    });

    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("sub-a")).toBeInTheDocument();
    });

    view.getByRole("button", { name: "全部更新" }).click();

    await waitFor(() => {
      expect(view.getByText(/部分订阅更新失败/)).toBeInTheDocument();
      expect(view.getByText(/network down/)).toBeInTheDocument();
    });
  });

  it("shows 更新中 on update buttons while updating", async () => {
    listSubscriptions.mockResolvedValue([sampleMeta()]);
    let resolveUpdate: (v: unknown) => void = () => {};
    updateAllSubscriptions.mockReturnValue(
      new Promise((resolve) => {
        resolveUpdate = resolve;
      }),
    );

    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("sub-a")).toBeInTheDocument();
    });

    view.getByRole("button", { name: "全部更新" }).click();

    await waitFor(() => {
      const updatingButtons = view.getAllByRole("button", { name: "更新中" });
      expect(updatingButtons.length).toBeGreaterThanOrEqual(2);
      for (const btn of updatingButtons) {
        expect(btn).toBeDisabled();
      }
    });

    resolveUpdate({ results: [] });
    await waitFor(() => {
      expect(view.getByRole("button", { name: "全部更新" })).toBeInTheDocument();
    });
  });

  it("shows an apply warning from a remove response", async () => {
    listSubscriptions.mockResolvedValue([sampleMeta({ name: "only-one" })]);
    removeSubscription.mockResolvedValue({
      ok: true,
      apply_warning: {
        code: "proxy.restore_failed",
        message: "内核已重载，但系统代理未能恢复",
      },
    });

    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("only-one")).toBeInTheDocument();
    });

    fireEvent.click(view.getByRole("button", { name: "删除" }));

    await waitFor(() => {
      expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    });
    fireEvent.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
        name: "删除",
      }),
    );

    await waitFor(() => {
      expect(removeSubscription).toHaveBeenCalled();
      expect(view.getByText(/系统代理未能恢复/)).toBeInTheDocument();
    });
  });

  it("renders apply subscriptions button", async () => {
    listSubscriptions.mockResolvedValue([sampleMeta()]);
    applySubscriptions.mockResolvedValue(undefined);

    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByRole("button", { name: "应用配置" })).toBeInTheDocument();
    });
    const row = view.getByText("sub-a").closest("[data-slot='item']");
    expect(row?.className.split(/\s+/)).toEqual(
      expect.arrayContaining(["px-3", "py-2.5"]),
    );
    expect(row?.className.split(/\s+/)).not.toContain("px-0");

    view.getByRole("button", { name: "应用配置" }).click();
    await waitFor(() => {
      expect(applySubscriptions).toHaveBeenCalled();
    });
  });

  it("shows group/rule/dns stats and parse warnings", async () => {
    listSubscriptions.mockResolvedValue([
      sampleMeta({
        name: "flower",
        group_count: 21,
        rule_count: 4270,
        has_dns: true,
        parse_warnings: ["GEOIP 规则已跳过", "未知组引用 x"],
      }),
    ]);

    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("flower")).toBeInTheDocument();
    });
    expect(view.getByText(/21 策略组/)).toBeInTheDocument();
    expect(view.getByText(/4270 规则/)).toBeInTheDocument();
    expect(view.getByText(/· DNS/)).toBeInTheDocument();
    expect(view.getByText(/GEOIP 规则已跳过/)).toBeInTheDocument();
  });

  it("renders legacy payloads without new fields (stale backend)", async () => {
    listSubscriptions.mockResolvedValue([
      {
        id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        name: "legacy",
        url: "https://example.com/a",
        enabled: true,
        format: "clash",
        node_count: 5,
        last_updated: null,
        last_error: null,
        etag: null,
        last_modified: null,
      },
    ]);

    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("legacy")).toBeInTheDocument();
    });
    expect(view.getByText(/5 节点/)).toBeInTheDocument();
  });

  it("imports with auto-update when the import switch is on", async () => {
    listSubscriptions.mockResolvedValue([]);
    const add = vi.mocked(api.addSubscription).mockResolvedValue({
      ...sampleMeta(),
      auto_update: true,
    });

    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByLabelText("自动更新")).toBeInTheDocument();
    });
    const importSwitch = view.getByLabelText("自动更新");
    expect(importSwitch).not.toBeChecked();
    fireEvent.click(importSwitch);
    expect(importSwitch).toBeChecked();

    fireEvent.change(
      view.getByPlaceholderText("订阅 URL（https 优先）"),
      { target: { value: "https://example.com/new" } },
    );
    fireEvent.click(view.getByRole("button", { name: "导入" }));

    await waitFor(() => {
      expect(add).toHaveBeenCalledWith(
        "https://example.com/new",
        undefined,
        true,
      );
    });
    await waitFor(() => {
      expect(importSwitch).not.toBeChecked();
    });
  });

  it("toggles auto-update per subscription via setSubscriptionAutoUpdate", async () => {
    const setAutoUpdate = vi.fn().mockResolvedValue({});
    vi.mocked(api.setSubscriptionAutoUpdate).mockImplementation(setAutoUpdate);
    listSubscriptions.mockResolvedValue([
      sampleMeta({ name: "a", auto_update: true, id: "11111111-1111-1111-1111-111111111111" }),
      sampleMeta({ name: "b", auto_update: false, id: "22222222-2222-2222-2222-222222222222" }),
    ]);

    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("b")).toBeInTheDocument();
    });
    const rowB = view.getByText("b").closest("[data-slot=item]") as HTMLElement;
    const switches = within(rowB).getAllByRole("switch", {
      name: "自动更新",
    });
    expect(switches).toHaveLength(1);
    expect(switches[0]).not.toBeChecked();
    fireEvent.click(switches[0]);
    await waitFor(() => {
      expect(setAutoUpdate).toHaveBeenCalledWith(
        "22222222-2222-2222-2222-222222222222",
        true,
      );
    });

    const rowA = view.getByText("a").closest("[data-slot=item]") as HTMLElement;
    expect(within(rowA).getAllByRole("switch", { name: "自动更新" })[0]).toBeChecked();
  });

  it("marks active subscription and switches via setSubscriptionActive", async () => {
    const setActive = vi.fn().mockResolvedValue({});
    vi.mocked(api.setSubscriptionActive).mockImplementation(setActive);
    listSubscriptions.mockResolvedValue([
      sampleMeta({ name: "a", active: true, id: "11111111-1111-1111-1111-111111111111" }),
      sampleMeta({ name: "b", active: false, id: "22222222-2222-2222-2222-222222222222" }),
    ]);

    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("b")).toBeInTheDocument();
    });
    const rowB = view.getByText("b").closest("[data-slot=item]") as HTMLElement;
    const toggle = within(rowB).getByRole("switch", { name: "激活" });
    expect(toggle).not.toBeChecked();
    toggle.click();
    await waitFor(() => {
      expect(setActive).toHaveBeenCalledWith(
        "22222222-2222-2222-2222-222222222222",
        true,
      );
    });
  });
});
