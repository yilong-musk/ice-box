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
  "head.title": "ice-box — 最好的 sing-box GUI",
  "head.description":
    "ice-box — 面向 macOS、Windows 与 Android 的轻量代理客户端。上手简单、启动迅速，运行时占用小，支持 Agent 通过 MCP 接入。",
  "nav.languageToggle": "EN",
  "nav.languageToggleAria": "切换到英文",
  "nav.themeToLight": "切换到浅色模式",
  "nav.themeToDark": "切换到深色模式",
  "hero.title": "最好的 <span class=\"accent\">sing-box GUI</span>",
  "hero.subtitle": "面向 macOS、Windows 和 Android 的轻量代理客户端。导入订阅、选好节点，一键开启；运行时安静省资源，还能让 Agent 通过 MCP 与你协同操作。",
  "hero.installer": "免费下载",
  "hero.installerPlatform": "下载 {platform} 版",
  "hero.tryDemo": "在线体验",
  "hero.platforms": "macOS · Windows · Android",
  "demo.windowBar": "实时产品演示",
  "demo.iframeTitle": "ice-box 桌面端实时演示",
  "features.kicker": "功能",
  "features.title": "日常所需，触手可及",
  "features.body": "从导入订阅到排查连接，常用操作都在一两次点击之内。",
  "features.subs.title": "订阅",
  "features.subs.body": "自动识别 sing-box JSON、Clash 与分享链接，也可导入本地配置。桌面端支持自动更新，剩余流量与到期时间一目了然。",
  "features.routing.title": "分流",
  "features.routing.body": "规则、全局、直连三种模式随时切换；订阅中的每条规则都可单独开关，还能叠加自定义规则。",
  "features.nodes.title": "节点",
  "features.nodes.body": "按策略组选择出口，单个或批量测速，常用节点可收藏置顶。",
  "features.connections.title": "连接",
  "features.connections.body": "在 macOS 和 Windows 上查看每条活动连接的目标、命中规则、出口与流量，支持搜索、排序和一键关闭。",
  "features.capture.title": "接管方式",
  "features.capture.body": "桌面端可选系统代理或 TUN；Android 使用系统 VPN，并可按应用设置跟随规则、代理、直连或拦截。",
  "features.tray.title": "托盘",
  "features.tray.body": "无需打开窗口即可启停代理，切换模式、订阅与出口节点；macOS 菜单栏还能显示实时网速。",
  "features.upkeep.title": "省心",
  "features.upkeep.body": "开机自启、签名校验的应用内更新，首页实时显示流量与内存，支持中英文与深浅色主题。",
  "agent.kicker": "Agent 友好",
  "agent.title": "让 Agent 和你操作同一个会话",
  "agent.body": "桌面端内置可选的本地 MCP 服务。开启后把配置交给 Agent，它就能读取状态、切换模式和节点、管理订阅与规则、查看连接，操作的正是你在界面上看到的同一份数据。",
  "agent.point1": "不额外启动后台进程，也不复制数据目录",
  "agent.point2": "你和 Agent 的操作彼此实时可见",
  "agent.point3": "默认关闭，仅监听本机",
  "agent.terminalTitle": "Agent 会话",
  "agent.prompt": "切到全局模式，然后看看哪些连接流量最大",
  "agent.reply": "已切换到全局模式。8 条连接中流量最大的是 googlevideo.com（YouTube），经 Tokyo / edge-01 出口。",
  "phoneDemo.iframeTitle": "ice-box Android 实时演示",
  "download.kicker": "下载",
  "download.title": "选择你的平台",
  "download.body": "安装包直接托管在 GitHub Releases，无需注册账号。",
  "download.action": "下载",
  "download.yours": "这台设备",
  "download.allReleases": "查看全部版本与更新日志",
  "download.macos.name": "macOS",
  "download.macos.kind": "Apple Silicon",
  "download.macos.requires": "macOS 12 或更高",
  "download.macos.note": "首次打开如提示“无法验证开发者”，请前往 系统设置 → 隐私与安全性 允许打开。",
  "download.windows.name": "Windows",
  "download.windows.kind": "64 位",
  "download.windows.requires": "Windows 10 或更高",
  "download.windows.note": "安装到当前用户，无需管理员权限；SmartScreen 可能会请求确认。",
  "download.android.name": "Android",
  "download.android.kind": "arm64",
  "download.android.requires": "Android 14 或更高",
  "download.android.note": "暂未上架应用商店，安装时请按系统提示允许来自此来源的应用。",
  "footer.source": "源代码",
  "footer.releases": "版本发布",
  "footer.changelog": "更新日志",
  "footer.license": "GPL-3.0-or-later 授权",
} as const;

export type MarketingKey = keyof typeof zh;

const en: Record<MarketingKey, string> = {
  "head.title": "ice-box — the best sing-box GUI",
  "head.description": "ice-box is a lightweight proxy client for macOS, Windows, and Android: easy to set up, quick to start, light on resources, and ready for Agents over MCP.",
  "nav.languageToggle": "中文",
  "nav.languageToggleAria": "Switch to Chinese",
  "nav.themeToLight": "Switch to light mode",
  "nav.themeToDark": "Switch to dark mode",
  "hero.title": "The best <span class=\"accent\">sing-box GUI</span>",
  "hero.subtitle": "A lightweight proxy client for macOS, Windows, and Android. Import a subscription, pick a node, and flip the switch. It stays quiet while it runs, and your Agent can work alongside you over MCP.",
  "hero.installer": "Free download",
  "hero.installerPlatform": "Download for {platform}",
  "hero.tryDemo": "Try the live demo",
  "hero.platforms": "macOS · Windows · Android",
  "demo.windowBar": "live product demo",
  "demo.iframeTitle": "Live demo of the ice-box desktop app",
  "features.kicker": "Features",
  "features.title": "Everything you need, right at hand",
  "features.body": "From importing a subscription to tracing a connection, everyday tasks are only a click or two away.",
  "features.subs.title": "Subscriptions",
  "features.subs.body": "Detects sing-box JSON, Clash, and share links automatically, and imports local configs too. On desktop, subscriptions update on their own, with remaining traffic and expiry at a glance.",
  "features.routing.title": "Routing",
  "features.routing.body": "Switch between Rule, Global, and Direct whenever you like. Toggle each subscription rule on its own and layer your own rules on top.",
  "features.nodes.title": "Nodes",
  "features.nodes.body": "Choose an exit for each strategy group, test latency for a single node or a whole list, and pin your favorites.",
  "features.connections.title": "Connections",
  "features.connections.body": "On macOS and Windows, see every live connection: its destination, matched rule, exit, and traffic. Search, sort, or close them in one click.",
  "features.capture.title": "Capture",
  "features.capture.body": "Use the system proxy or TUN on desktop. On Android, ice-box runs as a system VPN with per-app routing: follow rules, proxy, direct, or block.",
  "features.tray.title": "Tray",
  "features.tray.body": "Start or stop the proxy and switch modes, subscriptions, or exits without opening the window. On macOS, the menu bar shows live traffic too.",
  "features.upkeep.title": "Quiet upkeep",
  "features.upkeep.body": "Launch at login, signature-verified in-app updates, live traffic and memory on Home, plus English and 简体中文 in light and dark themes.",
  "agent.kicker": "Agent-friendly",
  "agent.title": "Your Agent works in the same session as you",
  "agent.body": "The desktop app includes an optional local MCP service. Turn it on and hand the config to your Agent: it can read status, switch modes and nodes, manage subscriptions and rules, and inspect connections, all against the same data you see in the window.",
  "agent.point1": "No extra background process, no duplicated data directory",
  "agent.point2": "You and your Agent see each other's changes instantly",
  "agent.point3": "Off by default, and listens on this machine only",
  "agent.terminalTitle": "Agent session",
  "agent.prompt": "Switch to Global mode, then show me the busiest connections",
  "agent.reply": "Switched to Global. Of 8 connections, googlevideo.com (YouTube) is the busiest, routed through Tokyo / edge-01.",
  "phoneDemo.iframeTitle": "Live demo of the ice-box Android app",
  "download.kicker": "Download",
  "download.title": "Choose your platform",
  "download.body": "Installers are hosted on GitHub Releases. No sign-up required.",
  "download.action": "Download",
  "download.yours": "This device",
  "download.allReleases": "See all releases and the changelog",
  "download.macos.name": "macOS",
  "download.macos.kind": "Apple Silicon",
  "download.macos.requires": "macOS 12 or later",
  "download.macos.note": "If macOS says the developer cannot be verified on first launch, allow the app in System Settings → Privacy & Security.",
  "download.windows.name": "Windows",
  "download.windows.kind": "64-bit",
  "download.windows.requires": "Windows 10 or later",
  "download.windows.note": "Installs for the current user without administrator rights. SmartScreen may ask you to confirm.",
  "download.android.name": "Android",
  "download.android.kind": "arm64",
  "download.android.requires": "Android 14 or later",
  "download.android.note": "Not on any app store yet. When Android asks, allow installs from this source.",
  "footer.source": "Source code",
  "footer.releases": "Releases",
  "footer.changelog": "Changelog",
  "footer.license": "Licensed under GPL-3.0-or-later",
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
