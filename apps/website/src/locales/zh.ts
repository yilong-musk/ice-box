// SPDX-License-Identifier: GPL-3.0-or-later

/** The zh dictionary is the source of truth for marketing message keys. */
export const zh = {
  "head.title": "ice-box — 最好的 sing-box GUI",
  "head.description":
    "ice-box — 面向 macOS、Windows、Linux 与 Android 的轻量代理客户端。上手简单、启动迅速，运行时占用小，支持 Agent 通过 MCP 接入。",
  "nav.languageAria": "选择语言",
  "nav.themeToLight": "切换到浅色模式",
  "nav.themeToDark": "切换到深色模式",
  "nav.features": "功能",
  "nav.agent": "Agent",
  "nav.download": "下载",
  "hero.release": "{version} 已发布",
  "hero.title": "最好的 <span class=\"accent\">sing-box GUI</span>",
  "hero.subtitle":
    "面向 macOS、Windows、Linux 和 Android 的轻量代理客户端。导入订阅、选好节点，一键开启；运行时安静省资源，还能让 Agent 通过 MCP 与你协同操作。",
  "hero.installer": "免费下载",
  "hero.installerPlatform": "下载 {platform} 版",
  "hero.tryDemo": "在线体验",
  "hero.platforms": "macOS · Windows · Linux · Android",
  "demo.windowBar": "实时产品演示",
  "demo.iframeTitle": "ice-box 桌面端实时演示",
  "features.kicker": "功能",
  "features.title": "日常所需，触手可及",
  "features.body": "从导入订阅到排查连接，常用操作都在一两次点击之内。",
  "features.groupDaily": "日常使用",
  "features.groupBackground": "后台运行",
  "features.subs.title": "订阅",
  "features.subs.body":
    "自动识别 sing-box JSON、Clash 与分享链接，也可导入本地配置。桌面端支持自动更新，剩余流量与到期时间一目了然。",
  "features.routing.title": "分流",
  "features.routing.body":
    "规则、全局、直连三种模式随时切换；订阅中的每条规则都可单独开关，还能叠加自定义规则。",
  "features.nodes.title": "节点",
  "features.nodes.body": "按策略组选择出口，单个或批量测速，常用节点可收藏置顶。",
  "features.connections.title": "连接",
  "features.connections.body":
    "在桌面端（macOS、Windows、Linux）查看每条活动连接的目标、命中规则、出口与流量，支持搜索、排序和一键关闭。",
  "features.capture.title": "接管方式",
  "features.capture.body":
    "桌面端可选系统代理或 TUN；Android 使用系统 VPN，并可按应用设置跟随规则、代理、直连或拦截。",
  "features.tray.title": "托盘",
  "features.tray.body":
    "无需打开窗口即可启停代理，切换模式、订阅与出口节点；macOS 菜单栏还能显示实时网速。",
  "features.upkeep.title": "省心",
  "features.upkeep.body":
    "开机自启、签名校验的应用内更新，首页实时显示流量与内存，支持多语言（含 RTL）与深浅色主题。",
  "agent.kicker": "Agent 友好",
  "agent.title": "让 Agent 和你操作同一个会话",
  "agent.body":
    "桌面端内置可选的本地 MCP 服务。开启后把配置交给 Agent，它就能读取状态、切换模式和节点、管理订阅与规则、查看连接，操作的正是你在界面上看到的同一份数据。",
  "agent.point1": "不额外启动后台进程，也不复制数据目录",
  "agent.point2": "你和 Agent 的操作彼此实时可见",
  "agent.point3": "默认关闭，仅监听本机",
  "agent.terminalTitle": "Agent 会话",
  "agent.prompt": "切到全局模式，然后看看哪些连接流量最大",
  "agent.reply":
    "已切换到全局模式。8 条连接中流量最大的是 googlevideo.com（YouTube），经 Los Angeles / edge-03 出口。",
  "phoneDemo.iframeTitle": "ice-box Android 实时演示",
  "download.kicker": "下载",
  "download.title": "选择你的平台",
  "download.body": "安装包直接托管在 GitHub Releases，无需注册账号。",
  "download.tabListAria": "选择操作系统",
  "download.actionPlatform": "下载 {platform} 版",
  "download.yours": "这台设备",
  "download.allReleases": "查看全部版本与更新日志",
  "download.macos.name": "macOS",
  "download.macos.kind": "Apple Silicon",
  "download.macos.requires": "macOS 12 或更高",
  "download.macos.note":
    "首次打开如提示“无法验证开发者”，请前往 系统设置 → 隐私与安全性 允许打开。",
  "download.windows.name": "Windows",
  "download.windows.kind": "64 位",
  "download.windows.requires": "Windows 10 或更高",
  "download.windows.note":
    "安装到当前用户，无需管理员权限；SmartScreen 可能会请求确认。",
  "download.linux.name": "Linux",
  "download.linux.requires": "Ubuntu 22.04+ 等主流发行版",
  "download.linux.note":
    "AppImage 支持签名应用内更新，.deb 通过系统包管理器更新。系统代理支持 GNOME/KDE，TUN 需 systemd 与 Polkit。",
  "download.linux.archAria": "选择 Linux 架构",
  "download.linux.appimageAction": "下载 AppImage",
  "download.linux.debAction": "下载 .deb",
  "download.android.name": "Android",
  "download.android.kind": "arm64",
  "download.android.requires": "Android 14 或更高",
  "download.android.note":
    "暂未上架应用商店，安装时请按系统提示允许来自此来源的应用。",
  "footer.source": "源代码",
  "footer.releases": "版本发布",
  "footer.changelog": "更新日志",
  "footer.license": "GPL-3.0-or-later 授权",
} as const;

export type MarketingKey = keyof typeof zh;
