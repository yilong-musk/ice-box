// SPDX-License-Identifier: GPL-3.0-or-later

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { t } from "../../lib/i18n";
import type { StatusResponse } from "../../api/client";

const requestBatteryExemption = vi.fn();

vi.mock("../../api/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../api/client")>();
  return {
    ...actual,
    api: {
      ...actual.api,
      requestBatteryExemption: () => requestBatteryExemption(),
    },
  };
});

const { AndroidSystemCard } = await import("./Android");

describe("AndroidSystemCard", () => {
  it("shows an action's error under its own row", async () => {
    requestBatteryExemption.mockRejectedValue(new Error("settings page blocked"));
    const status = {
      battery_unrestricted: false,
      always_on_vpn: false,
    } as StatusResponse;
    render(<AndroidSystemCard status={status} />);

    fireEvent.click(
      screen.getByRole("button", { name: t("settings.batteryAction") }),
    );

    const error = await screen.findByText(/settings page blocked/);
    const rows = document.querySelectorAll('[data-slot="settings-row"]');
    const batteryRow = Array.from(rows).find((row) =>
      row.textContent?.includes(t("settings.batteryTitle")),
    );
    const alwaysOnRow = Array.from(rows).find((row) =>
      row.textContent?.includes(t("settings.alwaysOnTitle")),
    );
    await waitFor(() => expect(batteryRow).toContainElement(error));
    expect(alwaysOnRow).not.toContainElement(error);
  });
});
