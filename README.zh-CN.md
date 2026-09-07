<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="apps/desktop/src/assets/logo.png" alt="ice-box logo" width="112">
</a>

# ice-box

**代理，保持简单。**

面向 macOS 与 Windows 的轻量代理客户端。<br>
上手简单、启动迅速、运行安静——内置 [sing-box](https://github.com/SagerNet/sing-box) 内核。

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-555555?style=flat-square)](#安装)
[![Core](https://img.shields.io/badge/sing--box-1.13.19-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**下载**](https://github.com/yilong-musk/ice-box/releases/latest) · [**在线演示**](https://yilong-musk.github.io/ice-box/) · [安装](#安装) · [TUN](#tun-模式) · [文档](#文档) · [更新日志](CHANGELOG.md)

[English](README.md) · 简体中文

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/home.png" alt="ice-box 主页：代理状态、模式切换、出口节点和实时流量图" width="880">
</a>

<sub>安装前想先看看？<a href="https://yilong-musk.github.io/ice-box/">在线演示</a>在浏览器中运行真实的桌面端界面，后端为模拟数据。</sub>

</div>

<br>

## 安装

前往 [最新发布页](https://github.com/yilong-musk/ice-box/releases/latest) 下载对应平台的安装包。

| 平台 | 安装包 | 首次启动 |
|---|---|---|
| **macOS**（Apple Silicon） | `ice-box_<version>_aarch64.dmg` | 应用未签名。右键 **ice-box.app** 选择 **打开**，或执行 `xattr -dr com.apple.quarantine /Applications/ice-box.app`。 |
| **Windows**（x64） | `ice-box_<version>_x64-setup.exe` | 按用户安装的 NSIS 安装包，无需管理员权限。安装包没有 Authenticode 签名，SmartScreen 可能会要求确认。 |

**更新。** 从 0.1.5 起，ice-box 会在后台检查 GitHub Releases（可在设置中关闭），并在 **设置 → 应用更新** 中安装经签名校验的更新。0.1.5 之前的版本需要先手动升级一次。

## 快速上手

1. **导入订阅。** 打开 **订阅**，粘贴链接，点击 **导入**。sing-box JSON、Clash 和分享链接列表会自动识别。需要的话打开 **自动更新**。
2. **选择模式和节点。** 在 **主页** 选择 **规则**、**全局** 或 **直连**；在 **节点** 中选择出口并测速。
3. **启动代理服务。** 按下电源键。系统代理随即生效（启用 TUN 模式时则拉起 TUN 网卡），流量图开始滚动。

还没有订阅？ice-box 会以直连模式启动，内核、抓取方式和界面都可以先体验一遍。

## TUN 模式

TUN 在网络层接管流量，不读取系统代理的应用也能覆盖。可在设置或主页中启用。这个开关只决定*下一次*启动时的抓取方式；停止服务时，无论当前是哪种抓取方式都会被完整拆除。

| | macOS | Windows |
|---|---|---|
| **准备** | 首次启用时通过系统授权对话框安装一个小型特权辅助组件。 | 首次启用时弹出一次 UAC，创建 `ice-box-tun` 计划任务；之后的启动和停止不再提示。 |
| **覆盖范围** | IPv4 + IPv6 双栈，TCP 与 UDP。 | 当前内核下**仅 IPv4 TCP**。 |

Windows TUN 在当前固定内核上的能力：

| 可用 | 不可用 |
|---|---|
| IPv4 HTTPS 及其他 TCP 流量 | UDP / QUIC / HTTP3 |
| 经 DoT / DoH 的系统 DNS | IPv6 |
| 用于诊断的 Mixed 入站 | fake-ip DNS |

Windows 上需要 UDP 或 IPv6？请使用系统代理。完整的配置形态、提权模型和取舍见 [`docs/tun.md`](docs/tun.md)。

## 开发

Rust（stable）、Node.js 22，macOS 上还需要 Xcode Command Line Tools。

```bash
cd apps/desktop && npm install && cd ../..
npm run fetch-singbox
npm run dev
```

第二步会下载当前主机对应的固定版本 sing-box 内核。

| 命令 | 用途 |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` | 按指定主机目标运行 |
| `npm run gate` | fmt、clippy、Rust 测试、tsc、vitest |
| `npm run fetch-singbox && npm run build` | 构建 macOS `.dmg` |
| `npm run fetch-singbox -- win && npm run build:win` | 构建 Windows NSIS 安装包 |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` | 在真实主机上运行的验收门禁 |

```
apps/desktop    React 界面 + Tauri 外壳
apps/website    GitHub Pages 站点与在线演示
crates/         Rust 工作区：subscription、config、engine、proxy-sys、tun-sys、helper
docs/           架构、TUN、发布流程
```

## 文档

| 文档 | 内容 |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | 实现规范：进程模型、状态机、配置生成、IPC 契约 |
| [`docs/tun.md`](docs/tun.md) | TUN 抓取：各平台配置形态、提权、已知限制、实机门禁 |
| [`docs/release-process.md`](docs/release-process.md) | 版本号、更新日志、门禁、打标签、发布流水线 |
| [`CHANGELOG.md`](CHANGELOG.md) | 每个版本的变更 |

以上文档目前均为英文。

## 许可证

ice-box 是自由软件，以 [GNU 通用公共许可证第 3 版或更新版本](LICENSE) 发布。内置的 sing-box 内核同样采用 [GPL-3.0-or-later](third_party/sing-box/LICENSE)，并带有上游的命名限制条款；全部第三方声明见 [NOTICE](NOTICE)。

<div align="center">
<br>
<sub>基于 Tauri、React 与 sing-box 构建。</sub>
</div>
