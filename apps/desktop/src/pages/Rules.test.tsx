import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Rules } from "./Rules";

const getRuleOverview = vi.fn();
const listRules = vi.fn();
const setRuleDisabled = vi.fn();
const addCustomRule = vi.fn();
const removeCustomRule = vi.fn();
const listNodes = vi.fn();

vi.mock("../api/tauri", () => ({
  api: {
    getRuleOverview: (...args: unknown[]) => getRuleOverview(...args),
    listRules: (...args: unknown[]) => listRules(...args),
    setRuleDisabled: (...args: unknown[]) => setRuleDisabled(...args),
    addCustomRule: (...args: unknown[]) => addCustomRule(...args),
    removeCustomRule: (...args: unknown[]) => removeCustomRule(...args),
    listNodes: (...args: unknown[]) => listNodes(...args),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

function sampleOverview(overrides: Partial<Record<string, unknown>> = {}) {
  return {
    total: 3,
    disabled: 1,
    custom: 1,
    rule_sets: 2,
    types: [
      { rule_type: "domain_suffix", count: 2 },
      { rule_type: "geoip", count: 1 },
    ],
    ...overrides,
  };
}

function sampleList(overrides: Partial<Record<string, unknown>> = {}) {
  return {
    total: 3,
    offset: 0,
    limit: 50,
    items: [
      {
        index: 0,
        fingerprint: "fp-1",
        rule: { domain_suffix: ["youtube.com"], outbound: "n1" },
        custom: false,
        disabled: false,
        rule_type: "domain_suffix",
      },
      {
        index: null,
        fingerprint: "fp-custom",
        rule: { domain: ["example.com"], outbound: "block" },
        custom: true,
        disabled: false,
        rule_type: "domain",
      },
      {
        index: 2,
        fingerprint: "fp-3",
        rule: { geoip: ["cn"], outbound: "direct" },
        custom: false,
        disabled: true,
        rule_type: "geoip",
      },
    ],
    ...overrides,
  };
}

describe("Rules", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getRuleOverview.mockResolvedValue(sampleOverview());
    listRules.mockResolvedValue(sampleList());
    setRuleDisabled.mockResolvedValue({ ok: true, disabled: true });
    addCustomRule.mockResolvedValue({ ok: true, fingerprint: "fp-new" });
    removeCustomRule.mockResolvedValue({ ok: true });
    listNodes.mockResolvedValue([
      { tag: "n1", outbound_type: "socks", group_now: null, group_all: null },
      { tag: "Proxies", outbound_type: "selector", group_now: "n1", group_all: ["n1"] },
    ]);
  });

  it("renders stats, rows and filters", async () => {
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });
    expect(view.getByText("3", { selector: ".rule-stat strong" })).toBeInTheDocument();
    expect(view.getByText("已禁用 1")).toBeInTheDocument();
    expect(view.getByText("1", { selector: ".rule-stat strong" })).toBeInTheDocument();
    expect(view.getByText("2", { selector: ".rule-stat strong" })).toBeInTheDocument();
    expect(view.getByText("域名后缀 2")).toBeInTheDocument();
    expect(view.getByText("example.com")).toBeInTheDocument();
  });

  it("debounces keyword search and sends server-side filters", async () => {
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });

    view.getByLabelText("搜索规则").focus();
    fireEvent.change(view.getByLabelText("搜索规则"), {
      target: { value: "goo" },
    });

    await waitFor(() => {
      const calls = listRules.mock.calls;
      const call = calls[calls.length - 1]?.[0] as {
        keyword: string | null;
      };
      expect(call.keyword).toBe("goo");
    });
  });

  it("disables a rule and refreshes", async () => {
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });

    const row = view.getByText("youtube.com").closest("li") as HTMLElement;
    within(row).getByRole("button", { name: "禁用" }).click();

    await waitFor(() => {
      expect(setRuleDisabled).toHaveBeenCalledWith("fp-1", true);
    });
    await waitFor(() => {
      expect(getRuleOverview.mock.calls.length).toBeGreaterThanOrEqual(2);
    });
  });

  it("shows apply warning from rule toggle", async () => {
    setRuleDisabled.mockResolvedValue({
      ok: true,
      disabled: true,
      apply_warning: {
        code: "config.invalid",
        message: "bad outbound",
      },
    });
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });

    const row = view.getByText("youtube.com").closest("li") as HTMLElement;
    within(row).getByRole("button", { name: "禁用" }).click();

    await waitFor(() => {
      expect(screen.getByText(/已保存，但应用失败/)).toBeInTheDocument();
      expect(screen.getByText(/config.invalid: bad outbound/)).toBeInTheDocument();
    });
  });

  it("adds a custom rule via interactive form", async () => {
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });

    fireEvent.click(view.getByRole("button", { name: "+ 自定义规则" }));
    fireEvent.change(view.getByLabelText("匹配值"), {
      target: { value: "x.io, y.io" },
    });
    const outbound = view.getByLabelText("出口") as HTMLSelectElement;
    await waitFor(() => {
      expect(
        within(outbound).getByRole("option", { name: "n1" }),
      ).toBeInTheDocument();
    });
    fireEvent.change(outbound, { target: { value: "n1" } });
    view.getByRole("button", { name: "添加" }).click();

    await waitFor(() => {
      expect(addCustomRule).toHaveBeenCalledWith({
        domain_suffix: ["x.io", "y.io"],
        outbound: "n1",
      });
    });
    await waitFor(() => {
      expect(view.queryByLabelText("匹配值")).not.toBeInTheDocument();
    });
  });

  it("lists current nodes/groups as outbound options", async () => {
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });

    fireEvent.click(view.getByRole("button", { name: "+ 自定义规则" }));
    const outbound = view.getByLabelText("出口") as HTMLSelectElement;
    await waitFor(() => {
      expect(
        within(outbound).getByRole("option", { name: "Proxies（策略组）" }),
      ).toBeInTheDocument();
    });
    expect(
      within(outbound).getByRole("option", { name: "direct（直连）" }),
    ).toBeInTheDocument();
  });

  it("shows boolean matcher as checkbox", async () => {
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });

    fireEvent.click(view.getByRole("button", { name: "+ 自定义规则" }));
    fireEvent.change(view.getByLabelText("匹配类型"), {
      target: { value: "ip_is_private" },
    });
    const checkbox = view.getByLabelText("私网 IP") as HTMLInputElement;
    expect(checkbox.type).toBe("checkbox");

    view.getByRole("button", { name: "添加" }).click();
    await waitFor(() => {
      expect(addCustomRule).toHaveBeenCalledWith({
        ip_is_private: true,
        outbound: "direct",
      });
    });
  });

  it("disables the add button for empty matcher value", async () => {
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });

    fireEvent.click(view.getByRole("button", { name: "+ 自定义规则" }));
    const addButton = view.getByRole("button", {
      name: "添加",
    }) as HTMLButtonElement;
    expect(addButton.disabled).toBe(true);
    fireEvent.change(view.getByLabelText("匹配值"), {
      target: { value: "x.io" },
    });
    await waitFor(() => {
      expect(
        (view.getByRole("button", { name: "添加" }) as HTMLButtonElement)
          .disabled,
      ).toBe(false);
    });
    expect(addCustomRule).not.toHaveBeenCalled();
  });

  it("removes a custom rule after confirmation", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("example.com")).toBeInTheDocument();
    });

    const row = view.getByText("example.com").closest("li") as HTMLElement;
    within(row).getByRole("button", { name: "删除" }).click();

    await waitFor(() => {
      expect(removeCustomRule).toHaveBeenCalledWith("fp-custom");
    });
  });

  it("paginates with server-side offset", async () => {
    listRules.mockResolvedValue(
      sampleList({ offset: 0, limit: 50, total: 120 }),
    );
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });
    expect(view.getByText(/第 1 \/ 3 页 · 共 120 条/)).toBeInTheDocument();

    view.getByRole("button", { name: "下一页" }).click();
    await waitFor(() => {
      const calls = listRules.mock.calls;
      const call = calls[calls.length - 1]?.[0] as { offset: number };
      expect(call.offset).toBe(50);
    });
  });
});