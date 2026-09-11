// SPDX-License-Identifier: GPL-3.0-or-later

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { t, isMessageKey } from "../lib/i18n";
import { ruleTypeLabel } from "../lib/rules";
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
  formatInvokeError: (err: unknown) => {
    if (err && typeof err === "object") {
      const o = err as { code?: string; message?: string };
      if (typeof o.code === "string") {
        const key = `error.${o.code}`;
        if (isMessageKey(key)) return `${t(key)} (${o.code})`;
        if (typeof o.message === "string") return `${o.code}: ${o.message}`;
      }
      if (typeof o.message === "string") return o.message;
    }
    return String(err);
  },
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

  it("renders rows, type chips and filters", async () => {
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });
    expect(view.getByRole("radio", { name: `${ruleTypeLabel("domain_suffix")} 2` })).toBeInTheDocument();
    expect(view.getByRole("radio", { name: t("rules.typeAll") })).toBeInTheDocument();
    expect(view.getByRole("radio", { name: `${ruleTypeLabel("geoip")} 1` })).toBeInTheDocument();
    const chips = view.getByLabelText(t("rules.filtersAria"));
    const customFilter = view.getByRole("button", { name: t("rules.customCount", { count: 1 }) });
    const disabledFilter = view.getByRole("button", { name: t("rules.disabledCount", { count: 1 }) });
    expect(chips).toContainElement(customFilter);
    expect(chips).toContainElement(disabledFilter);
    expect(view.getByText("example.com")).toBeInTheDocument();
    const customRow = view
      .getByText("example.com")
      .closest("[data-slot=item]") as HTMLElement;
    const customMarker = within(customRow).getByText(t("rules.custom"));
    expect(customMarker).not.toHaveAttribute("data-slot", "badge");
    expect(container.querySelector(".node-table")).toBeNull();
    expect(view.getByRole("list", { name: t("rules.listAria") })).toBeInTheDocument();
    const ruleList = view.getByRole("list", { name: t("rules.listAria") });
    const scrollArea = ruleList.closest('[data-slot="scroll-area"]');
    expect(
      scrollArea?.querySelector('[data-slot="scroll-area-viewport"]'),
    ).toBeInTheDocument();
    expect(view.getByTestId("rules-panel")).toBeInTheDocument();
  });

  it("reloads the active subscription when the pane is reactivated", async () => {
    const { container, rerender } = render(<Rules active />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });
    const initialLoads = getRuleOverview.mock.calls.length;

    rerender(<Rules active={false} />);
    rerender(<Rules active />);

    await waitFor(() => {
      expect(getRuleOverview.mock.calls.length).toBeGreaterThan(initialLoads);
    });
  });

  it("filters disabled rules from the chip next to type filters", async () => {
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });

    view.getByRole("button", { name: t("rules.disabledCount", { count: 1 }) }).click();

    await waitFor(() => {
      const calls = listRules.mock.calls;
      const call = calls[calls.length - 1]?.[0] as { disabled: string };
      expect(call.disabled).toBe("disabled");
    });
  });

  it("filters custom rules from the quick filter toggle", async () => {
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });

    view.getByRole("button", { name: t("rules.customCount", { count: 1 }) }).click();

    await waitFor(() => {
      const calls = listRules.mock.calls;
      const call = calls[calls.length - 1]?.[0] as { custom: boolean | null };
      expect(call.custom).toBe(true);
    });
  });

  it("debounces keyword search and sends server-side filters", async () => {
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });

    view.getByLabelText(t("rules.searchAria")).focus();
    fireEvent.change(view.getByLabelText(t("rules.searchAria")), {
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

    const row = view.getByText("youtube.com").closest("[data-slot=item]") as HTMLElement;
    within(row).getByRole("button", { name: t("common.disable") }).click();

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

    const row = view.getByText("youtube.com").closest("[data-slot=item]") as HTMLElement;
    within(row).getByRole("button", { name: t("common.disable") }).click();

    await waitFor(() => {
      expect(screen.getByText(t("rules.savedButApplyFailed", { detail: `${t("error.config.invalid")} (config.invalid)` }))).toBeInTheDocument();
    });
  });

  it("adds a custom rule via interactive form", async () => {
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });

    fireEvent.click(view.getByRole("button", { name: t("rules.addCustom") }));
    await waitFor(() => {
      expect(screen.getByRole("dialog")).toBeInTheDocument();
    });
    expect(screen.getByLabelText(t("ruleForm.matchValue"))).toBeInTheDocument();
    expect(screen.getByLabelText(t("ruleForm.outbound"))).toBeInTheDocument();
    expect(screen.getByRole("button", { name: t("ruleForm.add") })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText(t("ruleForm.matchValue")), {
      target: { value: "x.io, y.io" },
    });
    const outbound = screen.getByLabelText(t("ruleForm.outbound")) as HTMLSelectElement;
    await waitFor(() => {
      expect(
        within(outbound).getByRole("option", { name: "n1" }),
      ).toBeInTheDocument();
    });
    fireEvent.change(outbound, { target: { value: "n1" } });
    screen.getByRole("button", { name: t("ruleForm.add") }).click();

    await waitFor(() => {
      expect(addCustomRule).toHaveBeenCalledWith({
        domain_suffix: ["x.io", "y.io"],
        outbound: "n1",
      });
    });
    await waitFor(() => {
      expect(screen.queryByLabelText(t("ruleForm.matchValue"))).not.toBeInTheDocument();
    });
  });

  it("lists current nodes/groups as outbound options", async () => {
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });

    fireEvent.click(view.getByRole("button", { name: t("rules.addCustom") }));
    const outbound = screen.getByLabelText(t("ruleForm.outbound")) as HTMLSelectElement;
    await waitFor(() => {
      expect(
        within(outbound).getByRole("option", {
          name: `Proxies${t("ruleForm.strategyGroupSuffix")}`,
        }),
      ).toBeInTheDocument();
    });
    expect(
      within(outbound).getByRole("option", { name: t("ruleForm.directOption") }),
    ).toBeInTheDocument();
  });

  it("shows boolean matcher as checkbox", async () => {
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });

    fireEvent.click(view.getByRole("button", { name: t("rules.addCustom") }));
    fireEvent.change(screen.getByLabelText(t("ruleForm.matcherType")), {
      target: { value: "ip_is_private" },
    });
    const checkbox = screen.getByRole("checkbox", {
      name: t("ruleType.ipIsPrivate"),
    });
    expect(checkbox).toBeInTheDocument();

    screen.getByRole("button", { name: t("ruleForm.add") }).click();
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

    fireEvent.click(view.getByRole("button", { name: t("rules.addCustom") }));
    const addButton = screen.getByRole("button", {
      name: t("ruleForm.add"),
    }) as HTMLButtonElement;
    expect(addButton.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText(t("ruleForm.matchValue")), {
      target: { value: "x.io" },
    });
    await waitFor(() => {
      expect(
        (screen.getByRole("button", { name: t("ruleForm.add") }) as HTMLButtonElement)
          .disabled,
      ).toBe(false);
    });
    expect(addCustomRule).not.toHaveBeenCalled();
  });

  it("resets the form after a successful add", async () => {
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });

    fireEvent.click(view.getByRole("button", { name: t("rules.addCustom") }));
    fireEvent.change(screen.getByLabelText(t("ruleForm.matchValue")), {
      target: { value: "x.io" },
    });
    screen.getByRole("button", { name: t("ruleForm.add") }).click();
    await waitFor(() => {
      expect(addCustomRule).toHaveBeenCalledWith({
        domain_suffix: ["x.io"],
        outbound: "direct",
      });
    });
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });

    fireEvent.click(view.getByRole("button", { name: t("rules.addCustom") }));
    await waitFor(() => {
      expect(screen.getByRole("dialog")).toBeInTheDocument();
    });
    expect((screen.getByLabelText(t("ruleForm.matchValue")) as HTMLInputElement).value).toBe(
      "",
    );
    expect(
      (screen.getByRole("button", { name: t("ruleForm.add") }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });

  it("removes a custom rule after confirmation", async () => {
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("example.com")).toBeInTheDocument();
    });

    const row = view.getByText("example.com").closest("[data-slot=item]") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: t("common.delete") }));

    await waitFor(() => {
      expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    });
    fireEvent.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
        name: t("common.delete"),
      }),
    );

    await waitFor(() => {
      expect(removeCustomRule).toHaveBeenCalledWith("fp-custom");
    });
  });

  it("paginates with server-side offset", async () => {
    listRules.mockResolvedValue(
      sampleList({ offset: 0, limit: 100, total: 120 }),
    );
    const { container } = render(<Rules />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("youtube.com")).toBeInTheDocument();
    });
    const pager = view.getByTestId("rules-pager");
    expect(pager).toHaveTextContent(
      t("rules.pageInfo", { page: 1, pages: 2, total: 120 }),
    );

    const viewport = container.querySelector(
      '[data-slot="scroll-area-viewport"]',
    ) as HTMLDivElement;
    Object.defineProperties(viewport, {
      scrollHeight: { configurable: true, value: 1_000 },
      clientHeight: { configurable: true, value: 300 },
      scrollTop: { configurable: true, value: 100, writable: true },
    });
    fireEvent.scroll(viewport);
    expect(pager).toHaveAttribute("data-visible", "false");

    view.getByRole("button", { name: t("rules.nextPage") }).click();
    await waitFor(() => {
      const calls = listRules.mock.calls;
      const call = calls[calls.length - 1]?.[0] as { offset: number };
      expect(call.offset).toBe(100);
    });
  });
});
