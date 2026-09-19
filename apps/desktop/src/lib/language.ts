// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Language primitives shared by the desktop app and the website.
 *
 * Plain DOM helpers (no React, no dictionaries): `i18n.tsx` re-exports them
 * for the app's existing imports, and the marketing site reuses them so both
 * sides agree on the storage key (`ice-box.language`) and on how a stored
 * preference resolves to a concrete language.
 */

export type LanguagePreference = "system" | "zh" | "en";
export type ResolvedLanguage = "zh" | "en";

export const LANGUAGE_STORAGE_KEY = "ice-box.language";

export function isLanguagePreference(
  value: unknown,
): value is LanguagePreference {
  return value === "system" || value === "zh" || value === "en";
}

export function readLanguagePreference(): LanguagePreference {
  try {
    const raw = window.localStorage.getItem(LANGUAGE_STORAGE_KEY);
    if (isLanguagePreference(raw)) return raw;
  } catch {
    // Private mode / blocked storage: stay on the default.
  }
  return "system";
}

/** Closest supported language for the current system locale. */
export function systemLanguage(): ResolvedLanguage {
  const lang = (typeof navigator.language === "string"
    ? navigator.language
    : "en"
  ).toLowerCase();
  return lang.startsWith("zh") ? "zh" : "en";
}

export function resolveLanguage(
  preference: LanguagePreference,
): ResolvedLanguage {
  if (preference === "system") return systemLanguage();
  return preference;
}
