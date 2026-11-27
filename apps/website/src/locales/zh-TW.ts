// SPDX-License-Identifier: GPL-3.0-or-later

import type { MarketingKey } from "./zh";

export const zhTw: Record<MarketingKey, string> = {
  "head.title": "ice-box — 最好的 sing-box GUI",
  "head.description":
    "ice-box — 面向 macOS、Windows、Linux 與 Android 的輕量代理用戶端。上手簡單、啟動迅速，執行時佔用小，支援 Agent 透過 MCP 接入。",
  "nav.languageAria": "選擇語言",
  "nav.themeToLight": "切換至淺色模式",
  "nav.themeToDark": "切換至深色模式",
  "nav.features": "功能",
  "nav.agent": "Agent",
  "nav.download": "下載",
  "hero.release": "{version} 已發布",
  "hero.title": "最好的 <span class=\"accent\">sing-box GUI</span>",
  "hero.subtitle":
    "面向 macOS、Windows、Linux 與 Android 的輕量代理用戶端。匯入訂閱、選好節點，一鍵開啟；執行時安靜省資源，還能讓 Agent 透過 MCP 與你協同操作。",
  "hero.installer": "免費下載",
  "hero.installerPlatform": "下載 {platform} 版",
  "hero.tryDemo": "線上體驗",
  "hero.platforms": "macOS · Windows · Linux · Android",
  "demo.windowBar": "即時產品演示",
  "demo.iframeTitle": "ice-box 桌面端即時演示",
  "features.kicker": "功能",
  "features.title": "日常所需，觸手可及",
  "features.body": "從匯入訂閱到排查連線，常用操作都在一兩次點擊之內。",
  "features.groupDaily": "日常使用",
  "features.groupBackground": "背景執行",
  "features.subs.title": "訂閱",
  "features.subs.body":
    "自動辨識 sing-box JSON、Clash 與分享連結，也可匯入本機設定。桌面端支援自動更新，剩餘流量與到期時間一目了然。",
  "features.routing.title": "分流",
  "features.routing.body":
    "規則、全域、直連三種模式隨時切換；訂閱中的每條規則皆可單獨開關，還能疊加自訂規則。",
  "features.nodes.title": "節點",
  "features.nodes.body": "依策略組選擇出口，單一或批次測速，常用節點可收藏置頂。",
  "features.connections.title": "連線",
  "features.connections.body":
    "在桌面端（macOS、Windows、Linux）檢視每條活動連線的目標、命中規則、出口與流量，支援搜尋、排序與一鍵關閉。",
  "features.capture.title": "接管方式",
  "features.capture.body":
    "桌面端可選系統代理或 TUN；Android 使用系統 VPN，並可依應用程式設定跟隨規則、代理、直連或封鎖。",
  "features.tray.title": "系統匣",
  "features.tray.body":
    "無需開啟視窗即可啟停代理，切換模式、訂閱與出口節點；macOS 選單列還能顯示即時網速。",
  "features.upkeep.title": "省心",
  "features.upkeep.body":
    "登入時自動啟動、簽章驗證的應用程式內更新，首頁即時顯示流量與記憶體，支援多語言（含 RTL）與深淺色主題。",
  "agent.kicker": "Agent 友好",
  "agent.title": "讓 Agent 和你操作同一個工作階段",
  "agent.body":
    "桌面端內建可選的本機 MCP 服務。開啟後將設定交給 Agent，它就能讀取狀態、切換模式與節點、管理訂閱與規則、檢視連線，操作的正是你在介面上看到的同一份資料。",
  "agent.point1": "不額外啟動背景程序，也不複製資料目錄",
  "agent.point2": "你與 Agent 的操作彼此即時可見",
  "agent.point3": "預設關閉，僅監聽本機",
  "agent.terminalTitle": "Agent 工作階段",
  "agent.prompt": "切到全域模式，然後看看哪些連線流量最大",
  "agent.reply":
    "已切換至全域模式。8 條連線中流量最大的是 googlevideo.com（YouTube），經由 Los Angeles / edge-03 出口。",
  "phoneDemo.iframeTitle": "ice-box Android 即時演示",
  "download.kicker": "下載",
  "download.title": "選擇你的平台",
  "download.body": "安裝套件直接託管於 GitHub Releases，無需註冊帳號。",
  "download.tabListAria": "選擇作業系統",
  "download.actionPlatform": "下載 {platform} 版",
  "download.yours": "這台裝置",
  "download.allReleases": "檢視全部版本與更新日誌",
  "download.macos.name": "macOS",
  "download.macos.kind": "Apple Silicon",
  "download.macos.requires": "macOS 12 或更高",
  "download.macos.note":
    "首次開啟如提示「無法驗證開發者」，請前往 系統設定 → 隱私權與安全性 允許開啟。",
  "download.windows.name": "Windows",
  "download.windows.kind": "64 位元",
  "download.windows.requires": "Windows 10 或更高",
  "download.windows.note":
    "安裝至目前使用者，無需管理員權限；SmartScreen 可能會請求確認。",
  "download.linux.name": "Linux",
  "download.linux.requires": "Ubuntu 22.04+ 等主流發行版",
  "download.linux.note":
    "AppImage 支援簽章應用程式內更新，.deb 透過系統套件管理員更新。系統代理支援 GNOME/KDE，TUN 需 systemd 與 Polkit。",
  "download.linux.archAria": "選擇 Linux 架構",
  "download.linux.appimageAction": "下載 AppImage",
  "download.linux.debAction": "下載 .deb",
  "download.android.name": "Android",
  "download.android.kind": "arm64",
  "download.android.requires": "Android 14 或更高",
  "download.android.note":
    "尚未上架應用程式商店，安裝時請依系統提示允許來自此來源的應用程式。",
  "footer.source": "原始碼",
  "footer.releases": "版本發布",
  "footer.changelog": "更新日誌",
  "footer.license": "GPL-3.0-or-later 授權",
};
