import { render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
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
    ...overrides,
  };
}

describe("Subscriptions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listSubscriptions.mockResolvedValue([]);
  });

  it("renders import form", async () => {
    const { container } = render(<Subscriptions />);
    await waitFor(() => {
      expect(
        within(container).getByPlaceholderText("订阅 URL（https 优先）"),
      ).toBeInTheDocument();
    });
    expect(screen.queryByText("暂无订阅。导入 URL 后可启动内核。")).toBeInTheDocument();
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

  it("shows apply warning when remove stops core", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
    listSubscriptions.mockResolvedValue([
      sampleMeta({ name: "only-one" }),
    ]);
    removeSubscription.mockResolvedValue({
      ok: true,
      apply_warning: {
        code: "core.stopped_no_nodes",
        message: "内核已停止：没有可用的订阅节点",
      },
    });

    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("only-one")).toBeInTheDocument();
    });

    view.getByRole("button", { name: "删除" }).click();

    await waitFor(() => {
      expect(view.getByText(/core.stopped_no_nodes/)).toBeInTheDocument();
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
    const rowB = view.getByText("b").closest("li") as HTMLElement;
    const toggle = within(rowB).getByRole("checkbox");
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
