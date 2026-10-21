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
    "ice-box — 面向 macOS、Windows 与 Android 的轻量代理客户端。上手简单、启动迅速，运行时占用小。",
  "hero.title":
    "低运行开销，Agent 友好<br>基于 sing-box 的跨平台代理工具",
  "hero.installer": "下载",
  "hero.installerPlatform": "下载 {platform} 版本",
  "hero.more": "更多安装包下载",
  "download.kicker": "安装包",
  "download.title": "直接下载安装包。",
  "download.action": "下载",
  "download.yours": "这台设备",
  "download.macos.name": "macOS",
  "download.macos.kind": "Apple Silicon",
  "download.macos.note":
    "首次打开若提示无法验证开发者，到系统设置 → 隐私与安全性里允许打开。",
  "download.windows.name": "Windows",
  "download.windows.kind": "64 位",
  "download.windows.note":
    "安装到当前用户，不需要管理员权限。SmartScreen 可能会要求确认。",
  "download.android.name": "Android",
  "download.android.kind": "arm64",
  "download.android.note":
    "没有应用商店。系统询问时，允许安装来自此来源的应用。",
  "demo.windowBar": "实时产品演示",
  "demo.iframeTitle": "ice-box 真实桌面端界面演示",
  "nav.languageToggle": "EN",
  "nav.languageToggleAria": "切换到英文",
} as const;

export type MarketingKey = keyof typeof zh;

const en: Record<MarketingKey, string> = {
  "head.title": "ice-box — proxy, kept simple",
  "head.description":
    "ice-box — a lightweight proxy client for macOS, Windows, and Android. Easy to set up, quick to start, and light on memory and CPU once it runs.",
  "hero.title": "Low overhead, agent-friendly<br>multi-platform proxy built on sing-box",
  "hero.installer": "Download",
  "hero.installerPlatform": "Download {platform}",
  "hero.more": "More installers",
  "download.kicker": "Installers",
  "download.title": "Download an installer.",
  "download.action": "Download",
  "download.yours": "This device",
  "download.macos.name": "macOS",
  "download.macos.kind": "Apple Silicon",
  "download.macos.note":
    "If the first launch says the developer cannot be verified, allow it in System Settings → Privacy & Security.",
  "download.windows.name": "Windows",
  "download.windows.kind": "64-bit",
  "download.windows.note":
    "Installs for the current user, with no administrator rights. SmartScreen may ask you to confirm.",
  "download.android.name": "Android",
  "download.android.kind": "arm64",
  "download.android.note":
    "There is no app store listing. Allow installs from this source when Android asks.",
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
