import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  THEME_STORAGE_KEY,
  applyStoredTheme,
  applyTheme,
  persistThemePreference,
  readThemePreference,
  resolveTheme,
  systemPrefersDark,
  useThemePreference,
} from "./theme";

function stubMatchMedia(dark: boolean) {
  let matches = dark;
  const listeners = new Set<(event: Event) => void>();
  const mq = {
    get matches() {
      return matches;
    },
    media: "(prefers-color-scheme: dark)",
    addEventListener: (type: string, listener: EventListener) => {
      if (type === "change") listeners.add(listener);
    },
    removeEventListener: (type: string, listener: EventListener) => {
      if (type === "change") listeners.delete(listener);
    },
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
    onchange: null,
    setDark(next: boolean) {
      matches = next;
      listeners.forEach((listener) =>
        listener({ matches: next } as MediaQueryListEvent),
      );
    },
  };
  vi.stubGlobal("matchMedia", (query: string) => {
    if (query.includes("prefers-color-scheme: dark")) return mq;
    return { ...mq, matches: false, setDark: undefined };
  });
  return mq;
}

describe("theme", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    window.localStorage.removeItem(THEME_STORAGE_KEY);
    document.documentElement.classList.remove("dark");
    document.documentElement.style.colorScheme = "";
  });

  it("defaults to following the system", () => {
    expect(readThemePreference()).toBe("system");
  });

  it("ignores invalid stored values", () => {
    window.localStorage.setItem(THEME_STORAGE_KEY, "nope");
    expect(readThemePreference()).toBe("system");
  });

  it("resolves system preference from matchMedia", () => {
    stubMatchMedia(true);
    expect(systemPrefersDark()).toBe(true);
    expect(resolveTheme("system")).toBe("dark");
    stubMatchMedia(false);
    expect(resolveTheme("system")).toBe("light");
  });

  it("applies an explicit light or dark class", () => {
    stubMatchMedia(true);
    applyTheme("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    expect(document.documentElement.style.colorScheme).toBe("light");
    applyTheme("dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    expect(document.documentElement.style.colorScheme).toBe("dark");
  });

  it("persists the preference and restores it", () => {
    persistThemePreference("light");
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe("light");
    expect(readThemePreference()).toBe("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });

  it("applies the stored preference on bootstrap", () => {
    window.localStorage.setItem(THEME_STORAGE_KEY, "dark");
    applyStoredTheme();
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    expect(document.documentElement.style.colorScheme).toBe("dark");
  });

  it("follows OS scheme changes while preference is system", () => {
    const mq = stubMatchMedia(false);
    const { result } = renderHook(() => useThemePreference());

    expect(result.current.preference).toBe("system");
    expect(result.current.resolved).toBe("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);

    act(() => {
      mq.setDark(true);
    });
    expect(result.current.preference).toBe("system");
    expect(result.current.resolved).toBe("dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
  });

  it("does not follow OS scheme changes when an explicit theme is set", () => {
    const mq = stubMatchMedia(false);
    const { result } = renderHook(() => useThemePreference());

    act(() => {
      result.current.setPreference("light");
    });
    act(() => {
      mq.setDark(true);
    });
    expect(result.current.preference).toBe("light");
    expect(result.current.resolved).toBe("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });

  it("syncs preference across hook instances", () => {
    stubMatchMedia(false);
    const { result: first } = renderHook(() => useThemePreference());
    const { result: second } = renderHook(() => useThemePreference());

    act(() => {
      first.current.setPreference("dark");
    });
    expect(second.current.preference).toBe("dark");
    expect(second.current.resolved).toBe("dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);

    act(() => {
      persistThemePreference("light");
    });
    expect(first.current.preference).toBe("light");
    expect(second.current.preference).toBe("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });
});
