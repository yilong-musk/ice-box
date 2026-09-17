// SPDX-License-Identifier: GPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import type { CheckAppUpdateResponse } from "../api/tauri";
import {
  BACKGROUND_UPDATE_RETRY_MS,
  CORE_READY_RETRY_MS,
  UPDATE_CHECK_INTERVAL_MS,
  backgroundUpdateRetryMs,
  startBackgroundAppUpdateCheck,
} from "./appUpdateCheck";

const empty: CheckAppUpdateResponse = {
  available: false,
  version: null,
  notes: null,
  skipped: false,
  should_prompt: false,
};

const available: CheckAppUpdateResponse = {
  available: true,
  version: "0.1.8",
  notes: "fixes",
  skipped: false,
  should_prompt: false,
};

function last<T>(items: T[]): T | undefined {
  return items[items.length - 1];
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("backgroundUpdateRetryMs", () => {
  it("steps through backoff and is exhausted after the 15-minute slot", () => {
    expect(backgroundUpdateRetryMs(0)).toBe(BACKGROUND_UPDATE_RETRY_MS[0]);
    expect(backgroundUpdateRetryMs(1)).toBe(30_000);
    expect(backgroundUpdateRetryMs(5)).toBe(900_000);
    expect(backgroundUpdateRetryMs(6)).toBeNull();
  });
});

describe("startBackgroundAppUpdateCheck", () => {
  it("runs the launch check immediately and schedules the next round in 24h", async () => {
    const scheduled: Array<{ fn: () => void; ms: number }> = [];
    const check = vi.fn().mockResolvedValue(empty);
    const onResult = vi.fn();
    const checker = startBackgroundAppUpdateCheck({
      check,
      onResult,
      schedule: (fn, ms) => {
        scheduled.push({ fn, ms });
        return scheduled.length;
      },
      cancel: () => {},
    });
    await checker.idle();
    expect(check).toHaveBeenCalledTimes(1);
    expect(check).toHaveBeenCalledWith({ startup: true });
    expect(onResult).toHaveBeenCalledWith(empty);
    expect(scheduled).toEqual([
      { fn: expect.any(Function), ms: UPDATE_CHECK_INTERVAL_MS },
    ]);
    checker.stop();
  });

  it("checks again once the in-session interval elapses", async () => {
    const scheduled: Array<{ fn: () => void; ms: number }> = [];
    const check = vi.fn().mockResolvedValue(empty);
    const onResult = vi.fn();
    const checker = startBackgroundAppUpdateCheck({
      check,
      onResult,
      schedule: (fn, ms) => {
        scheduled.push({ fn, ms });
        return scheduled.length;
      },
      cancel: () => {
        scheduled.length = 0;
      },
    });
    await checker.idle();
    expect(last(scheduled)?.ms).toBe(UPDATE_CHECK_INTERVAL_MS);
    last(scheduled)?.fn();
    await checker.idle();
    expect(check).toHaveBeenCalledTimes(2);
    expect(check).toHaveBeenNthCalledWith(2, { startup: false });
    expect(last(scheduled)?.ms).toBe(UPDATE_CHECK_INTERVAL_MS);
    checker.stop();
  });

  it("honors a custom in-session interval", async () => {
    const scheduled: Array<{ fn: () => void; ms: number }> = [];
    const check = vi.fn().mockResolvedValue(empty);
    const checker = startBackgroundAppUpdateCheck({
      check,
      onResult: () => {},
      intervalMs: 60_000,
      schedule: (fn, ms) => {
        scheduled.push({ fn, ms });
        return scheduled.length;
      },
      cancel: () => {
        scheduled.length = 0;
      },
    });
    await checker.idle();
    expect(last(scheduled)?.ms).toBe(60_000);
    last(scheduled)?.fn();
    await checker.idle();
    expect(check).toHaveBeenNthCalledWith(2, { startup: false });
    checker.stop();
  });

  it("retries a failure with the first backoff and surfaces a later success", async () => {
    const scheduled: Array<{ fn: () => void; ms: number }> = [];
    const check = vi
      .fn()
      .mockRejectedValueOnce(new Error("net"))
      .mockResolvedValueOnce(available);
    const onResult = vi.fn();
    const checker = startBackgroundAppUpdateCheck({
      check,
      onResult,
      schedule: (fn, ms) => {
        scheduled.push({ fn, ms });
        return scheduled.length;
      },
      cancel: () => {
        scheduled.length = 0;
      },
    });
    await checker.idle();
    expect(onResult).not.toHaveBeenCalled();
    expect(check).toHaveBeenCalledWith({ startup: true });
    expect(scheduled).toEqual([{ fn: expect.any(Function), ms: 10_000 }]);
    scheduled[0].fn();
    await checker.idle();
    expect(check).toHaveBeenCalledTimes(2);
    expect(check).toHaveBeenNthCalledWith(2, { startup: true });
    expect(onResult).toHaveBeenCalledWith(available);
    expect(last(scheduled)?.ms).toBe(UPDATE_CHECK_INTERVAL_MS);
    checker.stop();
  });

  it("retries sooner when the core becomes ready after a failure", async () => {
    const scheduled: Array<{ fn: () => void; ms: number }> = [];
    const check = vi
      .fn()
      .mockRejectedValueOnce(new Error("net"))
      .mockResolvedValueOnce(available);
    const onResult = vi.fn();
    const checker = startBackgroundAppUpdateCheck({
      check,
      onResult,
      schedule: (fn, ms) => {
        scheduled.push({ fn, ms });
        return scheduled.length;
      },
      cancel: () => {
        scheduled.length = 0;
      },
    });
    await checker.idle();
    checker.notifyCoreRunning();
    expect(last(scheduled)?.ms).toBe(CORE_READY_RETRY_MS);
    last(scheduled)?.fn();
    await checker.idle();
    expect(onResult).toHaveBeenCalledWith(available);
    checker.stop();
  });

  it("uses the short delay when the core becomes ready during an in-flight failure", async () => {
    const scheduled: Array<{ fn: () => void; ms: number }> = [];
    const first = deferred<CheckAppUpdateResponse>();
    const check = vi
      .fn()
      .mockImplementationOnce(() => first.promise)
      .mockResolvedValueOnce(available);
    const onResult = vi.fn();
    const checker = startBackgroundAppUpdateCheck({
      check,
      onResult,
      schedule: (fn, ms) => {
        scheduled.push({ fn, ms });
        return scheduled.length;
      },
      cancel: () => {
        scheduled.length = 0;
      },
    });
    await Promise.resolve();
    checker.notifyCoreRunning();
    first.reject(new Error("net"));
    await checker.idle();
    expect(last(scheduled)?.ms).toBe(CORE_READY_RETRY_MS);
    checker.stop();
  });

  it("records the 24h cooldown after the 15-minute retry fails", async () => {
    const scheduled: Array<{ fn: () => void; ms: number }> = [];
    const recordCooldown = vi.fn().mockResolvedValue(undefined);
    const check = vi.fn().mockRejectedValue(new Error("net"));
    const onResult = vi.fn();
    const checker = startBackgroundAppUpdateCheck({
      check,
      onResult,
      recordCooldown,
      schedule: (fn, ms) => {
        scheduled.push({ fn, ms });
        return scheduled.length;
      },
      cancel: () => {},
    });
    await checker.idle();
    for (const delay of BACKGROUND_UPDATE_RETRY_MS) {
      expect(last(scheduled)?.ms).toBe(delay);
      last(scheduled)?.fn();
      await checker.idle();
    }
    expect(check).toHaveBeenCalledTimes(1 + BACKGROUND_UPDATE_RETRY_MS.length);
    expect(recordCooldown).toHaveBeenCalledTimes(1);
    expect(onResult).not.toHaveBeenCalled();
    expect(last(scheduled)?.ms).toBe(UPDATE_CHECK_INTERVAL_MS);
    const armed = scheduled.length;
    checker.notifyCoreRunning();
    expect(scheduled.length).toBe(armed);
    last(scheduled)?.fn();
    await checker.idle();
    expect(check).toHaveBeenCalledTimes(2 + BACKGROUND_UPDATE_RETRY_MS.length);
    expect(check).toHaveBeenNthCalledWith(
      BACKGROUND_UPDATE_RETRY_MS.length + 2,
      { startup: false },
    );
    checker.stop();
  });

  it("does not run a scheduled retry after stop", async () => {
    const scheduled: Array<{ fn: () => void; ms: number }> = [];
    const check = vi.fn().mockRejectedValue(new Error("net"));
    const checker = startBackgroundAppUpdateCheck({
      check,
      onResult: () => {},
      schedule: (fn, ms) => {
        scheduled.push({ fn, ms });
        return scheduled.length;
      },
      cancel: () => {},
    });
    await checker.idle();
    expect(check).toHaveBeenCalledTimes(1);
    const retry = scheduled[0]?.fn;
    checker.stop();
    retry?.();
    await checker.idle();
    expect(check).toHaveBeenCalledTimes(1);
  });
});
