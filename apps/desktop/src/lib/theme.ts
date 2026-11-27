// SPDX-License-Identifier: GPL-3.0-or-later

import { useEffect, useSyncExternalStore } from "react";

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

type ThemeSnapshot = {
  preference: ThemePreference;
  resolved: ResolvedTheme;
};

/** Document-wide theme state shared across all hooks. Keeping it outside React
 * state ensures a page hidden inside `<Activity mode="hidden">` (whose effects
 * are torn down while state is kept) never retains a stale preference and
 * reverts the document theme when shown again. */
let snapshot: ThemeSnapshot | undefined;
/** Fallback when localStorage is unavailable or a synthetic event lands before
 * storage reflects it. Cleared once storage matches or `applyStoredTheme` runs. */
let sessionPreference: ThemePreference | null = null;
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

function currentSnapshot(): ThemeSnapshot {
  const preference = sessionPreference ?? readThemePreference();
  const resolved = resolveTheme(preference);
  if (
    !snapshot ||
    snapshot.preference !== preference ||
    snapshot.resolved !== resolved
  ) {
    snapshot = { preference, resolved };
  }
  return snapshot;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function applyTheme(preference: ThemePreference): ResolvedTheme {
  const resolved = resolveTheme(preference);
  const root = document.documentElement;
  root.classList.toggle("dark", resolved === "dark");
  root.style.colorScheme = resolved;
  if (
    !snapshot ||
    snapshot.preference !== preference ||
    snapshot.resolved !== resolved
  ) {
    snapshot = { preference, resolved };
    notify();
  }
  return resolved;
}

if (typeof window !== "undefined") {
  // Changes made elsewhere (another component, the website's storage bridge)
  // arrive as this event; one module-level listener keeps the store current
  // even while every subscribed page is hidden.
  window.addEventListener(THEME_CHANGE_EVENT, (event) => {
    const next = (event as CustomEvent<unknown>).detail;
    if (!isThemePreference(next)) return;
    sessionPreference = readThemePreference() === next ? null : next;
    applyTheme(next);
  });
}

export function persistThemePreference(preference: ThemePreference): void {
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, preference);
    sessionPreference = null;
  } catch {
    // Theme still applies for this session.
    sessionPreference = preference;
  }
  applyTheme(preference);
  window.dispatchEvent(new CustomEvent(THEME_CHANGE_EVENT, { detail: preference }));
}

/** Apply stored preference as soon as the module loads (reduces flash after CSS). */
export function applyStoredTheme(): void {
  sessionPreference = null;
  applyTheme(readThemePreference());
}

export function useThemePreference() {
  const { preference, resolved } = useSyncExternalStore(
    subscribe,
    currentSnapshot,
  );

  useEffect(() => {
    applyTheme(preference);
    if (preference !== "system") return;
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    if (!mq) return;
    const onChange = () => applyTheme("system");
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [preference]);

  return {
    preference,
    resolved,
    setPreference: persistThemePreference,
  };
}
