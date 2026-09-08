// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * UI internationalization (zh / en).
 *
 * `t()` is module-global: it reads the currently active language, so plain
 * (non-component) helpers can call it directly. Components re-render on
 * language change through `useLanguagePreference()`. The preference follows
 * the theme module's pattern: localStorage cache for an instant boot apply,
 * plus an app-level custom event for cross-component sync. The Settings page
 * additionally persists `language` in `settings.json` (authoritative) and
 * re-applies it whenever settings are (re)loaded.
 */

import { useEffect, useState } from "react";

export type LanguagePreference = "system" | "zh" | "en";
export type ResolvedLanguage = "zh" | "en";

export const LANGUAGE_STORAGE_KEY = "ice-box.language";
export const LANGUAGE_CHANGE_EVENT = "ice-box-language";

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

/** The zh dictionary is the source of truth for message keys. */
const zh = {
  // --- common ---
  "common.cancel": "取消",
  "common.confirm": "确认",
  "common.delete": "删除",
  "common.saved": "已保存",
  "common.updating": "更新中",
  "common.update": "更新",
  "common.refresh": "刷新",
  "common.activate": "激活",
  "common.enable": "启用",
  "common.disable": "禁用",
  "common.all": "全部",
  "common.empty": "（空）",
  "common.withIface": "（{iface}）",
  "common.withIfaceLabel": "接口 {iface}",
  "common.dash": "—",

  // --- app shell ---
  "app.nav.home": "主页",
  "app.nav.nodes": "节点",
  "app.nav.rules": "规则",
  "app.nav.subs": "订阅",
  "app.nav.logs": "日志",
  "app.nav.settings": "设置",
  "app.nav.aria": "主导航",
  "app.versionAria": "版本 {version}",
  "app.updateAvailableAria": "有新版本 {version}，前往应用更新",

  // --- home page ---
  "home.mode.rule": "规则",
  "home.mode.global": "全局",
  "home.mode.direct": "直连",
  "home.outboundDirect": "直连",
  "home.empty.runningTitle": "仅直连模式运行中",
  "home.empty.runningDesc":
    "当前没有订阅节点，所有流量直接连接。导入订阅后会自动切换到节点分流。需要时用上方大按钮接管流量（系统代理或 TUN）。",
  "home.empty.idleTitle": "还没有可用节点",
  "home.empty.idleDesc":
    "未导入任何订阅。打开软件会自动启动内核（仅直连）；用上方大按钮接管流量（系统代理或 TUN），或先导入订阅。",
  "home.capture.tun": "TUN{iface}",
  "home.capture.systemProxy": "系统代理",
  "home.capture.none": "未接管",
  "home.info.core": "内核",
  "home.info.capture": "捕获",
  "home.info.outbound": "当前出站",
  "home.info.inbound": "入站",
  "home.info.message": "消息",
  "home.outboundTyped": "{tag}（{type}）",
  "home.outboundGroupNow": "{tag} → {now}",
  "home.coreStatus.running": "运行中",
  "home.coreStatus.stopped": "已停止",
  "home.coreStatus.starting": "启动中",
  "home.coreStatus.stopping": "停止中",
  "home.coreStatus.error": "出错",
  "home.power.start": "启动代理服务",
  "home.power.stop": "停止代理服务",
  "home.power.busy": "处理中…",
  "home.power.tunActive": "TUN 已接管{iface}",
  "home.power.proxyLive": "系统代理已接管",
  "home.power.recorded": "已记录，可恢复系统代理",
  "home.power.tunReady": "将启用 TUN 模式接管流量",
  "home.power.clickToCapture": "点击接管系统代理",
  "home.warn.proxyOutOfSync": "系统代理未接管或已不同步",
  "home.warn.permissionRequired":
    "启用 TUN 需要系统权限，未修改任何系统配置。点击「安装辅助组件」将弹出系统授权密码框；安装后自动重试，或停用 TUN 改用系统代理。",
  "home.warn.permissionRequiredNoHelper":
    "启用 TUN 需要一次性管理员授权（创建计划任务），未修改任何系统配置。点击「完成一次性权限设置」将弹出系统 UAC；授权后自动重试，或停用 TUN 改用系统代理。",
  "home.installHelper": "安装辅助组件",
  "home.ensureTunElevation": "完成一次性权限设置",
  "home.fallbackToSystemProxy": "停用 TUN，改用系统代理",
  "home.warn.recoveryRequired":
    "TUN 清理未确认，已阻止新的 TUN 激活。清理不确定时不会启用系统代理回退；请先重试恢复。",
  "home.retryRecovery": "重试恢复",
  "home.proxyStatus": "代理状态",
  "home.tunUnavailable": "TUN 暂不可用",
  "home.unsupported": "当前平台不支持系统代理或 TUN 接管",
  "home.modeAria": "模式",
  "home.tunMode": "TUN 模式",
  "home.infoTitle": "信息",
  "home.trafficTitle": "流量",
  "home.trafficDesc": "最近 60 秒上下行",
  "home.goToSubs": "前往订阅页导入",
  "home.autoSwitchHint": "导入订阅后会自动切换到节点分流。",

  // --- nodes page ---
  "nodes.groupType": "策略组 · {type}",
  "nodes.membersAria": "{group} 成员",
  "nodes.memberCurrentAria": "{member}（当前出口）",
  "nodes.setMemberAria": "将 {member} 设为 {group} 出口",
  "nodes.currentExit": "当前出口",
  "nodes.setExit": "设为出口",
  "nodes.setExitAfterStart": "设为出口（保存后启动生效）",
  "nodes.inUse": "选用中",
  "nodes.collapseMembers": "收起成员",
  "nodes.expandMembers": "展开成员",
  "nodes.notRunning": "代理服务未运行",
  "nodes.testAllMembers": "测全部成员延迟",
  "nodes.testCurrent": "测当前出口延迟",
  "nodes.test": "测速",
  "nodes.select": "选用",
  "nodes.notRunningWarn":
    "代理服务未运行：测延迟不可用；切换出口会保存，启动后生效。",
  "nodes.batchTest": "批量测延迟",
  "nodes.loading": "加载节点列表…",
  "nodes.emptyTitle": "暂无节点",
  "nodes.emptyDesc":
    "未导入任何订阅节点。导入订阅后即可在此查看节点、测速并切换出口。",
  "nodes.listAria": "节点列表",
  "nodes.noTestableGroup": "当前策略组没有可测的出口",
  "nodes.noTestable": "当前没有可测的出口",

  // --- subscriptions page ---
  "subs.partialUpdateFailed": "部分订阅更新失败：{details}",
  "subs.url": "导入订阅",
  "subs.urlPlaceholder": "订阅 URL（https 优先）",
  "subs.name": "名称",
  "subs.namePlaceholder": "名称（可选）",
  "subs.importAction": "导入",
  "subs.autoUpdate": "自动更新",
  "subs.interval": "更新间隔",
  "subs.interval.one_hour": "1 小时",
  "subs.interval.three_hours": "3 小时",
  "subs.interval.six_hours": "6 小时",
  "subs.interval.twelve_hours": "12 小时",
  "subs.interval.twenty_four_hours": "24 小时",
  "subs.httpWarn": "当前为 http://，传输未加密，建议改用 https。",
  "subs.title": "订阅",
  "subs.emptyHint": "尚未导入订阅",
  "subs.count": "{n} 条",
  "subs.updateAll": "全部更新",
  "subs.emptyTitle": "暂无订阅",
  "subs.emptyDesc":
    "打开软件会自动启动内核；需要时在主页用大按钮接管系统代理，也可导入订阅 URL。",
  "subs.listAria": "订阅列表",
  "subs.activeBadge": "已激活",
  "subs.summaryNodes": "{n} 节点",
  "subs.summaryGroups": "{n} 策略组",
  "subs.summaryRules": "{n} 规则",
  "subs.hasDns": "DNS",
  "subs.deleteTitle": "删除订阅",
  "subs.deleteConfirm": "确认删除订阅「{name}」？",
  "subs.unknownError": "未知错误",

  // --- rules page ---
  "rules.savedButApplyFailed": "已保存，但应用失败：{detail}",
  "rules.addCustom": "+ 自定义规则",
  "rules.searchPlaceholder": "搜索域名 / 出口 / 规则集…",
  "rules.searchAria": "搜索规则",
  "rules.filtersAria": "规则筛选",
  "rules.typeAll": "全部",
  "rules.customCount": "自定义 {count}",
  "rules.disabledCount": "已禁用 {count}",
  "rules.emptyTitle": "暂无规则",
  "rules.emptyDesc":
    "当前没有订阅规则，也没有自定义规则。导入含规则的订阅后可在此查询、禁用规则或添加自定义规则。",
  "rules.listAria": "规则列表",
  "rules.custom": "自定义",
  "rules.delete": "删除",
  "rules.prevPage": "上一页",
  "rules.pageInfo": "第 {page} / {pages} 页 · 共 {total} 条",
  "rules.nextPage": "下一页",
  "rules.deleteCustomTitle": "删除自定义规则",
  "rules.deleteCustomConfirm": "确认删除规则：{summary}？",

  // --- logs page ---
  "logs.empty": "（空）",
  "logs.clear": "清空日志",

  // --- settings page ---
  "settings.appearance": "外观",
  "settings.appearanceDesc": "默认跟随系统深浅色。",
  "settings.appearance.system": "跟随系统",
  "settings.appearance.light": "浅色",
  "settings.appearance.dark": "深色",
  "settings.language": "语言",
  "settings.languageDesc": "默认跟随系统语言。",
  "settings.language.system": "跟随系统",
  "settings.language.zh": "简体中文",
  "settings.language.en": "English",
  "settings.tun": "TUN 模式",
  "settings.tunDesc":
    "决定下次启动代理服务时走透明代理还是系统代理；切换本开关不会立即启停当前服务",
  "settings.tunEnable": "启用 TUN 模式",
  "settings.tunTransition.preparing": "正在启用 TUN…",
  "settings.tunTransition.stopping": "正在关闭 TUN…",
  "settings.tunRecoveryRequired":
    "TUN 清理未确认，已阻止新的 TUN 激活；请在主页点击「重试恢复」后再切换",
  "settings.tunNotSupported": "当前平台暂不支持 TUN 模式",
  "settings.tunActiveWithIface":
    "当前通过 TUN 接管流量{interface}。关闭本开关不会停止当前服务，下次启动将改用系统代理",
  "settings.helperStale":
    "应用已更新内核版本，辅助组件仍在运行旧版内核。请点击下方「更新辅助组件」替换（将弹出系统授权密码框），更新前无法启用 TUN",
  "settings.helperReady":
    "辅助组件已安装并授权，可直接启用 TUN",
  "settings.helperNeeded":
    "需要系统权限：先安装并授权辅助组件（将弹出系统授权密码框）。本开关只决定下次启动走 TUN 还是系统代理",
  "settings.tunElevationDesc":
    "首次启用会弹出一次系统 UAC，用于创建计划任务 ice-box-tun；之后开关与启停都不再提权。本开关只决定下次启动走 TUN 还是系统代理",
  "settings.updateHelper": "更新辅助组件",
  "settings.installHelper": "安装辅助组件",
  "settings.uninstallHelper": "卸载辅助组件",
  "settings.inbound": "入站",
  "settings.inboundDesc": "Mixed 与 Clash API 监听地址",
  "settings.mixedListen": "Mixed 监听",
  "settings.mixedPort": "Mixed 端口",
  "settings.clashListen": "Clash API 监听",
  "settings.clashPort": "Clash API 端口",
  "settings.allowLan": "允许局域网共享（Allow LAN）",
  "settings.allowLanDesc":
    "局域网共享时 Mixed 入站监听 0.0.0.0，其他设备可通过本机局域网 IP 连接；Clash API 仍仅限本机",
  "settings.autoDefaultRules": "为无规则的订阅附加默认分流规则",
  "settings.autoDefaultRulesDesc":
    "订阅本身不带规则时（如分享链接订阅），自动附加内置分流：私网 IP / 国内 IP / 国内域名直连，其余走所选节点；并配套国内 / 远程 DNS 分流",
  "settings.coreLogLevel": "详细核心日志（info）",
  "settings.coreLogLevelDesc":
    "默认 warn，减少长期运行占盘；调试时打开 info，变更会在下次应用配置后生效",
  "settings.openDataDir": "打开数据目录",
  "settings.helperStatusUnconfirmed":
    "辅助组件状态未确认，未更改 TUN 设置；请稍后重试",
  "settings.tunNotSaved": "TUN 设置未保存：请先修正上方表单中的错误后重试",
  "settings.update": "应用更新",
  "settings.updateAutoCheck": "自动检查更新",
  "settings.updateCurrent": "当前版本 {version}",
  "settings.updateCheck": "检查更新",
  "settings.updateUpToDate": "已是最新版本",
  "settings.updateAvailable": "发现新版本 {version}",
  "settings.updateInstall": "安装更新",
  "settings.updateCheckFailed":
    "无法检查更新。若 GitHub 无法直连，请先启动代理服务后再试。",
  "settings.updateFeedUnavailable":
    "未找到 GitHub 上的更新清单。在带自动更新的版本正式发布之前，检查更新会失败。",

  // --- app update progress ---
  "update.installing": "正在下载更新…",
  "update.progressPct": "正在下载更新… {pct}%",
  "update.progressBytes": "已下载 {bytes} 字节",

  // --- window controls ---
  "window.minimize": "最小化",
  "window.maximize": "最大化",
  "window.close": "关闭",

  // --- TUN install dialog ---
  "tunDialog.title": "启用 TUN 需要先安装辅助组件",
  "tunDialog.desc":
    "辅助组件以系统权限运行 TUN 内核，当前尚未安装或未授权。点击「安装并启用」将弹出系统授权密码框，安装成功后再保存并启用 TUN 设置；取消则不启用。",
  "tunDialog.installAndEnable": "安装并启用",

  // --- custom rule form ---
  "ruleForm.title": "添加自定义规则",
  "ruleForm.desc":
    "自定义规则优先于订阅规则生效，出口需为 direct / block 或现有节点 / 策略组标签。",
  "ruleForm.matcherType": "匹配类型",
  "ruleForm.matchValue": "匹配值",
  "ruleForm.outbound": "出口",
  "ruleForm.directOption": "direct（直连）",
  "ruleForm.blockOption": "block（拦截）",
  "ruleForm.strategyGroupSuffix": "（策略组）",
  "ruleForm.matchBool": "匹配{label}",
  "ruleForm.preview": "预览：",
  "ruleForm.add": "添加",

  // --- traffic chart ---
  "traffic.down": "下行",
  "traffic.up": "上行",
  "traffic.idleHint": "启动代理服务后显示实时上下行曲线（最近 {n} 秒）。",
  "traffic.samplingInterrupted": "采样中断：{error}",
  "traffic.chartAria": "上下行流量曲线",
  "traffic.peak": "峰值刻度 {rate} · 最近 {n} 秒（后台持续采样）",

  // --- validation (field name is prefixed by the caller) ---
  "validation.listen": "必须是 loopback 地址（127.0.0.1、localhost 或 ::1）",
  "validation.port": "必须是 1024–65535 之间的整数",
  "validation.portsConflict": "Mixed 端口与 Clash API 端口不能相同",

  // --- rule type labels ---
  "ruleType.domain": "域名",
  "ruleType.domainSuffix": "域名后缀",
  "ruleType.domainKeyword": "域名关键词",
  "ruleType.domainRegex": "域名正则",
  "ruleType.ipCidr": "IP 段",
  "ruleType.ipIsPrivate": "私网 IP",
  "ruleType.sourceIpCidr": "源 IP 段",
  "ruleType.sourceIpIsPrivate": "源私网 IP",
  "ruleType.ruleSet": "规则集",
  "ruleType.geoip": "GEOIP",
  "ruleType.geosite": "GEOSITE",
  "ruleType.port": "端口",
  "ruleType.sourcePort": "源端口",
  "ruleType.network": "网络",
  "ruleType.protocol": "协议",
  "ruleType.processName": "进程名",
  "ruleType.processPath": "进程路径",
  "ruleType.packageName": "应用包名",
  "ruleType.inbound": "入站",
  "ruleType.wifiSsid": "WiFi SSID",
  "ruleType.wifiBssid": "WiFi BSSID",
  "ruleType.clashMode": "Clash 模式",
  "ruleType.user": "用户",
  "ruleType.other": "其他",

  // --- IPC errors (FE-5) ---
  "error.core.not_found": "找不到内核",
  "error.core.spawn_failed": "内核启动失败",
  "error.core.healthcheck_failed": "内核健康检查失败",
  "error.core.invalid_state": "内核状态无效",
  "error.core.adopt_rejected": "无法接管已有内核进程",
  "error.core.api_failed": "内核接口请求失败",
  "error.config.empty_outbounds": "没有可用出站",
  "error.config.invalid": "配置无效",
  "error.proxy.apply_failed": "系统代理设置失败",
  "error.proxy.apply_failed_core_reloaded": "系统代理设置失败（内核已重载）",
  "error.proxy.restore_failed": "系统代理恢复失败",
  "error.proxy.backup_corrupt": "系统代理备份已损坏",
  "error.settings.reset": "设置文件无效，已恢复为默认值",
  "error.logs.oversized": "日志文件超过 20 MiB，请在日志页清空",
  "error.sub.fetch_failed": "订阅下载失败",
  "error.sub.unknown_format": "无法识别的订阅格式",
  "error.sub.parse_failed": "订阅解析失败",
  "error.sub.empty": "订阅为空",
  "error.sub.not_found": "找不到该订阅",
  "error.sub.io": "订阅读写失败",
  "error.app.lock_poisoned": "内部状态已损坏",
  "error.tun.not_supported": "当前平台不支持 TUN",
  "error.tun.permission_required": "需要系统权限才能启用 TUN",
  "error.tun.apply_failed": "TUN 启用失败",
  "error.tun.restore_failed": "TUN 恢复失败",
  "error.tun.healthcheck_failed": "TUN 健康检查失败",
  "error.tun.recovery_required": "TUN 需要先恢复",
  "error.tun.invalid_argument": "TUN 参数无效",
  "error.tun.config_rejected": "已拒绝不安全的 TUN 配置",
  "error.tun.helper_stale": "辅助组件版本过旧",
  "error.tun.helper_install_failed": "辅助组件安装失败",
  "error.tun.helper_install_cancelled": "已取消安装辅助组件",
  "error.tun.helper_not_ready": "辅助组件未就绪",
  "error.tun.elevation_cancelled": "已取消管理员授权",
  "error.update.check_failed": "检查更新失败",
  "error.update.feed_unavailable": "更新目录暂不可用",
  "error.update.install_failed": "安装更新失败",
  "error.update.disabled": "已关闭应用更新",

  // --- delay ---
  "delay.failed": "失败",

  "ui.raw": "{text}",
  "tun.unsupportedPlatform": "当前版本仅在 macOS 和 Windows 上支持 TUN",
  "parse.truncated": "已截断 {kind}：丢弃 {dropped} 项",
  "parse.groupUnknownMember": "策略组 {name}：未知成员 {member}",
  "parse.groupNoMembers": "策略组 {name}：没有可解析的成员",
  "parse.groupUnsupportedType": "策略组 {name}：不支持的类型 {type}",
  "parse.dnsUnsupportedNameserver": "DNS：不支持的 nameserver {server}",
  "parse.dnsNoUsableNameserver": "DNS：没有可用的 nameserver",
  "parse.ruleUnknownTarget": "规则目标 {target} 无法解析为出站，已跳过",
  "parse.uriLine": "第 {line} 行：{reason}",
  "parse.droppedDetour": "已丢弃出站 {tag}：detour {detour} 不存在",
  "parse.trimmedGroup": "已修剪策略组 {tag}：丢弃 {dropped} 个缺失成员",
  "parse.droppedEmptyGroup": "已丢弃空策略组 {tag}",
  "parse.routeOutboundMissing": "路由规则出站 {outbound} 缺失，回退到 {fallback}",
  "parse.routeFinalMissing": "路由 final {outbound} 缺失，回退到 {fallback}",
  "core.writePid": "写入 PID 失败：{error}",
  "core.stopFailed": "停止失败：{error}",
  "core.exitedUnexpectedly": "内核意外退出（代码 {code}）",
  "parse.skippedOutbound": "已跳过出站 {tag}：{detail}",
  "recover.pendingSettings": "检测到未完成的后端切换，已恢复为上一份设置",
  "recover.controllerUnavailable": "捕获控制器不可用，未能恢复 TUN 状态",
  "recover.tunJournalMissing": "内核意外退出后找不到 TUN 日志，已进入故障关闭",
  "recover.tunCleanupUnconfirmed": "内核意外退出后未能确认 TUN 清理：{detail}",
  "recover.tunCleanupRetry": "未能确认 TUN 清理，已故障关闭。请重试恢复",
  "recover.tunDnsFailed": "TUN DNS 恢复失败：{detail}",
  "recover.orphanCoreUnconfirmed": "残留内核清理未确认：{detail}",
  "recover.tunStateUnconfirmed": "TUN 状态恢复未确认：{detail}",
  "recover.proxyAfterExit": "内核意外退出后系统代理恢复失败：{detail}",
  "recover.tunShutdownUnconfirmed": "未能确认 TUN 捕获关闭：{detail}",
  "recover.autoStartFailed": "自动启动失败：{detail}",
} as const;

export type MessageKey = keyof typeof zh;

export function isMessageKey(key: string): key is MessageKey {
  return Object.prototype.hasOwnProperty.call(zh, key);
}

const en: Record<MessageKey, string> = {
  // --- common ---
  "common.cancel": "Cancel",
  "common.confirm": "Confirm",
  "common.delete": "Delete",
  "common.saved": "Saved",
  "common.updating": "Updating…",
  "common.update": "Update",
  "common.refresh": "Refresh",
  "common.activate": "Activate",
  "common.enable": "Enable",
  "common.disable": "Disable",
  "common.all": "All",
  "common.empty": "(empty)",
  "common.withIface": " ({iface})",
  "common.withIfaceLabel": "interface {iface}",
  "common.dash": "—",

  // --- app shell ---
  "app.nav.home": "Home",
  "app.nav.nodes": "Nodes",
  "app.nav.rules": "Rules",
  "app.nav.subs": "Subscriptions",
  "app.nav.logs": "Logs",
  "app.nav.settings": "Settings",
  "app.nav.aria": "Main navigation",
  "app.versionAria": "Version {version}",
  "app.updateAvailableAria": "Version {version} is available, open App Updates",

  // --- home page ---
  "home.mode.rule": "Rule",
  "home.mode.global": "Global",
  "home.mode.direct": "Direct",
  "home.outboundDirect": "Direct",
  "home.empty.runningTitle": "Running in direct-only mode",
  "home.empty.runningDesc":
    "There are no subscription nodes, so all traffic connects directly. Importing a subscription switches to node-based routing automatically. Use the big button above to capture traffic (system proxy or TUN) when needed.",
  "home.empty.idleTitle": "No nodes yet",
  "home.empty.idleDesc":
    "No subscriptions imported. The app starts the core (direct-only) automatically; capture traffic with the big button above (system proxy or TUN), or import a subscription first.",
  "home.capture.tun": "TUN{iface}",
  "home.capture.systemProxy": "System Proxy",
  "home.capture.none": "Not captured",
  "home.info.core": "Core",
  "home.info.capture": "Capture",
  "home.info.outbound": "Outbound",
  "home.info.inbound": "Inbound",
  "home.info.message": "Message",
  "home.outboundTyped": "{tag} ({type})",
  "home.outboundGroupNow": "{tag} → {now}",
  "home.coreStatus.running": "Running",
  "home.coreStatus.stopped": "Stopped",
  "home.coreStatus.starting": "Starting",
  "home.coreStatus.stopping": "Stopping",
  "home.coreStatus.error": "Error",
  "home.power.start": "Start Proxy Service",
  "home.power.stop": "Stop Proxy Service",
  "home.power.busy": "Working…",
  "home.power.tunActive": "TUN is active{iface}",
  "home.power.proxyLive": "System proxy is active",
  "home.power.recorded": "Recorded; system proxy can be restored",
  "home.power.tunReady": "Will capture traffic via TUN",
  "home.power.clickToCapture": "Click to capture via system proxy",
  "home.warn.proxyOutOfSync": "System proxy is not applied or out of sync",
  "home.warn.permissionRequired":
    "Enabling TUN requires system permission; no system configuration was changed. Clicking “Install Helper” opens the system authorization prompt; installation retries automatically, or disable TUN and use the system proxy.",
  "home.warn.permissionRequiredNoHelper":
    "Enabling TUN needs a one-time administrator approval to create the scheduled task; no system configuration was changed. Click “Complete one-time setup” to open UAC; approval retries automatically, or disable TUN and use the system proxy.",
  "home.installHelper": "Install Helper",
  "home.ensureTunElevation": "Complete one-time setup",
  "home.fallbackToSystemProxy": "Disable TUN, use system proxy",
  "home.warn.recoveryRequired":
    "TUN cleanup is unconfirmed, so new TUN activation is blocked. The system-proxy fallback is not enabled while cleanup is uncertain; retry recovery first.",
  "home.retryRecovery": "Retry Recovery",
  "home.proxyStatus": "Proxy Status",
  "home.tunUnavailable": "TUN unavailable",
  "home.unsupported":
    "System proxy and TUN capture are not supported on this platform",
  "home.modeAria": "Mode",
  "home.tunMode": "TUN Mode",
  "home.infoTitle": "Info",
  "home.trafficTitle": "Traffic",
  "home.trafficDesc": "Up/down over the last 60 seconds",
  "home.goToSubs": "Import subscriptions",
  "home.autoSwitchHint": "Node routing switches automatically after importing.",

  // --- nodes page ---
  "nodes.groupType": "Group · {type}",
  "nodes.membersAria": "{group} members",
  "nodes.memberCurrentAria": "{member} (current exit)",
  "nodes.setMemberAria": "Set {member} as {group} exit",
  "nodes.currentExit": "Current exit",
  "nodes.setExit": "Set as exit",
  "nodes.setExitAfterStart": "Set as exit (applied after start)",
  "nodes.inUse": "In use",
  "nodes.collapseMembers": "Collapse members",
  "nodes.expandMembers": "Expand members",
  "nodes.notRunning": "Proxy service not running",
  "nodes.testAllMembers": "Test delay of all members",
  "nodes.testCurrent": "Test delay of current exit",
  "nodes.test": "Test",
  "nodes.select": "Select",
  "nodes.notRunningWarn":
    "Proxy service not running: delay tests are unavailable; switching exits is saved and applies after start.",
  "nodes.batchTest": "Batch Test",
  "nodes.loading": "Loading node list…",
  "nodes.emptyTitle": "No nodes",
  "nodes.emptyDesc":
    "No subscription nodes imported. Import a subscription to view nodes, test delay, and switch exits here.",
  "nodes.listAria": "Node list",
  "nodes.noTestableGroup": "This group has no testable exit",
  "nodes.noTestable": "No testable exits",

  // --- subscriptions page ---
  "subs.partialUpdateFailed": "Some subscriptions failed to update: {details}",
  "subs.url": "Import Subscription",
  "subs.urlPlaceholder": "Subscription URL (https preferred)",
  "subs.name": "Name",
  "subs.namePlaceholder": "Name (optional)",
  "subs.importAction": "Import",
  "subs.autoUpdate": "Auto update",
  "subs.interval": "Update interval",
  "subs.interval.one_hour": "1 hour",
  "subs.interval.three_hours": "3 hours",
  "subs.interval.six_hours": "6 hours",
  "subs.interval.twelve_hours": "12 hours",
  "subs.interval.twenty_four_hours": "24 hours",
  "subs.httpWarn":
    "This uses http://, which is unencrypted; https is recommended.",
  "subs.title": "Subscriptions",
  "subs.emptyHint": "No subscriptions yet",
  "subs.count": "{n} items",
  "subs.updateAll": "Update All",
  "subs.emptyTitle": "No subscriptions",
  "subs.emptyDesc":
    "The app starts the core automatically; capture the system proxy from Home when needed, or import a subscription URL.",
  "subs.listAria": "Subscription list",
  "subs.activeBadge": "Active",
  "subs.summaryNodes": "{n} nodes",
  "subs.summaryGroups": "{n} groups",
  "subs.summaryRules": "{n} rules",
  "subs.hasDns": "DNS",
  "subs.deleteTitle": "Delete Subscription",
  "subs.deleteConfirm": "Delete subscription “{name}”?",
  "subs.unknownError": "Unknown error",

  // --- rules page ---
  "rules.savedButApplyFailed": "Saved, but applying failed: {detail}",
  "rules.addCustom": "+ Add Custom Rule",
  "rules.searchPlaceholder": "Search domain / outbound / rule set…",
  "rules.searchAria": "Search rules",
  "rules.filtersAria": "Rule filters",
  "rules.typeAll": "All",
  "rules.customCount": "Custom {count}",
  "rules.disabledCount": "Disabled {count}",
  "rules.emptyTitle": "No rules",
  "rules.emptyDesc":
    "No subscription rules and no custom rules. After importing a subscription with rules you can search, disable rules, and add custom rules here.",
  "rules.listAria": "Rule list",
  "rules.custom": "Custom",
  "rules.delete": "Delete",
  "rules.prevPage": "Previous",
  "rules.pageInfo": "Page {page} of {pages} · {total} total",
  "rules.nextPage": "Next",
  "rules.deleteCustomTitle": "Delete Custom Rule",
  "rules.deleteCustomConfirm": "Delete rule: {summary}?",

  // --- logs page ---
  "logs.empty": "(empty)",
  "logs.clear": "Clear logs",

  // --- settings page ---
  "settings.appearance": "Appearance",
  "settings.appearanceDesc": "Follows the system by default.",
  "settings.appearance.system": "System",
  "settings.appearance.light": "Light",
  "settings.appearance.dark": "Dark",
  "settings.language": "Language",
  "settings.languageDesc": "Follows the system language by default.",
  "settings.language.system": "System",
  "settings.language.zh": "简体中文",
  "settings.language.en": "English",
  "settings.tun": "TUN Mode",
  "settings.tunDesc":
    "Chooses transparent proxy vs the system proxy for the next time the proxy service starts. Toggling this switch does not start or stop the current service",
  "settings.tunEnable": "Enable TUN Mode",
  "settings.tunTransition.preparing": "Enabling TUN…",
  "settings.tunTransition.stopping": "Disabling TUN…",
  "settings.tunRecoveryRequired":
    "TUN cleanup is unconfirmed, so new TUN activation is blocked. Run “Retry Recovery” on Home before switching",
  "settings.tunNotSupported": "TUN is not supported on this platform",
  "settings.tunActiveWithIface":
    "Currently capturing via TUN{interface}. Turning this off does not stop the current service; the next start will use the system proxy",
  "settings.helperStale":
    "The app updated its core, but the helper still runs the old core. Click “Update Helper” below to replace it (a system authorization prompt will appear); TUN stays disabled until then",
  "settings.helperReady":
    "The helper is installed and authorized; TUN can be enabled directly",
  "settings.helperNeeded":
    "System permission required: install and authorize the helper first (a system authorization prompt will appear). This switch only chooses TUN vs the system proxy for the next start",
  "settings.tunElevationDesc":
    "The first enable shows one UAC prompt to create the ice-box-tun scheduled task; later toggles and start/stop do not prompt. This switch only chooses TUN vs the system proxy for the next start",
  "settings.updateHelper": "Update Helper",
  "settings.installHelper": "Install Helper",
  "settings.uninstallHelper": "Uninstall Helper",
  "settings.inbound": "Inbound",
  "settings.inboundDesc": "Mixed and Clash API listen addresses",
  "settings.mixedListen": "Mixed Listen",
  "settings.mixedPort": "Mixed Port",
  "settings.clashListen": "Clash API Listen",
  "settings.clashPort": "Clash API Port",
  "settings.allowLan": "Allow LAN Sharing (Allow LAN)",
  "settings.allowLanDesc":
    "With LAN sharing, the Mixed inbound listens on 0.0.0.0 and other devices can connect via this machine's LAN IP; the Clash API stays local-only",
  "settings.autoDefaultRules": "Attach default rules to rule-less subscriptions",
  "settings.autoDefaultRulesDesc":
    "When a subscription carries no rules (e.g. share-link subscriptions), built-in split routing is attached automatically: private IPs / China IPs / China domains go direct and the rest goes through the selected node, with matching China / remote DNS split",
  "settings.coreLogLevel": "Verbose core logs (info)",
  "settings.coreLogLevelDesc":
    "Default is warn to limit disk growth; turn on info for debug sessions. Takes effect after the next config apply",
  "settings.openDataDir": "Open Data Directory",
  "settings.helperStatusUnconfirmed":
    "Helper status unconfirmed; TUN settings unchanged. Try again later",
  "settings.tunNotSaved": "TUN setting not saved: fix the errors above and retry",
  "settings.update": "App Updates",
  "settings.updateAutoCheck": "Check for updates automatically",
  "settings.updateCurrent": "Current version {version}",
  "settings.updateCheck": "Check for Updates",
  "settings.updateUpToDate": "You're up to date",
  "settings.updateAvailable": "Version {version} is available",
  "settings.updateInstall": "Install Update",
  "settings.updateCheckFailed":
    "Could not check for updates. If GitHub is unreachable, start the proxy service and try again.",
  "settings.updateFeedUnavailable":
    "No update catalog was found on GitHub. Checking for updates fails until an updater-capable release is published.",

  // --- app update progress ---
  "update.installing": "Downloading update…",
  "update.progressPct": "Downloading update… {pct}%",
  "update.progressBytes": "Downloaded {bytes} bytes",

  // --- window controls ---
  "window.minimize": "Minimize",
  "window.maximize": "Maximize",
  "window.close": "Close",

  // --- TUN install dialog ---
  "tunDialog.title": "Install the helper before enabling TUN",
  "tunDialog.desc":
    "The helper runs the TUN core with system permission and is not installed or authorized yet. Clicking “Install & Enable” opens the system authorization prompt; the TUN setting is saved and enabled after a successful install. Cancel keeps TUN disabled.",
  "tunDialog.installAndEnable": "Install & Enable",

  // --- custom rule form ---
  "ruleForm.title": "Add Custom Rule",
  "ruleForm.desc":
    "Custom rules take effect before subscription rules. The outbound must be direct / block or an existing node / group tag.",
  "ruleForm.matcherType": "Match Type",
  "ruleForm.matchValue": "Match Value",
  "ruleForm.outbound": "Outbound",
  "ruleForm.directOption": "direct (Direct)",
  "ruleForm.blockOption": "block (Block)",
  "ruleForm.strategyGroupSuffix": " (group)",
  "ruleForm.matchBool": "Match {label}",
  "ruleForm.preview": "Preview: ",
  "ruleForm.add": "Add",

  // --- traffic chart ---
  "traffic.down": "Down",
  "traffic.up": "Up",
  "traffic.idleHint":
    "A live up/down curve appears after the proxy service starts (last {n} seconds).",
  "traffic.samplingInterrupted": "Sampling interrupted: {error}",
  "traffic.chartAria": "Up/down traffic curve",
  "traffic.peak":
    "Peak scale {rate} · last {n} seconds (continuous background sampling)",

  // --- validation ---
  "validation.listen": "must be a loopback address (127.0.0.1, localhost or ::1)",
  "validation.port": "must be an integer between 1024 and 65535",
  "validation.portsConflict": "Mixed and Clash API ports cannot be the same",

  // --- rule type labels ---
  "ruleType.domain": "Domain",
  "ruleType.domainSuffix": "Domain Suffix",
  "ruleType.domainKeyword": "Domain Keyword",
  "ruleType.domainRegex": "Domain Regex",
  "ruleType.ipCidr": "IP CIDR",
  "ruleType.ipIsPrivate": "Private IP",
  "ruleType.sourceIpCidr": "Source IP CIDR",
  "ruleType.sourceIpIsPrivate": "Source Private IP",
  "ruleType.ruleSet": "Rule Set",
  "ruleType.geoip": "GEOIP",
  "ruleType.geosite": "GEOSITE",
  "ruleType.port": "Port",
  "ruleType.sourcePort": "Source Port",
  "ruleType.network": "Network",
  "ruleType.protocol": "Protocol",
  "ruleType.processName": "Process Name",
  "ruleType.processPath": "Process Path",
  "ruleType.packageName": "Package Name",
  "ruleType.inbound": "Inbound",
  "ruleType.wifiSsid": "WiFi SSID",
  "ruleType.wifiBssid": "WiFi BSSID",
  "ruleType.clashMode": "Clash Mode",
  "ruleType.user": "User",
  "ruleType.other": "Other",

  // --- IPC errors (FE-5) ---
  "error.core.not_found": "Core binary not found",
  "error.core.spawn_failed": "Failed to start the core",
  "error.core.healthcheck_failed": "Core health check failed",
  "error.core.invalid_state": "Core is in an invalid state",
  "error.core.adopt_rejected": "Cannot adopt the existing core process",
  "error.core.api_failed": "Core API request failed",
  "error.config.empty_outbounds": "No usable outbounds",
  "error.config.invalid": "Configuration is invalid",
  "error.proxy.apply_failed": "Failed to apply system proxy",
  "error.proxy.apply_failed_core_reloaded":
    "Failed to apply system proxy (core already reloaded)",
  "error.proxy.restore_failed": "Failed to restore system proxy",
  "error.proxy.backup_corrupt": "System proxy backup is corrupt",
  "error.settings.reset": "Settings file was invalid and was reset to defaults",
  "error.logs.oversized": "Log files exceeded 20 MiB; clear them on the Logs page",
  "error.sub.fetch_failed": "Failed to download the subscription",
  "error.sub.unknown_format": "Unrecognized subscription format",
  "error.sub.parse_failed": "Failed to parse the subscription",
  "error.sub.empty": "Subscription is empty",
  "error.sub.not_found": "Subscription not found",
  "error.sub.io": "Subscription I/O failed",
  "error.app.lock_poisoned": "Internal state is corrupted",
  "error.tun.not_supported": "TUN is not supported on this platform",
  "error.tun.permission_required": "System permission is required to enable TUN",
  "error.tun.apply_failed": "Failed to enable TUN",
  "error.tun.restore_failed": "Failed to restore TUN",
  "error.tun.healthcheck_failed": "TUN health check failed",
  "error.tun.recovery_required": "TUN must be recovered first",
  "error.tun.invalid_argument": "TUN argument is invalid",
  "error.tun.config_rejected": "Rejected an unsafe TUN configuration",
  "error.tun.helper_stale": "Helper component is out of date",
  "error.tun.helper_install_failed": "Failed to install the helper",
  "error.tun.helper_install_cancelled": "Helper install was cancelled",
  "error.tun.helper_not_ready": "Helper is not ready",
  "error.tun.elevation_cancelled": "Administrator authorization was cancelled",
  "error.update.check_failed": "Failed to check for updates",
  "error.update.feed_unavailable": "Update catalog is not available yet",
  "error.update.install_failed": "Failed to install the update",
  "error.update.disabled": "App updates are disabled",

  // --- delay ---
  "delay.failed": "Failed",

  "ui.raw": "{text}",
  "tun.unsupportedPlatform": "TUN is supported on macOS and Windows only in this release",
  "parse.truncated": "Truncated {kind}: dropped {dropped}",
  "parse.groupUnknownMember": "Group {name}: unknown member {member}",
  "parse.groupNoMembers": "Group {name}: no resolvable members",
  "parse.groupUnsupportedType": "Group {name}: unsupported type {type}",
  "parse.dnsUnsupportedNameserver": "DNS: unsupported nameserver {server}",
  "parse.dnsNoUsableNameserver": "DNS: no usable nameserver entries",
  "parse.ruleUnknownTarget": "Rule target {target} does not resolve to an outbound; skipped",
  "parse.uriLine": "Line {line}: {reason}",
  "parse.droppedDetour": "Dropped outbound {tag}: detour {detour} is missing",
  "parse.trimmedGroup": "Trimmed group {tag}: dropped {dropped} missing members",
  "parse.droppedEmptyGroup": "Dropped empty group {tag}",
  "parse.routeOutboundMissing": "Route rule outbound {outbound} is missing; falling back to {fallback}",
  "parse.routeFinalMissing": "Route final {outbound} is missing; falling back to {fallback}",
  "core.writePid": "Failed to write pid: {error}",
  "core.stopFailed": "Stop failed: {error}",
  "core.exitedUnexpectedly": "sing-box exited unexpectedly (code {code})",
  "parse.skippedOutbound": "Skipped outbound {tag}: {detail}",
  "recover.pendingSettings":
    "An incomplete backend switch was detected; previous settings were restored",
  "recover.controllerUnavailable":
    "Capture controller is unavailable; TUN state was not restored",
  "recover.tunJournalMissing":
    "TUN journal missing after sing-box exited unexpectedly; fail-closed",
  "recover.tunCleanupUnconfirmed":
    "TUN cleanup unconfirmed after sing-box exited unexpectedly: {detail}",
  "recover.tunCleanupRetry": "TUN cleanup unconfirmed; fail-closed. Retry recovery",
  "recover.tunDnsFailed": "TUN DNS recovery failed: {detail}",
  "recover.orphanCoreUnconfirmed": "Leftover core cleanup unconfirmed: {detail}",
  "recover.tunStateUnconfirmed": "TUN state recovery unconfirmed: {detail}",
  "recover.proxyAfterExit":
    "System proxy recovery failed after sing-box exited unexpectedly: {detail}",
  "recover.tunShutdownUnconfirmed": "TUN capture shutdown unconfirmed: {detail}",
  "recover.autoStartFailed": "Auto-start failed: {detail}",
};

const dictionaries: Record<ResolvedLanguage, Record<MessageKey, string>> = {
  zh,
  en,
};

/** Currently active language used by `t()`. Initialized lazily so the module
 * can be imported before the environment's locale is fully available (e.g.
 * test setup files), then kept in sync by `applyLanguage`. */
let activeLanguage: ResolvedLanguage | undefined;

/** Translate a message key; `{name}` placeholders are replaced from params. */
export function t(
  key: MessageKey,
  params?: Record<string, string | number>,
): string {
  if (!activeLanguage) activeLanguage = resolveLanguage(readLanguagePreference());
  const template = dictionaries[activeLanguage][key];
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    params[name] !== undefined ? String(params[name]) : match,
  );
}

/** Apply a preference: resolves it, updates the active dictionary and the
 * document `lang` attribute. Returns the resolved language. */
export function applyLanguage(preference: LanguagePreference): ResolvedLanguage {
  const resolved = resolveLanguage(preference);
  activeLanguage = resolved;
  document.documentElement.lang = resolved;
  return resolved;
}

export function persistLanguagePreference(
  preference: LanguagePreference,
): void {
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
  const [preference, setPreferenceState] = useState<LanguagePreference>(
    readLanguagePreference,
  );
  const [resolved, setResolved] = useState<ResolvedLanguage>(() =>
    resolveLanguage(readLanguagePreference()),
  );

  useEffect(() => {
    setResolved(applyLanguage(preference));
    if (preference !== "system") return;
    // Re-resolve when the OS locale changes while following the system.
    const onLocaleChange = () => setResolved(applyLanguage("system"));
    window.addEventListener("languagechange", onLocaleChange);
    return () => window.removeEventListener("languagechange", onLocaleChange);
  }, [preference]);

  useEffect(() => {
    const onCustom = (event: Event) => {
      const next = (event as CustomEvent<LanguagePreference>).detail;
      if (!isLanguagePreference(next)) return;
      setPreferenceState(next);
      setResolved(applyLanguage(next));
    };
    window.addEventListener(LANGUAGE_CHANGE_EVENT, onCustom);
    return () => window.removeEventListener(LANGUAGE_CHANGE_EVENT, onCustom);
  }, []);

  function setPreference(next: LanguagePreference) {
    persistLanguagePreference(next);
    setPreferenceState(next);
    setResolved(applyLanguage(next));
  }

  return {
    preference,
    resolved,
    setPreference,
  };
}