// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Language primitives shared by the desktop app and the website.
 *
 * Plain DOM helpers (no React, no dictionaries): `i18n.tsx` re-exports them
 * for the app's existing imports, and the marketing site reuses them so both
 * sides agree on the storage key (`ice-box.language`) and on how a stored
 * preference resolves to a concrete language.
 */

/** Supported UI languages in standard international autonym order
 * (Latin A-Z, Cyrillic, Arabic/Persian, then East Asian scripts). */
export const SUPPORTED_LANGUAGES = [
  "id",
  "de",
  "en",
  "es",
  "fr",
  "pt-BR",
  "vi",
  "tr",
  "ru",
  "uk",
  "ar",
  "fa",
  "ja",
  "ko",
  "zh",
  "zh-TW",
] as const;

export type ResolvedLanguage = (typeof SUPPORTED_LANGUAGES)[number];
export type LanguagePreference = "system" | ResolvedLanguage;

/** Autonyms displayed in pickers regardless of the active UI language. */
export const LANGUAGE_LABELS: Record<ResolvedLanguage, string> = {
  id: "Bahasa Indonesia",
  de: "Deutsch",
  en: "English",
  es: "Español",
  fr: "Français",
  "pt-BR": "Português (BR)",
  vi: "Tiếng Việt",
  tr: "Türkçe",
  ru: "Русский",
  uk: "Українська",
  ar: "العربية",
  fa: "فارسی",
  ja: "日本語",
  ko: "한국어",
  zh: "简体中文",
  "zh-TW": "繁體中文",
};

export const LANGUAGE_STORAGE_KEY = "ice-box.language";

export function isResolvedLanguage(value: unknown): value is ResolvedLanguage {
  return (
    typeof value === "string" &&
    (SUPPORTED_LANGUAGES as readonly string[]).includes(value)
  );
}

export function isLanguagePreference(
  value: unknown,
): value is LanguagePreference {
  return value === "system" || isResolvedLanguage(value);
}

export function isRtlLanguage(language: ResolvedLanguage): boolean {
  return language === "fa" || language === "ar";
}

export function languageDirection(language: ResolvedLanguage): "ltr" | "rtl" {
  return isRtlLanguage(language) ? "rtl" : "ltr";
}

/** Map a BCP-47 / POSIX / Windows locale tag to the closest supported language. */
export function resolveLocaleTag(tag: string): ResolvedLanguage {
  const trimmed = tag.trim();
  if (!trimmed) return "en";
  // Drop POSIX encoding/modifier suffixes (e.g. `.UTF-8`, `@euro`) first.
  const base = trimmed.split(/[.@]/, 1)[0] ?? trimmed;
  const parts = base
    .split(/[-_\s()]+/)
    .filter(Boolean)
    .map((part) => part.toLowerCase());
  const primary = parts[0];
  if (!primary) return "en";

  if (primary === "zh" || primary === "chinese") {
    const isTraditional = parts
      .slice(1)
      .some(
        (sub) =>
          sub === "hant" ||
          sub === "tw" ||
          sub === "hk" ||
          sub === "mo" ||
          sub === "taiwan" ||
          sub === "hongkong" ||
          sub === "traditional",
      );
    return isTraditional ? "zh-TW" : "zh";
  }
  if (primary === "ru" || primary === "russian") return "ru";
  if (primary === "ja" || primary === "jpn" || primary === "japanese") return "ja";
  if (
    primary === "fa" ||
    primary === "pes" ||
    primary === "prs" ||
    primary === "persian" ||
    primary === "farsi"
  ) {
    return "fa";
  }
  if (primary === "ar" || primary === "ara" || primary === "arabic") return "ar";
  if (primary === "ko" || primary === "kor" || primary === "korean") return "ko";
  if (primary === "tr" || primary === "tur" || primary === "turkish") return "tr";
  if (primary === "vi" || primary === "vie" || primary === "vietnamese") {
    return "vi";
  }
  if (
    primary === "id" ||
    primary === "in" ||
    primary === "ind" ||
    primary === "indonesian"
  ) {
    return "id";
  }
  if (primary === "es" || primary === "spa" || primary === "spanish") return "es";
  if (
    primary === "de" ||
    primary === "deu" ||
    primary === "ger" ||
    primary === "german"
  ) {
    return "de";
  }
  if (
    primary === "fr" ||
    primary === "fra" ||
    primary === "fre" ||
    primary === "french"
  ) {
    return "fr";
  }
  if (primary === "pt" || primary === "por" || primary === "portuguese") {
    return "pt-BR";
  }
  if (primary === "uk" || primary === "ukr" || primary === "ukrainian") {
    return "uk";
  }
  return "en";
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
  const lang = typeof navigator.language === "string" ? navigator.language : "en";
  return resolveLocaleTag(lang);
}

export function resolveLanguage(
  preference: LanguagePreference,
): ResolvedLanguage {
  if (preference === "system") return systemLanguage();
  return preference;
}
