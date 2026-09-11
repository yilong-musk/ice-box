// SPDX-License-Identifier: GPL-3.0-or-later

import { useCallback, useRef } from "react";

/** Invalidates in-flight async work when generation changes (mutations / tab switches). */
export function useGenerationGuard() {
  const genRef = useRef(0);

  const nextGeneration = useCallback(() => {
    genRef.current += 1;
    return genRef.current;
  }, []);

  const isStale = useCallback((gen: number) => gen !== genRef.current, []);

  return { nextGeneration, isStale };
}
