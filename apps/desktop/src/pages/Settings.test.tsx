import { fireEvent, render, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { THEME_STORAGE_KEY } from "../lib/theme";
import { Settings } from "./Settings";

const getSettings = vi.fn();
const saveSettings = vi.fn();

vi.mock("../api/tauri", () => ({
  api: {
    getSettings: (...args: unknown[]) => getSettings(...args),
    saveSettings: (...args: unknown[]) => saveSettings(...args),
    revealDataDir: vi.fn(),
  },
  formatInvokeError: (err: unknown) => String(err),
}));

describe("Settings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.localStorage.removeItem(THEME_STORAGE_KEY);
    document.documentElement.classList.remove("dark");
    getSettings.mockResolvedValue({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: null,
      auto_set_system_proxy: false,
      allow_lan: false,
      proxy_mode: "rule",
    });
  });

  it("blocks save when port is out of range", async () => {
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByDisplayValue("17890")).toBeInTheDocument();
    });

    const portInput = view.getByDisplayValue("17890");
    fireEvent.change(portInput, { target: { value: "80" } });
    fireEvent.submit(portInput.closest("form")!);

    await waitFor(() => {
      expect(container.textContent).toContain("1024");
      expect(saveSettings).not.toHaveBeenCalled();
    });
  });

  it("blocks save when listen address is not loopback", async () => {
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getAllByDisplayValue("127.0.0.1").length).toBeGreaterThan(0);
    });

    const listenInputs = view.getAllByDisplayValue("127.0.0.1");
    fireEvent.change(listenInputs[0], { target: { value: "0.0.0.0" } });
    fireEvent.submit(listenInputs[0].closest("form")!);

    await waitFor(() => {
      expect(container.textContent).toContain("loopback");
      expect(saveSettings).not.toHaveBeenCalled();
    });
  });

  it("allows non-loopback mixed listen when allow_lan is on", async () => {
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByLabelText("允许局域网共享（Allow LAN）")).toBeInTheDocument();
    });

    fireEvent.click(view.getByLabelText("允许局域网共享（Allow LAN）"));

    await waitFor(() => {
      const listenInputs = view.getAllByDisplayValue("127.0.0.1");
      expect(listenInputs[0]).toBeDisabled();
      expect(listenInputs[1]).not.toBeDisabled();
    });

    const saveButton = view.getByRole("button", { name: "保存" });
    fireEvent.submit(saveButton.closest("form")!);

    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({ allow_lan: true }),
      );
      expect(container.textContent).not.toContain("loopback");
    });
  });

  it("blocks save when mixed and clash api ports conflict", async () => {
    const { container } = render(<Settings />);
    const view = within(container);
    await waitFor(() => {
      expect(view.getByDisplayValue("19090")).toBeInTheDocument();
    });

    const clashPort = view.getByDisplayValue("19090");
    fireEvent.change(clashPort, { target: { value: "17890" } });
    fireEvent.submit(clashPort.closest("form")!);

    await waitFor(() => {
      expect(container.textContent).toContain("不能相同");
      expect(saveSettings).not.toHaveBeenCalled();
    });
  });

  it("blocks save before settings load completes", async () => {
    let resolveSettings: (value: unknown) => void = () => {};
    getSettings.mockReturnValue(
      new Promise((resolve) => {
        resolveSettings = resolve;
      }),
    );

    const { container } = render(<Settings />);
    const view = within(container);
    const saveButton = view.getByRole("button", { name: "保存" });
    expect(saveButton).toBeDisabled();

    fireEvent.submit(view.getByRole("button", { name: "保存" }).closest("form")!);
    expect(saveSettings).not.toHaveBeenCalled();

    resolveSettings({
      mixed_listen: "127.0.0.1",
      mixed_port: 17890,
      clash_api_listen: "127.0.0.1",
      clash_api_port: 19090,
      selected_tag: null,
      auto_set_system_proxy: false,
      allow_lan: false,
      proxy_mode: "rule",
    });

    await waitFor(() => {
      expect(saveButton).not.toBeDisabled();
    });
  });

  it("blocks save when settings load fails", async () => {
    getSettings.mockRejectedValue("load failed");

    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(container.textContent).toContain("load failed");
    });

    const saveButton = view.getByRole("button", { name: "保存" });
    expect(saveButton).toBeDisabled();
    fireEvent.submit(saveButton.closest("form")!);
    expect(saveSettings).not.toHaveBeenCalled();
  });

  it("defaults appearance to follow the system and applies immediately", async () => {
    const { container } = render(<Settings />);
    const view = within(container);

    await waitFor(() => {
      expect(view.getByRole("button", { name: "跟随系统" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });

    fireEvent.click(view.getByRole("button", { name: "浅色" }));
    expect(view.getByRole("button", { name: "浅色" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    expect(saveSettings).not.toHaveBeenCalled();

    fireEvent.click(view.getByRole("button", { name: "深色" }));
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");
    expect(saveSettings).not.toHaveBeenCalled();

    fireEvent.click(view.getByRole("button", { name: "跟随系统" }));
    expect(view.getByRole("button", { name: "跟随系统" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("system");
    expect(saveSettings).not.toHaveBeenCalled();
  });

  it("lets the settings card fill the content pane", () => {
    const { container } = render(<Settings />);
    const card = container.querySelector("[data-slot=card]");
    expect(card).not.toBeNull();
    const classes = card!.className.split(/\s+/);
    expect(classes).toContain("w-full");
    expect(classes).not.toContain("max-w-lg");
  });
});
