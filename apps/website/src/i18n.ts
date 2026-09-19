// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Marketing copy for the website (landing page + Live Demo window chrome).
 *
 * The site keeps its own small dictionary (the app dictionary has no
 * marketing strings). What both sides share is `desktop/src/lib/language.ts`:
 * the same `ice-box.language` storage key and the same resolution rules, so a
 * language pick on either side follows the visitor everywhere.
 */

import {
  readLanguagePreference,
  resolveLanguage,
  type ResolvedLanguage,
} from "../../desktop/src/lib/language";

/** The zh dictionary is the source of truth for message keys. */
const zh = {
  "head.title": "ice-box — 代理，保持简单",
  "head.description":
    "ice-box — 面向 macOS 与 Windows 的轻量代理客户端。上手简单、启动迅速、运行安静。",
  "hero.title": "代理，<br><em>保持简单。</em>",
  "hero.demo": "在线演示",
  "hero.download": "下载 ↗",
  "demo.windowBar": "实时产品演示",
  "demo.iframeTitle": "ice-box 真实桌面端界面演示",
  "nav.languageToggle": "EN",
  "nav.languageToggleAria": "切换到英文",
} as const;

export type MarketingKey = keyof typeof zh;

const en: Record<MarketingKey, string> = {
  "head.title": "ice-box — proxy, kept simple",
  "head.description":
    "ice-box — a lightweight proxy client for macOS and Windows. Easy to set up, quick to start, quiet once it runs.",
  "hero.title": "Proxy,<br><em>kept simple.</em>",
  "hero.demo": "Live Demo",
  "hero.download": "Download ↗",
  "demo.windowBar": "live product demo",
  "demo.iframeTitle": "ice-box real desktop frontend demo",
  "nav.languageToggle": "中文",
  "nav.languageToggleAria": "Switch to Chinese",
};

const dictionaries: Record<ResolvedLanguage, Record<MarketingKey, string>> = {
  zh,
  en,
};

export function marketingText(
  key: MarketingKey,
  language: ResolvedLanguage,
): string {
  return dictionaries[language][key];
}

/** Stored preference when the visitor picked one, else the system locale. */
export function resolveInitialLanguage(): ResolvedLanguage {
  return resolveLanguage(readLanguagePreference());
}

/** Document-level language signals: `<html lang>`, title, meta description. */
export function applyMarketingLanguage(language: ResolvedLanguage): void {
  document.documentElement.lang = language;
  document.title = marketingText("head.title", language);
  const description = document.querySelector('meta[name="description"]');
  if (description) {
    description.setAttribute(
      "content",
      marketingText("head.description", language),
    );
  }
}
