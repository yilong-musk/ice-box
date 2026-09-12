// SPDX-License-Identifier: GPL-3.0-or-later

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { t, isMessageKey } from "../lib/i18n";
import { api } from "../api/tauri";
import { clearNodesSnapshot } from "../lib/nodes";
import { Subscriptions } from "./Subscriptions";

const listSubscriptions = vi.fn();
const updateAllSubscriptions = vi.fn();
const removeSubscription = vi.fn();
const listenStateChanged = vi.fn();

vi.mock("../lib/nodes", () => ({
  clearNodesSnapshot: vi.fn(),
}));

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
    listenStateChanged: (...args: unknown[]) => listenStateChanged(...args),
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
  formatUiMessage: (msg: unknown) => {
    if (!msg) return "";
    if (typeof msg === "string") return msg;
    const m = msg as { key?: string; params?: Record<string, string> };
    if (!m.key) return String(msg);
    if (m.key === "ui.raw") return m.params?.text ?? "";
    return isMessageKey(m.key) ? t(m.key, m.params) : m.key;
  },
}));

/** Handler the page registers for `app://state-changed` (tray actions). */
let stateChangedHandler: (() => void) | null = null;

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
    auto_update_interval: null,
    ...overrides,
  };
}

describe("Subscriptions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    stateChangedHandler = null;
    listenStateChanged.mockImplementation((handler: () => void) => {
      stateChangedHandler = handler;
      return Promise.resolve(() => {});
    });
    listSubscriptions.mockResolvedValue([]);
    vi.mocked(clearNodesSnapshot).mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("renders import form", async () => {
    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(
        view.getByPlaceholderText(t("subs.urlPlaceholder")),
      ).toBeInTheDocument();
    });
    expect(view.getByText(t("subs.emptyTitle"))).toBeInTheDocument();
    expect(
      view.getByText(t("subs.emptyDesc")),
    ).toBeInTheDocument();
    expect(container.querySelector(".sub-list")).toBeNull();
    expect(view.getByTestId("subs-panel")).toBeInTheDocument();
  });

  it("re-reads the active subscription after a tray switch", async () => {
    const secondId = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    listSubscriptions.mockResolvedValue([
      sampleMeta({ name: "sub-a", active: true }),
      sampleMeta({ id: secondId, name: "sub-b", active: false }),
    ]);

    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("sub-a")).toBeInTheDocument();
    });
    vi.mocked(clearNodesSnapshot).mockClear();
    const fetchesBefore = listSubscriptions.mock.calls.length;

    // The tray「订阅」submenu made sub-b the active one.
    listSubscriptions.mockResolvedValue([
      sampleMeta({ name: "sub-a", active: false }),
      sampleMeta({ id: secondId, name: "sub-b", active: true }),
    ]);

    await act(async () => {
      stateChangedHandler?.();
    });

    await waitFor(() => {
      expect(listSubscriptions.mock.calls.length).toBeGreaterThan(fetchesBefore);
      const title = view.getByText("sub-b").closest("[data-slot=item-title]");
      expect(title).toHaveTextContent(t("subs.activeBadge"));
    });
    expect(clearNodesSnapshot).toHaveBeenCalled();
  });

  it("does not drop the node snapshot when a tray event did not switch subscriptions", async () => {
    listSubscriptions.mockResolvedValue([
      sampleMeta({ name: "sub-a", active: true }),
    ]);

    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("sub-a")).toBeInTheDocument();
    });
    vi.mocked(clearNodesSnapshot).mockClear();
    const fetchesBefore = listSubscriptions.mock.calls.length;

    await act(async () => {
      stateChangedHandler?.();
    });

    await waitFor(() => {
      expect(listSubscriptions.mock.calls.length).toBeGreaterThan(fetchesBefore);
    });
    expect(clearNodesSnapshot).not.toHaveBeenCalled();
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

    view.getByRole("button", { name: t("subs.updateAll") }).click();

    await waitFor(() => {
      expect(view.getByRole("alert")).toHaveTextContent(
        t("subs.partialUpdateFailed", { details: "x" }).split("x")[0],
      );
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

    view.getByRole("button", { name: t("subs.updateAll") }).click();

    await waitFor(() => {
      const updatingButtons = view.getAllByRole("button", { name: t("common.updating") });
      expect(updatingButtons.length).toBeGreaterThanOrEqual(2);
      for (const btn of updatingButtons) {
        expect(btn).toBeDisabled();
      }
    });

    resolveUpdate({ results: [] });
    await waitFor(() => {
      expect(view.getByRole("button", { name: t("subs.updateAll") })).toBeInTheDocument();
    });
  });

  it("shows an apply warning from a remove response", async () => {
    listSubscriptions.mockResolvedValue([sampleMeta({ name: "only-one" })]);
    removeSubscription.mockResolvedValue({
      ok: true,
      apply_warning: {
        code: "proxy.restore_failed",
        message: "core reloaded but system proxy was not restored",
      },
    });

    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("only-one")).toBeInTheDocument();
    });

    fireEvent.click(view.getByRole("button", { name: t("common.delete") }));

    await waitFor(() => {
      expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    });
    fireEvent.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
      name: t("common.delete"),
      }),
    );

    await waitFor(() => {
      expect(removeSubscription).toHaveBeenCalled();
      expect(view.getByText(t("error.proxy.restore_failed"), { exact: false })).toBeInTheDocument();
    });
  });

  it("shows group/rule/dns stats and parse warnings", async () => {
    listSubscriptions.mockResolvedValue([
      sampleMeta({
        name: "flower",
        group_count: 21,
        rule_count: 4270,
        has_dns: true,
        parse_warnings: [
          { key: "parse.groupUnknownMember", params: { name: "x", member: "y" } },
        ],
      }),
    ]);

    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("flower")).toBeInTheDocument();
    });
    expect(view.getByText(t("subs.summaryGroups", { n: 21 }), { exact: false })).toBeInTheDocument();
    expect(view.getByText(t("subs.summaryRules", { n: 4270 }), { exact: false })).toBeInTheDocument();
    expect(view.getByText(t("subs.hasDns"), { exact: false })).toBeInTheDocument();
    expect(
      view.getByText(t("parse.groupUnknownMember", { name: "x", member: "y" }), {
        exact: false,
      }),
    ).toBeInTheDocument();
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
    expect(view.getByText(t("subs.summaryNodes", { n: 5 }), { exact: false })).toBeInTheDocument();
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
      expect(view.getByLabelText(t("subs.autoUpdate"))).toBeInTheDocument();
    });
    const importSwitch = view.getByLabelText(t("subs.autoUpdate"));
    expect(importSwitch).not.toBeChecked();
    fireEvent.click(importSwitch);
    expect(importSwitch).toBeChecked();

    const interval = view.getByLabelText(t("subs.interval"));
    expect(interval).toBeEnabled();
    fireEvent.change(interval, { target: { value: "six_hours" } });

    fireEvent.change(
      view.getByPlaceholderText(t("subs.urlPlaceholder")),
      { target: { value: "https://example.com/new" } },
    );
    fireEvent.click(view.getByRole("button", { name: t("subs.importAction") }));

    await waitFor(() => {
      expect(add).toHaveBeenCalledWith(
        "https://example.com/new",
        undefined,
        true,
        "six_hours",
      );
    });
    await waitFor(() => {
      expect(importSwitch).not.toBeChecked();
      expect(interval).toBeDisabled();
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
      name: t("subs.autoUpdate"),
    });
    expect(switches).toHaveLength(1);
    expect(switches[0]).not.toBeChecked();
    fireEvent.click(switches[0]);
    await waitFor(() => {
      expect(setAutoUpdate).toHaveBeenCalledWith(
        "22222222-2222-2222-2222-222222222222",
        true,
        "one_hour",
      );
    });

    const rowA = view.getByText("a").closest("[data-slot=item]") as HTMLElement;
    expect(within(rowA).getAllByRole("switch", { name: t("subs.autoUpdate") })[0]).toBeChecked();
  });

  it("changes the auto-update interval from a subscription row", async () => {
    const setAutoUpdate = vi.fn().mockResolvedValue({});
    vi.mocked(api.setSubscriptionAutoUpdate).mockImplementation(setAutoUpdate);
    listSubscriptions.mockResolvedValue([
      sampleMeta({ name: "a", auto_update: true, id: "11111111-1111-1111-1111-111111111111" }),
    ]);

    const { container } = render(<Subscriptions />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByText("a")).toBeInTheDocument();
    });
    const rowA = view.getByText("a").closest("[data-slot=item]") as HTMLElement;
    const interval = within(rowA).getByLabelText(t("subs.interval"));
    expect(interval).toHaveValue("one_hour");
    fireEvent.change(interval, { target: { value: "twelve_hours" } });
    await waitFor(() => {
      expect(setAutoUpdate).toHaveBeenCalledWith(
        "11111111-1111-1111-1111-111111111111",
        true,
        "twelve_hours",
      );
    });
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
    const toggle = within(rowB).getByRole("switch", { name: t("common.activate") });
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
