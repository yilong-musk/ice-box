// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Marketing copy for the website (landing page + Live Demo window chrome).
 *
 * The site keeps its own small dictionaries (the app dictionaries have no
 * marketing strings). What both sides share is `desktop/src/lib/language.ts`:
 * the same `ice-box.language` storage key and the same resolution rules, so a
 * language pick on either side follows the visitor everywhere.
 */

import {
  languageDirection,
  readLanguagePreference,
  resolveLanguage,
  type ResolvedLanguage,
} from "../../desktop/src/lib/language.ts";
import { zh, type MarketingKey } from "./locales/zh.ts";
import { zhTw } from "./locales/zh-TW.ts";
import { en } from "./locales/en.ts";
import { ru } from "./locales/ru.ts";
import { ja } from "./locales/ja.ts";
import { fa } from "./locales/fa.ts";
import { ar } from "./locales/ar.ts";
import { ko } from "./locales/ko.ts";
import { tr } from "./locales/tr.ts";
import { vi } from "./locales/vi.ts";
import { id } from "./locales/id.ts";
import { es } from "./locales/es.ts";
import { de } from "./locales/de.ts";
import { fr } from "./locales/fr.ts";
import { ptBr } from "./locales/pt-BR.ts";
import { uk } from "./locales/uk.ts";

export type { MarketingKey } from "./locales/zh.ts";

export const marketingDictionaries: Record<
  ResolvedLanguage,
  Record<MarketingKey, string>
> = {
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

export function marketingText(
  key: MarketingKey,
  language: ResolvedLanguage,
): string {
  return marketingDictionaries[language][key];
}

/** Stored preference when the visitor picked one, else the system locale. */
export function resolveInitialLanguage(): ResolvedLanguage {
  return resolveLanguage(readLanguagePreference());
}

/** Document-level language signals: `<html lang>`, `dir`, title, meta description. */
export function applyMarketingLanguage(language: ResolvedLanguage): void {
  document.documentElement.lang = language;
  document.documentElement.dir = languageDirection(language);
  document.title = marketingText("head.title", language);
  const description = document.querySelector('meta[name="description"]');
  if (description) {
    description.setAttribute(
      "content",
      marketingText("head.description", language),
    );
  }
}
