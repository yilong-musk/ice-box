// SPDX-License-Identifier: GPL-3.0-or-later

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { t } from "../lib/i18n";
import { Apps } from "./Apps";

const listApps = vi.fn();
const getSettings = vi.fn();
const setAppProxy = vi.fn();

vi.mock("../api/client", () => ({
  api: {
    listApps: (...args: unknown[]) => listApps(...args),
    getSettings: (...args: unknown[]) => getSettings(...args),
    setAppProxy: (...args: unknown[]) => setAppProxy(...args),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

const apps = [
  {
    package_name: "com.example.browser",
    label: "Browser",
    system: false,
    icon: null,
  },
  {
    package_name: "com.android.settings",
    label: "Settings",
    system: true,
    icon: null,
  },
  {
    package_name: "com.example.bank",
    label: "Bank",
    system: false,
    icon: null,
  },
];

describe("Apps", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listApps.mockResolvedValue(apps);
    getSettings.mockResolvedValue({
      app_proxies: { "com.example.browser": "direct" },
    });
    setAppProxy.mockResolvedValue(undefined);
  });

  it("lists installed apps with their current routing mode", async () => {
    const { container } = render(<Apps />);
    const view = within(container);

    await waitFor(() => {
      expect(
        view.getByRole("button", { name: t("apps.selectAria", { app: "Browser" }) }),
      ).toBeInTheDocument();
    });
    expect(
      view.getByRole("button", { name: t("apps.selectAria", { app: "Settings" }) }),
    ).toBeInTheDocument();
    expect(view.getByText(t("apps.mode.direct"))).toBeInTheDocument();
    expect(view.getByText(t("apps.systemBadge"))).toBeInTheDocument();
  });

  it("saves a new proxy mode for the selected app", async () => {
    const { container } = render(<Apps />);
    const view = within(container);

    fireEvent.click(
      await view.findByRole("button", {
        name: t("apps.selectAria", { app: "Browser" }),
      }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: new RegExp(`^${t("apps.mode.proxy")}`) }),
    );

    await waitFor(() => {
      expect(setAppProxy).toHaveBeenCalledWith("com.example.browser", "proxy");
    });
    await waitFor(() => {
      expect(screen.queryByText(t("apps.sheetDesc"))).toBeNull();
    });
  });

  it("shows a failed update in the sheet without duplicating it on the page", async () => {
    setAppProxy.mockRejectedValueOnce("disk full");
    const { container } = render(<Apps />);
    const view = within(container);

    fireEvent.click(
      await view.findByRole("button", {
        name: t("apps.selectAria", { app: "Browser" }),
      }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: new RegExp(`^${t("apps.mode.proxy")}`) }),
    );

    const message = t("apps.applyFailed", { detail: "disk full" });
    await waitFor(() => {
      expect(screen.getByText(message)).toBeInTheDocument();
    });
    expect(screen.getByText(t("apps.sheetDesc"))).toBeInTheDocument();
    expect(view.queryByText(message)).toBeNull();
  });

  it("filters by search text", async () => {
    const { container } = render(<Apps />);
    const view = within(container);

    fireEvent.change(
      await view.findByRole("searchbox", { name: t("apps.searchAria") }),
      { target: { value: "bank" } },
    );
    expect(
      view.queryByRole("button", { name: t("apps.selectAria", { app: "Browser" }) }),
    ).toBeNull();
    expect(
      view.getByRole("button", { name: t("apps.selectAria", { app: "Bank" }) }),
    ).toBeInTheDocument();
  });
});
