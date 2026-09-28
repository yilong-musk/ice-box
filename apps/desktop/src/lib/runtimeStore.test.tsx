// SPDX-License-Identifier: GPL-3.0-or-later

import { StrictMode } from "react";
import { act, render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RuntimeStoreProvider, useRuntimeStore, type RuntimeStore } from "./runtimeStore";

const mocks = vi.hoisted(() => ({
  getStatus: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("../api/tauri", () => ({ api: {
  getStatus: mocks.getStatus,
  listenWindowHidden: mocks.listen,
  listenWindowShown: mocks.listen,
  listenCoreStatusChanged: mocks.listen,
  listenStateChanged: mocks.listen,
} }));

describe("runtime listener lifecycle", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getStatus.mockResolvedValue(null);
    mocks.listen.mockResolvedValue(() => {});
  });

  it("disposes late StrictMode registrations and the remaining listeners on unmount", async () => {
    const resolvers: Array<() => void> = [];
    const listeners = new Set<() => void>();
    const removers: Array<ReturnType<typeof vi.fn>> = [];
    mocks.listen.mockImplementation((handler: () => void) => new Promise<() => void>((resolve) => {
      resolvers.push(() => {
        listeners.add(handler);
        const off = vi.fn(() => { listeners.delete(handler); });
        removers.push(off);
        resolve(off);
      });
    }));
    const view = render(<StrictMode><RuntimeStoreProvider>child</RuntimeStoreProvider></StrictMode>);
    expect(resolvers).toHaveLength(8);
    await act(async () => { resolvers.forEach(resolve => resolve()); });
    expect(listeners.size).toBe(4);
    view.unmount();
    expect(listeners.size).toBe(0);
    removers.forEach(off => expect(off).toHaveBeenCalledTimes(1));
  });

  it("suppresses callbacks and removes subscriptions resolved after unmount", async () => {
    const resolvers: Array<() => void> = [];
    const handlers: Array<() => void> = [];
    const off = vi.fn();
    mocks.listen.mockImplementation((handler: () => void) => {
      handlers.push(handler);
      return new Promise<() => void>(resolve => resolvers.push(() => resolve(off)));
    });
    const view = render(<RuntimeStoreProvider>child</RuntimeStoreProvider>);
    view.unmount();
    mocks.getStatus.mockClear();
    await act(async () => {
      handlers.forEach(handler => handler());
      resolvers.forEach(resolve => resolve());
    });
    expect(mocks.getStatus).not.toHaveBeenCalled();
    expect(off).toHaveBeenCalledTimes(4);
  });

  it("rejects an in-flight status response after unmount", async () => {
    const captured: { store: RuntimeStore | null } = { store: null };
    function Consumer() {
      captured.store = useRuntimeStore();
      return null;
    }
    const view = render(<RuntimeStoreProvider><Consumer /></RuntimeStoreProvider>);
    await act(async () => {});

    let resolveStatus!: (value: { subscription_count: number }) => void;
    mocks.getStatus.mockImplementation(() => new Promise(resolve => {
      resolveStatus = resolve;
    }));
    const response = captured.store!.refreshStatus();
    view.unmount();
    await act(async () => { resolveStatus({ subscription_count: 1 }); });
    await expect(response).resolves.toBeNull();
  });

  it("ignores the first StrictMode status response after effects restart", async () => {
    const resolvers: Array<(value: { subscription_count: number }) => void> = [];
    mocks.getStatus.mockImplementation(() => new Promise(resolve => {
      resolvers.push(resolve);
    }));
    function Consumer() {
      const runtime = useRuntimeStore();
      return <span>{runtime?.status?.subscription_count ?? "pending"}</span>;
    }
    const view = render(
      <StrictMode><RuntimeStoreProvider><Consumer /></RuntimeStoreProvider></StrictMode>,
    );
    expect(resolvers).toHaveLength(2);
    await act(async () => { resolvers[1]({ subscription_count: 2 }); });
    expect(view.getByText("2")).toBeInTheDocument();
    await act(async () => { resolvers[0]({ subscription_count: 1 }); });
    expect(view.getByText("2")).toBeInTheDocument();
    expect(view.queryByText("1")).toBeNull();
  });
});
