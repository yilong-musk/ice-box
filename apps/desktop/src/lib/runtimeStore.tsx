// SPDX-License-Identifier: GPL-3.0-or-later

/** Shared runtime status: one poller, visibility-aware, generation-checked (PERF-2 / FE-1). */

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { api, type StatusResponse } from "../api/tauri";

const STATUS_FALLBACK_MS = 10_000;

export type RuntimeStore = {
  status: StatusResponse | null;
  visible: boolean;
  bumpGeneration: () => number;
  isStale: (gen: number) => boolean;
  refreshStatus: () => Promise<StatusResponse | null>;
};

const RuntimeStoreContext = createContext<RuntimeStore | null>(null);

async function listenOptional(
  fn: ((handler: () => void) => Promise<() => void>) | undefined,
  handler: () => void,
): Promise<() => void> {
  if (typeof fn !== "function") return () => {};
  try {
    return await fn(handler);
  } catch {
    return () => {};
  }
}

function useRuntimeStoreEngine(enabled: boolean): RuntimeStore {
  const [status, setStatus] = useState<StatusResponse | null>(null);
  const [visible, setVisible] = useState(() =>
    typeof document === "undefined"
      ? true
      : document.visibilityState !== "hidden",
  );
  const genRef = useRef(0);

  const bumpGeneration = useCallback(() => {
    genRef.current += 1;
    return genRef.current;
  }, []);

  const isStale = useCallback((gen: number) => gen !== genRef.current, []);

  const refreshStatus = useCallback(async () => {
    const gen = genRef.current;
    try {
      const next = await api.getStatus();
      if (gen !== genRef.current) return null;
      setStatus(next);
      return next;
    } catch {
      return null;
    }
  }, []);

  useEffect(() => {
    if (!enabled) return;
    const onVis = () => {
      setVisible(document.visibilityState !== "hidden");
    };
    document.addEventListener("visibilitychange", onVis);
    let unHidden = () => {};
    let unShown = () => {};
    let unCore = () => {};
    let unState = () => {};
    void listenOptional(api.listenWindowHidden, () => setVisible(false)).then(
      (u) => {
        unHidden = u;
      },
    );
    void listenOptional(api.listenWindowShown, () => setVisible(true)).then(
      (u) => {
        unShown = u;
      },
    );
    void listenOptional(api.listenCoreStatusChanged, () => {
      void refreshStatus();
    }).then((u) => {
      unCore = u;
    });
    void listenOptional(api.listenStateChanged, () => {
      void refreshStatus();
    }).then((u) => {
      unState = u;
    });
    return () => {
      document.removeEventListener("visibilitychange", onVis);
      unHidden();
      unShown();
      unCore();
      unState();
    };
  }, [enabled, refreshStatus]);

  useEffect(() => {
    if (!enabled || !visible) return;
    void refreshStatus();
    const id = window.setInterval(() => void refreshStatus(), STATUS_FALLBACK_MS);
    return () => window.clearInterval(id);
  }, [enabled, visible, refreshStatus]);

  return { status, visible, bumpGeneration, isStale, refreshStatus };
}

export function RuntimeStoreProvider({ children }: { children: ReactNode }) {
  const store = useRuntimeStoreEngine(true);
  return (
    <RuntimeStoreContext.Provider value={store}>
      {children}
    </RuntimeStoreContext.Provider>
  );
}

/** Shared runtime status. Pages fall back to their own fetches when unwrapped (tests). */
export function useRuntimeStore(): RuntimeStore | null {
  return useContext(RuntimeStoreContext);
}

export const RUNTIME_STATUS_FALLBACK_MS = STATUS_FALLBACK_MS;
