// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * UI internationalization (16 languages, including RTL support for `fa` and `ar`).
 *
 * `t()` is module-global: it reads the currently active language, so plain
 * (non-component) helpers can call it directly. Components re-render on
 * language change through `useLanguagePreference()`, which subscribes to a
 * single module-level store instead of keeping per-component state: pages
 * hidden inside `<Activity>` drop their effects, and a per-component copy
 * would come back stale and re-apply the old language. The preference follows
 * the theme module's pattern: localStorage cache for an instant boot apply,
 * plus an app-level custom event for cross-component sync. The Settings page
 * additionally persists `language` in `settings.json` (authoritative) and
 * re-applies it whenever settings are (re)loaded. The storage key and the
 * resolution helpers themselves live in `./language`, shared with the website.
 */

import { useSyncExternalStore } from "react";
import {
  isLanguagePreference,
  languageDirection,
  readLanguagePreference,
  resolveLanguage,
  LANGUAGE_STORAGE_KEY,
  type LanguagePreference,
  type ResolvedLanguage,
} from "./language";
import { zh, type MessageKey } from "./locales/zh";
import { zhTw } from "./locales/zh-TW";
import { en } from "./locales/en";
import { ru } from "./locales/ru";
import { ja } from "./locales/ja";
import { fa } from "./locales/fa";
import { ar } from "./locales/ar";
import { ko } from "./locales/ko";
import { tr } from "./locales/tr";
import { vi } from "./locales/vi";
import { id } from "./locales/id";
import { es } from "./locales/es";
import { de } from "./locales/de";
import { fr } from "./locales/fr";
import { ptBr } from "./locales/pt-BR";
import { uk } from "./locales/uk";

// Re-exported so every existing `lib/i18n` import keeps working.
export {
  LANGUAGE_LABELS,
  LANGUAGE_STORAGE_KEY,
  SUPPORTED_LANGUAGES,
  isLanguagePreference,
  isResolvedLanguage,
  isRtlLanguage,
  languageDirection,
  readLanguagePreference,
  resolveLanguage,
  resolveLocaleTag,
  systemLanguage,
} from "./language";
export type { LanguagePreference, ResolvedLanguage } from "./language";
export type { MessageKey } from "./locales/zh";

export const LANGUAGE_CHANGE_EVENT = "ice-box-language";

export function isMessageKey(key: string): key is MessageKey {
  return Object.prototype.hasOwnProperty.call(zh, key);
}

export const dictionaries: Record<ResolvedLanguage, Record<MessageKey, string>> = {
  zh,
  "zh-TW": zhTw,
  en,
  ru,
  ja,
  fa,
  ar,
  ko,
  tr,
  vi,
  id,
  es,
  de,
  fr,
  "pt-BR": ptBr,
  uk,
};

const FORMAT_LOCALES: Record<ResolvedLanguage, string> = {
  zh: "zh-CN",
  "zh-TW": "zh-TW",
  en: "en-US",
  ru: "ru-RU",
  ja: "ja-JP",
  fa: "fa-IR",
  ar: "ar-SA",
  ko: "ko-KR",
  tr: "tr-TR",
  vi: "vi-VN",
  id: "id-ID",
  es: "es-ES",
  de: "de-DE",
  fr: "fr-FR",
  "pt-BR": "pt-BR",
  uk: "uk-UA",
};

type LanguageSnapshot = {
  preference: LanguagePreference;
  resolved: ResolvedLanguage;
};

/** The one language state for the whole document. Initialized lazily so the
 * module can be imported before the environment's locale is fully available
 * (e.g. test setup files), then replaced by `applyLanguage`. */
let snapshot: LanguageSnapshot | undefined;
const listeners = new Set<() => void>();

function currentSnapshot(): LanguageSnapshot {
  if (!snapshot) {
    const preference = readLanguagePreference();
    snapshot = { preference, resolved: resolveLanguage(preference) };
  }
  return snapshot;
}

/** BCP 47 locale for formatting dates and numbers in the app language rather
 * than the OS locale, so a localized UI on an English system stays consistent. */
export function formatLocale(
  lang: ResolvedLanguage = currentSnapshot().resolved,
): string {
  return FORMAT_LOCALES[lang];
}

/** Translate a message key; `{name}` placeholders are replaced from params. */
export function t(
  key: MessageKey,
  params?: Record<string, string | number>,
): string {
  const template = dictionaries[currentSnapshot().resolved][key];
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    params[name] !== undefined ? String(params[name]) : match,
  );
}

/** Apply a preference: resolves it, updates the active dictionary and the
 * document `lang` / `dir` attributes, and re-renders every subscribed
 * component. Returns the resolved language. */
export function applyLanguage(preference: LanguagePreference): ResolvedLanguage {
  const resolved = resolveLanguage(preference);
  document.documentElement.lang = resolved;
  document.documentElement.dir = languageDirection(resolved);
  const prev = snapshot;
  if (prev?.preference !== preference || prev.resolved !== resolved) {
    snapshot = { preference, resolved };
    for (const listener of listeners) listener();
  }
  return resolved;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

if (typeof window !== "undefined") {
  // Changes made elsewhere (another component, the website's storage bridge)
  // arrive as this event; one module-level listener keeps the store current
  // even while every subscribed page is hidden.
  window.addEventListener(LANGUAGE_CHANGE_EVENT, (event) => {
    const next = (event as CustomEvent<unknown>).detail;
    if (isLanguagePreference(next)) applyLanguage(next);
  });
  // Re-resolve when the OS locale changes while following the system.
  window.addEventListener("languagechange", () => {
    if (currentSnapshot().preference === "system") applyLanguage("system");
  });
}

export function persistLanguagePreference(
  preference: LanguagePreference,
): void {
  if (!isLanguagePreference(preference)) return;
  try {
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, preference);
  } catch {
    // The preference still applies for this session.
  }
  applyLanguage(preference);
  window.dispatchEvent(
    new CustomEvent(LANGUAGE_CHANGE_EVENT, { detail: preference }),
  );
}

/** Apply the stored preference as soon as the module loads (reduces flash). */
export function applyStoredLanguage(): void {
  applyLanguage(readLanguagePreference());
}

export function useLanguagePreference() {
  const { preference, resolved } = useSyncExternalStore(
    subscribe,
    currentSnapshot,
  );
  return {
    preference,
    resolved,
    setPreference: persistLanguagePreference,
  };
}
