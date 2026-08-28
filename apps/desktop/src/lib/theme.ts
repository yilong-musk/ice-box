import { useEffect, useState } from "react";

export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

export const THEME_STORAGE_KEY = "ice-box.theme";
export const THEME_CHANGE_EVENT = "ice-box-theme";

export function isThemePreference(value: unknown): value is ThemePreference {
  return value === "system" || value === "light" || value === "dark";
}

export function readThemePreference(): ThemePreference {
  try {
    const raw = window.localStorage.getItem(THEME_STORAGE_KEY);
    if (isThemePreference(raw)) return raw;
  } catch {
    // Private mode / blocked storage: stay on the default.
  }
  return "system";
}

export function systemPrefersDark(): boolean {
  if (typeof window.matchMedia !== "function") return false;
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function resolveTheme(preference: ThemePreference): ResolvedTheme {
  if (preference === "system") return systemPrefersDark() ? "dark" : "light";
  return preference;
}

export function applyTheme(preference: ThemePreference): ResolvedTheme {
  const resolved = resolveTheme(preference);
  const root = document.documentElement;
  root.classList.toggle("dark", resolved === "dark");
  root.style.colorScheme = resolved;
  return resolved;
}

export function persistThemePreference(preference: ThemePreference): void {
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, preference);
  } catch {
    // Theme still applies for this session.
  }
  applyTheme(preference);
  window.dispatchEvent(new CustomEvent(THEME_CHANGE_EVENT, { detail: preference }));
}

/** Apply stored preference as soon as the module loads (reduces flash after CSS). */
export function applyStoredTheme(): void {
  applyTheme(readThemePreference());
}

export function useThemePreference() {
  const [preference, setPreferenceState] = useState<ThemePreference>(readThemePreference);
  const [resolved, setResolved] = useState<ResolvedTheme>(() =>
    resolveTheme(readThemePreference()),
  );

  useEffect(() => {
    setResolved(applyTheme(preference));
    if (preference !== "system") return;
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    if (!mq) return;
    const onChange = () => setResolved(applyTheme("system"));
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [preference]);

  useEffect(() => {
    const onCustom = (event: Event) => {
      const next = (event as CustomEvent<ThemePreference>).detail;
      if (!isThemePreference(next)) return;
      setPreferenceState(next);
      setResolved(resolveTheme(next));
    };
    window.addEventListener(THEME_CHANGE_EVENT, onCustom);
    return () => window.removeEventListener(THEME_CHANGE_EVENT, onCustom);
  }, []);

  function setPreference(next: ThemePreference) {
    persistThemePreference(next);
    setPreferenceState(next);
    setResolved(resolveTheme(next));
  }

  return {
    preference,
    resolved,
    setPreference,
  };
}
