// SPDX-License-Identifier: GPL-3.0-or-later

import { StrictMode } from "react";
import { act, render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RuntimeStoreProvider, useRuntimeStore, type RuntimeStore } from "./runtimeStore";

const mocks = vi.hoisted(() => ({
  getStatus: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("../api/client", () => ({ api: {
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

  it("keeps the latest request when responses arrive in reverse order", async () => {
    const captured: { store: RuntimeStore | null } = { store: null };
    function Consumer() {
      captured.store = useRuntimeStore();
      return <span>{captured.store?.status?.subscription_count ?? "pending"}</span>;
    }
    const view = render(<RuntimeStoreProvider><Consumer /></RuntimeStoreProvider>);
    await act(async () => {});
    const resolvers: Array<(value: { subscription_count: number }) => void> = [];
    mocks.getStatus.mockImplementation(() => new Promise(resolve => resolvers.push(resolve)));
    const older = captured.store!.refreshStatus();
    const newer = captured.store!.refreshStatus();
    await act(async () => { resolvers[1]({ subscription_count: 2 }); });
    await expect(newer).resolves.toEqual({ subscription_count: 2 });
    await act(async () => { resolvers[0]({ subscription_count: 1 }); });
    await expect(older).resolves.toBeNull();
    expect(view.getByText("2")).toBeInTheDocument();
    view.unmount();
  });

  it("does not revive an older request when the latest request fails", async () => {
    const captured: { store: RuntimeStore | null } = { store: null };
    function Consumer() {
      captured.store = useRuntimeStore();
      return null;
    }
    const view = render(<RuntimeStoreProvider><Consumer /></RuntimeStoreProvider>);
    await act(async () => {});
    let resolveOlder!: (value: { subscription_count: number }) => void;
    mocks.getStatus
      .mockImplementationOnce(() => new Promise(resolve => { resolveOlder = resolve; }))
      .mockRejectedValueOnce(new Error("unavailable"));
    const older = captured.store!.refreshStatus();
    await expect(captured.store!.refreshStatus()).resolves.toBeNull();
    await act(async () => { resolveOlder({ subscription_count: 1 }); });
    await expect(older).resolves.toBeNull();
    expect(captured.store!.status).toBeNull();
    view.unmount();
  });

  it("keeps the same status object when only the clock and probe age change", async () => {
    const base = {
      revision: 1,
      sampled_at_ms: 10,
      diagnostics: { age_ms: 1, checked_at_ms: 2, stale: false, error: null },
      subscription_count: 3,
      memory: { app_bytes: 1, core_bytes: null, total_bytes: 1 },
    };
    mocks.getStatus.mockResolvedValue(base);
    const captured: { store: RuntimeStore | null } = { store: null };
    function Consumer() {
      captured.store = useRuntimeStore();
      return null;
    }
    const view = render(<RuntimeStoreProvider><Consumer /></RuntimeStoreProvider>);
    await act(async () => {});
    const first = captured.store!.status;
    expect(first?.revision).toBe(1);
    mocks.getStatus.mockResolvedValue({
      ...base,
      revision: 2,
      sampled_at_ms: 99,
      diagnostics: { age_ms: 40, checked_at_ms: 80, stale: false, error: null },
    });
    await act(async () => {
      await captured.store!.refreshStatus();
    });
    expect(captured.store!.status).toBe(first);
    mocks.getStatus.mockResolvedValue({
      ...base,
      memory: { app_bytes: 2, core_bytes: null, total_bytes: 2 },
    });
    await act(async () => {
      await captured.store!.refreshStatus();
    });
    expect(captured.store!.status).not.toBe(first);
    expect(captured.store!.status?.memory?.total_bytes).toBe(2);
    view.unmount();
  });

  it("still rejects responses from an earlier mutation generation", async () => {
    const captured: { store: RuntimeStore | null } = { store: null };
    function Consumer() {
      captured.store = useRuntimeStore();
      return null;
    }
    const view = render(<RuntimeStoreProvider><Consumer /></RuntimeStoreProvider>);
    await act(async () => {});
    let resolveStatus!: (value: { subscription_count: number }) => void;
    mocks.getStatus.mockImplementationOnce(() => new Promise(resolve => { resolveStatus = resolve; }));
    const response = captured.store!.refreshStatus();
    captured.store!.bumpGeneration();
    await act(async () => { resolveStatus({ subscription_count: 1 }); });
    await expect(response).resolves.toBeNull();
    expect(captured.store!.status).toBeNull();
    view.unmount();
  });
});
