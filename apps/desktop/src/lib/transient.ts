// SPDX-License-Identifier: GPL-3.0-or-later

import { useCallback, useLayoutEffect, useRef, useState } from "react";

type Shown<T> = { value: T };

/**
 * A value that reverts to `idle` a while after it is shown, such as a toast or
 * a "Copied" / "Saved" flash. `show(value, ms)` replaces any value still up.
 *
 * Pages stay mounted inside a hidden <Activity>, which tears down effects (and
 * the timers they own) but keeps state. A timer started in an event handler
 * and cleared on unmount is therefore lost on a tab switch, leaving the value
 * up for good. Here the countdown lives in an effect and only runs while the
 * page is visible: hiding pauses it, revealing resumes it with the time left.
 * A value shown while hidden (e.g. a background refresh error) is still seen
 * for its full time, and elapsed time is measured with `performance.now()`,
 * so a wall-clock jump cannot stretch or cut it.
 */
export function useTransient<T>(idle: T): [T, (value: T, ms: number) => void, () => void] {
  const [shown, setShown] = useState<Shown<T> | null>(null);
  // Time left on `shown`, tied to it so a replaced value's cleanup cannot
  // charge its elapsed time to the next one.
  const remaining = useRef<{ shown: Shown<T>; ms: number } | null>(null);

  const show = useCallback((value: T, ms: number) => {
    const next = { value };
    remaining.current = { shown: next, ms };
    setShown(next);
  }, []);

  const clear = useCallback(() => {
    remaining.current = null;
    setShown(null);
  }, []);

  // A layout effect, so a value whose time ran out is dropped before the
  // revealed page paints.
  useLayoutEffect(() => {
    if (!shown) return;
    const left = remaining.current?.shown === shown ? remaining.current.ms : 0;
    const expire = () => setShown((current) => (current === shown ? null : current));
    if (left <= 0) {
      expire();
      return;
    }
    const startedAt = performance.now();
    const id = window.setTimeout(expire, left);
    return () => {
      window.clearTimeout(id);
      const entry = remaining.current;
      if (entry?.shown === shown) {
        entry.ms = Math.max(0, entry.ms - (performance.now() - startedAt));
      }
    };
  }, [shown]);

  return [shown ? shown.value : idle, show, clear];
}
