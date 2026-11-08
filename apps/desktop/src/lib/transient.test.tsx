// SPDX-License-Identifier: GPL-3.0-or-later

import { act, render } from "@testing-library/react";
import { Activity } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useTransient } from "./transient";

type Api = { show: (value: string, ms: number) => void; clear: () => void };

function Probe({ api }: { api: { current: Api | null } }) {
  const [value, show, clear] = useTransient("idle");
  api.current = { show, clear };
  return <output data-testid="value">{value}</output>;
}

function setup() {
  const api: { current: Api | null } = { current: null };
  const view = render(
    <Activity mode="visible">
      <Probe api={api} />
    </Activity>,
  );
  const setVisible = (visible: boolean) =>
    view.rerender(
      <Activity mode={visible ? "visible" : "hidden"}>
        <Probe api={api} />
      </Activity>,
    );
  const value = () => view.getByTestId("value").textContent;
  const advance = (ms: number) =>
    act(() => {
      vi.advanceTimersByTime(ms);
    });
  const show = (text: string, ms: number) =>
    act(() => {
      api.current!.show(text, ms);
    });
  return { api, setVisible, value, advance, show };
}

describe("useTransient", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("reverts to idle after its time", () => {
    const { value, advance, show } = setup();
    expect(value()).toBe("idle");
    show("copied", 2000);
    expect(value()).toBe("copied");
    advance(1999);
    expect(value()).toBe("copied");
    advance(1);
    expect(value()).toBe("idle");
  });

  it("gives a replacement its own full time", () => {
    const { value, advance, show } = setup();
    show("first", 2000);
    advance(1500);
    show("second", 2000);
    advance(1999);
    expect(value()).toBe("second");
    advance(1);
    expect(value()).toBe("idle");
  });

  it("clears at once", () => {
    const { api, value, show } = setup();
    show("copied", 2000);
    act(() => api.current!.clear());
    expect(value()).toBe("idle");
  });

  it("keeps counting while the page is hidden", () => {
    const { setVisible, value, advance, show } = setup();
    show("saved", 2000);
    advance(500);
    setVisible(false);
    advance(1000);
    setVisible(true);
    expect(value()).toBe("saved");
    advance(499);
    expect(value()).toBe("saved");
    advance(1);
    expect(value()).toBe("idle");
  });

  it("is not shown again when the page returns after its time", () => {
    const { setVisible, value, advance, show } = setup();
    show("saved", 2000);
    setVisible(false);
    advance(60_000);
    setVisible(true);
    expect(value()).toBe("idle");
  });

  it("is not stretched by the wall clock stepping back while hidden", () => {
    const { setVisible, value, advance, show } = setup();
    show("saved", 2000);
    advance(500);
    setVisible(false);
    vi.setSystemTime(Date.now() - 60 * 60_000);
    setVisible(true);
    advance(1500);
    expect(value()).toBe("idle");
  });
});
