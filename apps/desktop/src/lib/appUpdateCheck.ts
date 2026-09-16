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

/** In-session background checks repeat at most once per this interval, the
 * same 24h the backend cooldown enforces. The launch round ignores both, so
 * every app start checks once. */
export const UPDATE_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

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

/** Which round a check belongs to. `startup` marks the first round after app
 * start, which the backend runs even inside the 24h cooldown. */
export type BackgroundAppUpdateRound = {
  startup: boolean;
};

/**
 * An immediate launch check (`startup: true`, so the 24h cooldown cannot skip
 * it), then silent retries on failure until the 15-minute attempt fails (or a
 * check succeeds). Each closed round arms the next in-session round
 * `intervalMs` later, which is what limits the while-running check frequency;
 * retries stay inside the round, so they still count as the same check.
 * Exhaustion records the 24h cooldown without surfacing an error.
 */
export function startBackgroundAppUpdateCheck(options: {
  check: (round: BackgroundAppUpdateRound) => Promise<CheckAppUpdateResponse>;
  onResult: (result: CheckAppUpdateResponse) => void;
  recordCooldown?: () => Promise<void>;
  intervalMs?: number;
  schedule?: (fn: () => void, ms: number) => number;
  cancel?: (id: number) => void;
}): BackgroundAppUpdateChecker {
  const intervalMs = options.intervalMs ?? UPDATE_CHECK_INTERVAL_MS;
  const schedule =
    options.schedule ??
    ((fn, ms) => window.setTimeout(fn, ms) as unknown as number);
  const cancel = options.cancel ?? ((id) => window.clearTimeout(id));

  let stopped = false;
  let inFlight = false;
  let launchRound = true;
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
    if (stopped) return;
    clearTimer();
    timer = schedule(() => {
      timer = null;
      kick();
    }, ms);
  }

  function kick() {
    if (stopped || inFlight) return;
    idle = attempt();
  }

  /** Close the current round and arm the next in-session one. `recordCooldown`
   * persists the exhausted-failure cooldown when the round never reached
   * GitHub; a successful round already wrote `last_check_at` in the backend. */
  async function closeRound(recordCooldown: boolean) {
    launchRound = false;
    failures = 0;
    coreReadyQueued = false;
    clearTimer();
    if (recordCooldown && options.recordCooldown) {
      try {
        await options.recordCooldown();
      } catch {
        // Cooldown persist is best-effort; the session keeps its own schedule.
      }
    }
    arm(intervalMs);
  }

  async function attempt() {
    if (stopped || inFlight) return;
    const startup = launchRound;
    inFlight = true;
    try {
      const result = await options.check({ startup });
      if (stopped) return;
      options.onResult(result);
      await closeRound(false);
    } catch {
      if (stopped) return;
      failures += 1;
      const scheduled = backgroundUpdateRetryMs(failures - 1);
      if (scheduled === null) {
        await closeRound(true);
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
      if (stopped) return;
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
