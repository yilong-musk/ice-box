<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/brand/switch-logo/assets/logo.gif" alt="ice-box logo" width="144">
</a>

# ice-box

**最好的 sing-box GUI。**

面向 macOS、Windows、Linux 與 Android 的輕量代理用戶端。<br>
上手簡單、啟動迅速、執行安靜，且對 Agent 友好——基於 [sing-box](https://github.com/SagerNet/sing-box) 核心。

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-007ec6?style=flat-square)](#安裝)
[![Core](https://img.shields.io/badge/sing--box-1.14.4-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**下載**](https://github.com/yilong-musk/ice-box/releases/latest) · [**即時演示**](https://yilong-musk.github.io/ice-box/) · [安裝](#安裝) · [TUN](#tun-模式) · [文件](#文件) · [更新日誌](CHANGELOG.md)

[Bahasa Indonesia](README.id.md) · [Deutsch](README.de.md) · [English](README.md) · [Español](README.es.md) · [Français](README.fr.md) · [Português (BR)](README.pt-BR.md) · [Tiếng Việt](README.vi.md) · [Türkçe](README.tr.md)<br>[Русский](README.ru.md) · [Українська](README.uk.md) · [العربية](README.ar.md) · [فارسی](README.fa.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [简体中文](README.zh-CN.md) · **繁體中文**

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/hero.png" alt="ice-box 桌面端與 Android 端：代理狀態、模式切換、出口節點、即時流量圖，以及手機端首頁的節點清單" width="880">
</a>

<sub><a href="https://yilong-musk.github.io/ice-box/">即時演示</a>可在瀏覽器中以模擬後端體驗桌面端與 Android 介面。</sub>

</div>

## 功能

- **訂閱。** 自動辨識 sing-box JSON、Clash 與分享連結清單，也能匯入本機 sing-box 設定檔。桌面端支援自動更新設定檔；Android 則於匯入或手動重新整理時更新。服務商回報的流量與到期日會顯示在每筆訂閱上。
- **分流。** **規則**、**全域**、**直連** 三種模式，每條訂閱規則都可獨立開關，還能疊加自訂規則。
- **節點。** 依策略組選擇出口，支援單一節點或整批測試延遲，並可將常用節點釘選收藏。
- **連線。** 在桌面端（macOS、Windows、Linux）即時檢視每條活動連線：目標位址、命中規則、使用的出口與累計流量；支援搜尋、排序，以及關閉單一或全部連線。
- **流量接管。** 桌面端提供系統代理或 TUN；Android 使用系統 VPN，並支援依應用程式分流（跟隨規則、代理、直連或封鎖）。
- **系統匣。** 無需開啟視窗即可啟停代理服務，切換模式、訂閱或出口節點。macOS 選單列項目還會顯示即時網速，並可直接在選單中測試延遲。
- **Agent 接入。** 可選的本機 MCP 服務，讓 Agent 直接操作同一個正在執行的工作階段。
- **省心。** 登入時自動啟動、簽章驗證的應用程式內更新、首頁即時流量與記憶體讀數、16 種介面語言（Bahasa Indonesia、Deutsch、English、Español、Français、Português (BR)、Tiếng Việt、Türkçe、Русский、Українська、العربية、فارسی、日本語、한국어、简体中文、繁體中文，含完整 RTL 支援）、淺色與深色主題。

## 安裝

從 [最新版本](https://github.com/yilong-musk/ice-box/releases/latest) 頁面下載對應平台的安裝程式。

| 平台 | 系統需求 | 安裝程式 | 首次啟動 |
|---|---|---|---|
| **macOS**（Apple Silicon） | macOS 12 或更高 | `ice-box_<version>_aarch64.dmg` | 僅具 ad-hoc 簽章（無 Developer ID、未公證），首次開啟會出現**無法驗證開發者**警告。請至 **系統設定 → 隱私權與安全性 → 強制打開**（macOS 15+）、按右鍵選擇 **打開**（較舊版本），或執行 `xattr -dr com.apple.quarantine /Applications/ice-box.app`。 |
| **Windows**（x64） | Windows 10 或更高 | `ice-box_<version>_x64-setup.exe` | 單一使用者 NSIS 安裝程式，無需系統管理員權限。因不含 Authenticode 簽章，SmartScreen 可能會要求確認。 |
| **Linux**（x64 / arm64） | Ubuntu 22.04+ 等主流發行版 | `ice-box_<version>_amd64.AppImage` / `aarch64.AppImage`<br>`ice-box_<version>_amd64.deb` / `arm64.deb` | AppImage 請先賦予執行權限（`chmod +x`）；`.deb` 可透過 `sudo apt install ./ice-box_*.deb` 安裝。GNOME 系統匣圖示需安裝 AppIndicator 擴充套件（`gnome-shell-extension-appindicator`）。在 Wayland + NVIDIA 環境下，若未事先設定，ice-box 會自動套用 `WEBKIT_DISABLE_DMABUF_RENDERER=1`。 |
| **Android**（arm64） | Android 14 或更高 | `ice-box_<version>_android_arm64.apk` | 使用專案金鑰庫簽署。未上架 Google Play；安裝時 Android 會詢問是否允許安裝來自此來源的應用程式。 |

**更新。** 在 macOS、Windows（0.1.5+）與 Linux AppImage 安裝環境中，ice-box 會在背景檢查 GitHub Releases，並透過 **設定 → 應用程式更新** 安裝經簽章驗證的更新；較舊的 macOS/Windows 版本需先手動升級一次。Linux `.deb` 安裝同樣會檢查更新目錄，並開啟版本下載頁面，讓升級持續由 `apt` / `dpkg` 管理。在 Android 上，**設定 → 應用程式更新** 會下載 arm64 APK 並交由系統安裝程式確認。

## 快速上手

1. **匯入訂閱。** 開啟 **訂閱**，貼上網址並點擊 **匯入**，格式將自動辨識。若要改用本機 sing-box 設定檔（JSON），請選擇 **設定檔**；因無遠端網址，無法重新整理。
2. **選擇模式與節點。** 在 **首頁** 選擇 **規則**、**全域** 或 **直連**。桌面端可開啟 **節點** 挑選出口並測速；Android 則直接使用 **首頁** 上的節點清單。
3. **啟動代理服務。** 按下電源按鈕。桌面端會套用系統代理，或於啟用時啟動 TUN；Android 會在系統授權提示後連線 VPN。

還沒有訂閱？ice-box 會以僅直連模式啟動，讓你在匯入任何內容前就能先體驗核心、接管方式與介面。

## 登入時自動啟動

在 macOS、Windows 與 Linux 上，啟用 **設定 → 啟動 → 登入時自動啟動**。登入啟動時視窗會保持隱藏並常駐於系統匣，同時啟動核心並還原上次的代理狀態。

關閉時也請使用同一個開關：僅在 macOS 的 **登入項目** 中停用並不會清除應用程式內的設定。在 macOS 上啟用前請先安裝應用程式，直接從磁碟映像檔執行會被拒絕。Linux 會將啟動項目寫入 `~/.config/autostart/com.yilong-musk.icebox.desktop`（以 AppImage 執行時會記錄 `$APPIMAGE`）。

Android 沒有登入項目。**設定 → 系統** 頁面說明了電池最佳化豁免、嚴格私人 DNS 與永遠開啟的 VPN。

## TUN 模式

在 macOS、Windows 與 Linux 上，TUN 於網路層擷取流量，涵蓋不讀取系統代理的應用程式。Windows TUN 僅處理 IPv4 TCP（不支援 UDP/QUIC 與 IPv6）且需要系統管理員帳戶；這些情境下仍可使用系統代理作為替代方案。Linux TUN 透過 Polkit（`pkexec`）安裝 systemd `ice-helper` 常駐程式，並透過 `systemd-resolved`（`resolvectl`）或靜態 `/etc/resolv.conf` 管理 DNS。設定方式、切換行為、平台支援與限制詳見 [`docs/tun.md`](docs/tun.md)。Android 使用系統 VPN，詳見 [`docs/architecture.md`](docs/architecture.md#clients)。

## 透過 MCP 接入 Agent

桌面應用程式（macOS、Windows 與 Linux）提供可選的本機 MCP 服務。開啟位於 **連線** 與 **設定** 之間的 **MCP** 頁面，啟用服務並複製產生的用戶端設定。Agent 會共用目前工作階段的設定、訂閱、資料、核心與代理狀態，而不會另行啟動第二個執行環境。

- **完整工具集。** 涵蓋狀態、設定、流量、訂閱、節點與策略組、規則、活動連線、日誌、診斷、代理控制與受保護的 TUN 復原。結果在適用時會附帶結構化內容，並保留應用程式錯誤代碼。
- **預設即安全。** MCP 僅監聽經身分驗證的本機回環位址，使用隨機產生的控制權杖，且該權杖絕不會出現在複製的用戶端設定中。
- **內建 stdio 橋接。** 產生的設定會以橋接模式啟動同一個執行檔，無需額外的背景常駐程式或協定轉接器。

MCP 僅限桌面端：Android 不提供 MCP，網站演示則使用模擬服務。設定、生命週期與安全邊界詳見 [`docs/mcp.md`](docs/mcp.md)。

## 開發

需要 Rust（版本固定於 `rust-toolchain.toml`，初次使用時 rustup 會自動安裝）、Node.js 22、macOS 上的 Xcode Command Line Tools，以及 Linux 上的 GTK/WebKit 開發套件（`libwebkit2gtk-4.1-dev`、`libgtk-3-dev`、`libayatana-appindicator3-dev`、`librsvg2-dev`、`libssl-dev`）。建置 Android 另需 JDK 17、Go，以及透過 `scripts/ci-install-android-sdk.sh` 安裝相關套件的 Android SDK（`ANDROID_HOME`）。

```bash
cd apps/desktop && npm install && cd ../..
npm run fetch-singbox   # 下載符合目前主機的固定版本 sing-box 核心
npm run dev
```

| 指令 | 用途 |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` / `dev:linux-x64` / `dev:linux-arm64` | 針對特定主機目標執行 |
| `npm run gate` | 完整工作區檢查關卡：fmt、clippy、Rust 測試、型別檢查、建置、冒煙測試、fixtures |
| `npm run fetch-singbox && npm run build` | 建置 macOS `.dmg` |
| `npm run fetch-singbox -- win && npm run build:win` | 建置 Windows NSIS 安裝程式 |
| `npm run fetch-singbox -- linux-x64 && npm run build:linux` | 建置 Linux `.deb` 與 `.AppImage`（arm64 請改傳 `linux-arm64`） |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` / `acceptance:linux` / `acceptance:linux-tun` | 在真實主機上執行實機驗收測試 |
| `bash scripts/build-android-apk.sh debug arm64` | 供 arm64 裝置使用的除錯 APK |
| `bash scripts/build-android-apk.sh debug amd64` | 供模擬器使用的除錯 APK |

```
apps/desktop    macOS、Windows 與 Linux 的 React 介面 + Tauri 外殼
apps/mobile     Android 外殼，共用桌面端介面
apps/website    GitHub Pages 網站與即時演示
crates/         Rust 工作區（主要 crate）：ice-app, ice-config, ice-core, ice-engine, ice-subscription, ice-proxy-sys, ice-tun-sys, ice-elevate, ice-helper, ice-types, ...
```

## 文件

| 文件 | 內容說明 |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | 系統架構、元件職責、資料流與設計邊界 |
| [`docs/mcp.md`](docs/mcp.md) | 桌面端 MCP 設定、共用執行環境模型、工具與安全性 |
| [`docs/tun.md`](docs/tun.md) | TUN 行為、復原機制、平台組態與限制 |
| [`docs/testing.md`](docs/testing.md) | 本機與 CI 檢查覆蓋範圍、手動實機測試 |
| [`docs/release-process.md`](docs/release-process.md) | 版本控管、簽章、封裝與發布流程 |
| [`CHANGELOG.md`](CHANGELOG.md) | 版本更新紀錄 |

以上技術文件目前皆為英文。

## 授權條款

ice-box 是自由軟體，以 [GNU 通用公共授權條款第 3 版或更新版本](LICENSE) 發布。隨附的 sing-box 核心同樣採用 [GPL-3.0-or-later](third_party/sing-box/LICENSE)，並附帶上游的命名限制條款；所有第三方聲明詳見 [NOTICE](NOTICE)。

<div align="center">
<br>
<sub>使用 Tauri、React 與 sing-box 建置。</sub>
</div>
