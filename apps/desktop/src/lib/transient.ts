// SPDX-License-Identifier: GPL-3.0-or-later

import { useCallback, useLayoutEffect, useState } from "react";

type Shown<T> = {
  value: T;
  /** `performance.now()` time the value goes away. */
  deadline: number;
};

/**
 * A value that reverts to `idle` a while after it is shown, such as a toast or
 * a "Copied" / "Saved" flash. `show(value, ms)` replaces any value still up.
 *
 * Pages stay mounted inside a hidden <Activity>, which tears down effects (and
 * the timers they own) but keeps state. A timer started in an event handler
 * and cleared on unmount is therefore lost on a tab switch, leaving the value
 * up for good. Here the value carries its deadline and an effect counts down
 * to it: the time keeps running while the page is hidden, and a reveal after
 * the deadline drops the value before it paints, so it is not shown again.
 * The deadline is on the `performance.now()` clock, so a wall-clock jump
 * cannot stretch or cut it.
 */
export function useTransient<T>(idle: T): [T, (value: T, ms: number) => void, () => void] {
  const [shown, setShown] = useState<Shown<T> | null>(null);

  const show = useCallback((value: T, ms: number) => {
    setShown({ value, deadline: performance.now() + ms });
  }, []);

  const clear = useCallback(() => {
    setShown(null);
  }, []);

  // A layout effect, so a value whose time ran out while hidden is dropped
  // before the revealed page paints.
  useLayoutEffect(() => {
    if (!shown) return;
    const expire = () => setShown((current) => (current === shown ? null : current));
    const left = shown.deadline - performance.now();
    if (left <= 0) {
      expire();
      return;
    }
    const id = window.setTimeout(expire, left);
    return () => window.clearTimeout(id);
  }, [shown]);

  return [shown ? shown.value : idle, show, clear];
}
