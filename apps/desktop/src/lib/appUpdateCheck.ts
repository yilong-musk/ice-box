// SPDX-License-Identifier: GPL-3.0-or-later

import type { CheckAppUpdateResponse } from "../api/tauri";

/** Backoff after a failed background GitHub check. After the 15-minute retry
 * still fails, the ladder is exhausted and the 24h cooldown starts. */
export const BACKGROUND_UPDATE_RETRY_MS = [
  10_000, 30_000, 60_000, 120_000, 300_000, 900_000,
] as const;

/** When the core becomes Running after a failed attempt, retry soon so Mixed
 * can proxy GitHub instead of waiting out the current backoff. */
export const CORE_READY_RETRY_MS = 2_000;

/** Delay before the next retry, or `null` when the 15-minute retry already
 * ran and failed (start the 24h cooldown). `failureCount` is 0 after the
 * first failed attempt. */
export function backgroundUpdateRetryMs(failureCount: number): number | null {
  if (failureCount < 0) {
    return BACKGROUND_UPDATE_RETRY_MS[0];
  }
  if (failureCount >= BACKGROUND_UPDATE_RETRY_MS.length) {
    return null;
  }
  return BACKGROUND_UPDATE_RETRY_MS[failureCount];
}

export type BackgroundAppUpdateChecker = {
  stop: () => void;
  notifyCoreRunning: () => void;
  /** Settles when the in-flight attempt (if any) finishes. */
  idle: () => Promise<void>;
};

/**
 * One immediate background check, then silent retries on failure until the
 * 15-minute attempt fails (or a check succeeds). Exhaustion records the 24h
 * cooldown without surfacing an error.
 */
export function startBackgroundAppUpdateCheck(options: {
  check: () => Promise<CheckAppUpdateResponse>;
  onResult: (result: CheckAppUpdateResponse) => void;
  recordCooldown?: () => Promise<void>;
  schedule?: (fn: () => void, ms: number) => number;
  cancel?: (id: number) => void;
}): BackgroundAppUpdateChecker {
  const schedule =
    options.schedule ??
    ((fn, ms) => window.setTimeout(fn, ms) as unknown as number);
  const cancel = options.cancel ?? ((id) => window.clearTimeout(id));

  let stopped = false;
  let inFlight = false;
  let done = false;
  let failures = 0;
  let timer: number | null = null;
  let coreReadyQueued = false;
  let idle: Promise<void> = Promise.resolve();

  function clearTimer() {
    if (timer !== null) {
      cancel(timer);
      timer = null;
    }
  }

  function arm(ms: number) {
    if (stopped || done) return;
    clearTimer();
    timer = schedule(() => {
      timer = null;
      kick();
    }, ms);
  }

  function kick() {
    if (stopped || done || inFlight) return;
    idle = attempt();
  }

  async function finishWithCooldown() {
    done = true;
    clearTimer();
    if (!options.recordCooldown) return;
    try {
      await options.recordCooldown();
    } catch {
      // Cooldown persist is best-effort; the session still stops retrying.
    }
  }

  async function attempt() {
    if (stopped || done || inFlight) return;
    inFlight = true;
    try {
      const result = await options.check();
      if (stopped) return;
      done = true;
      clearTimer();
      options.onResult(result);
    } catch {
      if (stopped) return;
      failures += 1;
      const scheduled = backgroundUpdateRetryMs(failures - 1);
      if (scheduled === null) {
        await finishWithCooldown();
        return;
      }
      const delay = coreReadyQueued ? CORE_READY_RETRY_MS : scheduled;
      coreReadyQueued = false;
      arm(delay);
    } finally {
      inFlight = false;
    }
  }

  kick();

  return {
    stop() {
      stopped = true;
      clearTimer();
    },
    notifyCoreRunning() {
      if (stopped || done) return;
      if (inFlight) {
        coreReadyQueued = true;
        return;
      }
      if (failures === 0) return;
      coreReadyQueued = false;
      arm(CORE_READY_RETRY_MS);
    },
    idle() {
      return idle;
    },
  };
}
