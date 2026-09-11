<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="apps/desktop/src/assets/logo.png" alt="ice-box logo" width="112">
</a>

# ice-box

**Proxy, kept simple.**

A lightweight proxy client for macOS and Windows.<br>
Easy to set up, quick to start, quiet once it runs — with a bundled [sing-box](https://github.com/SagerNet/sing-box) core underneath.

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-555555?style=flat-square)](#install)
[![Core](https://img.shields.io/badge/sing--box-1.13.19-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**Download**](https://github.com/yilong-musk/ice-box/releases/latest) · [**Live Demo**](https://yilong-musk.github.io/ice-box/) · [Install](#install) · [TUN](#tun-mode) · [Docs](#documentation) · [Changelog](CHANGELOG.md)

English · [简体中文](README.zh-CN.md)

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/home.png" alt="ice-box home page: proxy status, mode switches, exit node and a live traffic chart" width="880">
</a>

<sub>Curious before installing? The <a href="https://yilong-musk.github.io/ice-box/">Live Demo</a> runs the real desktop UI in your browser against a simulated backend.</sub>

</div>

<br>

## Install

Grab the installer for your platform from the [latest release](https://github.com/yilong-musk/ice-box/releases/latest).

| Platform | Installer | First launch |
|---|---|---|
| **macOS** (Apple Silicon) | `ice-box_<version>_aarch64.dmg` | The app is unsigned. Right-click **ice-box.app** and choose **Open**, or run `xattr -dr com.apple.quarantine /Applications/ice-box.app`. |
| **Windows** (x64) | `ice-box_<version>_x64-setup.exe` | Per-user NSIS installer; no administrator rights required. The installer carries no Authenticode signature, so SmartScreen may ask you to confirm. |

**Updating.** Starting with 0.1.5, ice-box checks GitHub Releases in the background (toggle in Settings) and installs signature-verified updates from **Settings → App Updates**. Installations older than 0.1.5 need one manual upgrade first.

## Quick start

1. **Import a subscription.** Open **Subscriptions**, paste the URL, and press **Import**. sing-box JSON, Clash, and share-link lists are detected automatically. Turn on **Auto update** if you like.
2. **Pick a mode and a node.** On **Home** choose **Rule**, **Global**, or **Direct**. On **Nodes** pick an exit and test its latency.
3. **Start the proxy service.** Press the power button. The system proxy is applied (or the TUN adapter comes up when TUN Mode is enabled), and the traffic chart starts moving.

No subscription yet? ice-box starts in direct-only mode, so the core, the capture, and the UI can be explored before anything is imported.

## TUN mode

TUN captures traffic at the network layer, covering apps that never read the system proxy. See [`docs/tun.md`](docs/tun.md) for setup, switch behavior, platform coverage, and limitations.

## Development

Rust (stable), Node.js 22, and the Xcode Command Line Tools on macOS.

```bash
cd apps/desktop && npm install && cd ../..
npm run fetch-singbox   # download the pinned sing-box core for this host
npm run dev
```

| Command | Purpose |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` | Run against a specific host target |
| `npm run gate` | fmt, clippy, Rust tests, tsc, vitest |
| `npm run fetch-singbox && npm run build` | Build the macOS `.dmg` |
| `npm run fetch-singbox -- win && npm run build:win` | Build the Windows NSIS installer |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` | Live acceptance gates on a real host |

```
apps/desktop    React UI + Tauri shell
apps/website    GitHub Pages site and Live Demo
crates/         Rust workspace: subscription, config, engine, proxy-sys, tun-sys, helper
docs/           Architecture, TUN, testing, release process
```

## Documentation

| Document | What you will find |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | System structure, component responsibilities, data flow, and design boundaries |
| [`docs/tun.md`](docs/tun.md) | TUN behavior, state machine, recovery, platform configuration, and limits |
| [`docs/testing.md`](docs/testing.md) | Local and CI gate coverage, manual live tests |
| [`docs/release-process.md`](docs/release-process.md) | Version bump, changelog, gate, tag, release pipeline |
| [`CHANGELOG.md`](CHANGELOG.md) | What changed in every release |

## License

ice-box is free software, released under the [GNU General Public License v3.0 or later](LICENSE). The bundled sing-box core is also [GPL-3.0-or-later](third_party/sing-box/LICENSE), with its upstream naming restriction; see [NOTICE](NOTICE) for all third-party notices.

<div align="center">
<br>
<sub>Built with Tauri, React, and sing-box.</sub>
</div>
