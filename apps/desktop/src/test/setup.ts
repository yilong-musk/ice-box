import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";
import { THEME_STORAGE_KEY } from "../lib/theme";

function ensureLocalStorage() {
  try {
    window.localStorage.setItem("__ice-box-ls", "1");
    window.localStorage.removeItem("__ice-box-ls");
    return;
  } catch {
    // Node 25+ installs a localStorage getter on globalThis. Vitest's jsdom
    // environment aliases window to globalThis, so that getter shadows Storage
    // unless --localstorage-file is set.
  }

  const store = new Map<string, string>();
  const localStorage: Storage = {
    getItem(key) {
      return store.has(key) ? store.get(key)! : null;
    },
    setItem(key, value) {
      store.set(key, String(value));
    },
    removeItem(key) {
      store.delete(key);
    },
    clear() {
      store.clear();
    },
    key(index) {
      return [...store.keys()][index] ?? null;
    },
    get length() {
      return store.size;
    },
  };
  Object.defineProperty(window, "localStorage", {
    configurable: true,
    enumerable: true,
    value: localStorage,
  });
}

ensureLocalStorage();

if (typeof window.ResizeObserver === "undefined") {
  window.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as typeof ResizeObserver;
}

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  window.localStorage.removeItem(THEME_STORAGE_KEY);
  document.documentElement.classList.remove("dark");
  document.documentElement.style.colorScheme = "";
});
