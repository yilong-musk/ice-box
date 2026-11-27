<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/brand/switch-logo/assets/logo.gif" alt="ice-box logo" width="144">
</a>

# ice-box

**최고의 sing-box GUI.**

macOS, Windows, Linux, Android를 위한 가벼운 프록시 클라이언트.<br>
간편한 설정, 빠른 시작, 조용한 백그라운드 실행, 그리고 에이전트 친화적인 설계 — [sing-box](https://github.com/SagerNet/sing-box) 코어를 기반으로 합니다.

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-007ec6?style=flat-square)](#설치)
[![Core](https://img.shields.io/badge/sing--box-1.14.4-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**다운로드**](https://github.com/yilong-musk/ice-box/releases/latest) · [**라이브 데모**](https://yilong-musk.github.io/ice-box/) · [설치](#설치) · [TUN](#tun-모드) · [문서](#문서) · [변경 로그](CHANGELOG.md)

[Bahasa Indonesia](README.id.md) · [Deutsch](README.de.md) · [English](README.md) · [Español](README.es.md) · [Français](README.fr.md) · [Português (BR)](README.pt-BR.md) · [Tiếng Việt](README.vi.md) · [Türkçe](README.tr.md)<br>[Русский](README.ru.md) · [Українська](README.uk.md) · [العربية](README.ar.md) · [فارسی](README.fa.md) · [日本語](README.ja.md) · **한국어** · [简体中文](README.zh-CN.md) · [繁體中文](README.zh-TW.md)

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/hero.png" alt="데스크톱과 Android의 ice-box: 프록시 상태, 모드 전환, 출구 노드, 실시간 트래픽 그래프 및 노드 목록이 있는 모바일 홈 화면" width="880">
</a>

<sub><a href="https://yilong-musk.github.io/ice-box/">라이브 데모</a>는 시뮬레이션된 백엔드를 사용하여 브라우저에서 데스크톱 및 Android UI를 바로 실행합니다.</sub>

</div>

## 기능

- **구독.** sing-box JSON, Clash, 공유 링크 목록을 자동으로 인식하며 로컬 sing-box 설정 파일도 가져올 수 있습니다. 데스크톱에서는 프로필 자동 업데이트를 지원하고, Android에서는 가져오기 또는 수동 새로고침 시 업데이트됩니다. 제공자의 트래픽 사용량과 만료일도 각 구독에 표시됩니다.
- **라우팅.** **규칙**, **글로벌**, **직접 연결** 모드를 제공하며, 구독 규칙을 개별적으로 켜고 끌 수 있고 그 위에 사용자 지정 규칙을 추가할 수 있습니다.
- **노드.** 전략 그룹별로 출구를 선택하고, 단일 노드나 전체 목록의 지연 시간을 테스트하며 자주 쓰는 노드를 즐겨찾기로 고정할 수 있습니다.
- **연결.** 데스크톱(macOS, Windows, Linux)에서 모든 활성 연결의 대상, 일치한 규칙, 사용 중인 출구, 누적 전송량을 실시간으로 확인할 수 있습니다. 검색, 정렬 및 개별 또는 전체 연결 종료를 지원합니다.
- **트래픽 캡처.** 데스크톱에서는 시스템 프록시 또는 TUN을 사용합니다. Android에서는 시스템 VPN을 사용하며 앱별 라우팅(규칙 따르기, 프록시, 직접 연결, 차단)을 설정할 수 있습니다.
- **트레이.** 창을 열지 않고도 프록시 서비스를 시작 및 중지하고 모드, 구독, 출구 노드를 전환할 수 있습니다. macOS 메뉴 막대 항목에서는 실시간 트래픽도 표시되며 메뉴에서 지연 시간 테스트를 실행할 수 있습니다.
- **에이전트 접근.** 선택적으로 활성화하는 로컬 MCP 서비스를 통해 에이전트가 실행 중인 동일한 세션을 제어할 수 있습니다.
- **조용한 관리.** 로그인 시 자동 실행, 서명 검증된 인앱 업데이트, 홈 화면의 실시간 트래픽 및 메모리 표시, 완벽한 RTL 지원을 포함한 16개 언어(Bahasa Indonesia, Deutsch, English, Español, Français, Português (BR), Tiếng Việt, Türkçe, Русский, Українська, العربية, فارسی, 日本語, 한국어, 简体中文, 繁體中文), 라이트 및 다크 테마를 지원합니다.

## 설치

[최신 릴리스](https://github.com/yilong-musk/ice-box/releases/latest)에서 사용 중인 플랫폼의 설치 파일을 내려받으세요.

| 플랫폼 | 요구 사항 | 설치 파일 | 첫 실행 |
|---|---|---|---|
| **macOS** (Apple Silicon) | macOS 12 이상 | `ice-box_<version>_aarch64.dmg` | ad-hoc 서명만 적용되어(Developer ID 없음, 공증 없음) 처음 실행 시 **확인되지 않은 개발자** 경고가 표시됩니다. **시스템 설정 → 개인정보 보호 및 보안 → 확인 없이 열기**(macOS 15+)에서 허용하거나, 오른쪽 클릭 후 **열기**(이전 버전)를 선택하거나, `xattr -dr com.apple.quarantine /Applications/ice-box.app` 명령을 실행하세요. |
| **Windows** (x64) | Windows 10 이상 | `ice-box_<version>_x64-setup.exe` | 사용자별 NSIS 설치 프로그램으로 관리자 권한이 필요하지 않습니다. Authenticode 서명이 없어 SmartScreen에서 확인을 요청할 수 있습니다. |
| **Linux** (x64 / arm64) | Ubuntu 22.04+ 및 최신 배포판 | `ice-box_<version>_amd64.AppImage` / `aarch64.AppImage`<br>`ice-box_<version>_amd64.deb` / `arm64.deb` | AppImage에는 실행 권한을 부여(`chmod +x`)하고, `.deb`는 `sudo apt install ./ice-box_*.deb`로 설치하세요. GNOME 트레이 아이콘에는 AppIndicator 확장(`gnome-shell-extension-appindicator`)이 필요합니다. Wayland + NVIDIA 환경에서는 이미 설정되어 있지 않은 경우 ice-box가 자동으로 `WEBKIT_DISABLE_DMABUF_RENDERER=1`을 설정합니다. |
| **Android** (arm64) | Android 14 이상 | `ice-box_<version>_android_arm64.apk` | 프로젝트 키스토어로 서명되어 있습니다. Google Play에 등록되어 있지 않으므로 설치 시 이 출처의 앱 설치 권한을 요청합니다. |

**업데이트.** macOS, Windows(0.1.5+), Linux AppImage 설치 환경에서는 ice-box가 백그라운드에서 GitHub Releases를 확인하고 **설정 → 앱 업데이트**에서 서명 검증된 업데이트를 설치합니다. 이전 버전의 macOS/Windows 설치본은 먼저 한 번 수동으로 업그레이드해야 합니다. Linux `.deb` 설치본은 동일한 업데이트 목록을 확인하되 `apt` / `dpkg`로 업그레이드를 관리할 수 있도록 릴리스 다운로드 페이지를 엽니다. Android에서는 **설정 → 앱 업데이트**가 arm64 APK를 다운로드한 뒤 시스템 설치 프로그램에 확인을 넘깁니다.

## 빠른 시작

1. **구독 가져오기.** **구독**을 열고 URL을 붙여 넣은 뒤 **가져오기**를 누르면 형식이 자동으로 인식됩니다. 로컬 sing-box 설정 파일(JSON)을 사용하려면 **설정 파일**을 선택하세요. 원격 URL이 없으므로 새로고침할 수 없습니다.
2. **모드와 노드 선택.** **홈**에서 **규칙**, **글로벌**, 또는 **직접 연결**을 선택합니다. 데스크톱에서는 **노드**를 열어 출구를 선택하고 지연 시간을 테스트하며, Android에서는 **홈**의 노드 목록을 사용합니다.
3. **프록시 서비스 시작.** 전원 버튼을 누릅니다. 데스크톱은 시스템 프록시를 적용하거나 활성화 시 TUN을 시작하며, Android는 시스템 권한 확인 후 VPN을 연결합니다.

아직 구독이 없으신가요? ice-box는 직접 연결 전용 모드로 시작되므로 구독을 가져오기 전에도 코어, 캡처 방식, UI를 먼저 살펴볼 수 있습니다.

## 로그인 시 자동 실행

macOS, Windows, Linux에서 **설정 → 시작 → 로그인 시 자동 실행**을 활성화하세요. 로그인 시 자동 실행되면 창이 숨겨진 상태로 트레이에 상주하며 코어를 시작하고 마지막 프록시 상태를 복원합니다.

비활성화할 때도 동일한 스위치를 사용하세요. macOS의 **로그인 항목**에서 항목을 꺼도 앱 설정이 해제되지 않습니다. macOS에서는 디스크 이미지에서 직접 실행하면 거부되므로 활성화하기 전에 앱을 설치하세요. Linux에서는 `~/.config/autostart/com.yilong-musk.icebox.desktop`에 항목이 기록됩니다(AppImage로 실행한 경우 `$APPIMAGE`를 기록합니다).

Android에는 로그인 항목이 없습니다. **설정 → 시스템**에서 배터리 최적화 예외, 엄격한 비공개 DNS, 항상 사용 설정된 VPN에 대해 안내합니다.

## TUN 모드

macOS, Windows, Linux에서 TUN은 네트워크 계층에서 트래픽을 캡처하므로 시스템 프록시를 읽지 않는 앱도 모두 포함합니다. Windows TUN은 IPv4 TCP만 처리하며(UDP/QUIC 및 IPv6 미지원) 관리자 계정이 필요합니다. 이러한 환경에서는 시스템 프록시를 대안으로 사용할 수 있습니다. Linux TUN은 Polkit(`pkexec`)으로 systemd `ice-helper` 데몬을 설치하고 `systemd-resolved`(`resolvectl`) 또는 정적 `/etc/resolv.conf`를 통해 DNS를 관리합니다. 설정 방법, 전환 동작, 플랫폼 지원 범위 및 제한 사항은 [`docs/tun.md`](docs/tun.md)를 참고하세요. Android는 시스템 VPN을 사용하며 자세한 내용은 [`docs/architecture.md`](docs/architecture.md#clients)를 참고하세요.

## MCP를 통한 에이전트 접근

데스크톱 앱(macOS, Windows, Linux)은 선택적으로 켤 수 있는 로컬 MCP 서비스를 제공합니다. **연결**과 **설정** 사이의 **MCP**를 열어 활성화하고 생성된 클라이언트 설정을 복사하세요. 에이전트는 별도의 런타임을 띄우지 않고 실행 중인 세션의 설정, 구독, 데이터, 코어, 프록시 상태를 그대로 공유합니다.

- **완전한 도구 세트.** 상태, 설정, 트래픽, 구독, 노드와 그룹, 규칙, 활성 연결, 로그, 진단, 프록시 제어, 보호된 TUN 복구를 모두 지원합니다. 가능한 경우 결과에 구조화된 콘텐츠를 포함하며 앱 오류 코드를 그대로 유지합니다.
- **기본부터 안전한 설계.** MCP는 인증된 로컬 루프백에서만 수신하며, 복사되는 클라이언트 설정에는 절대 포함되지 않는 무작위 제어 토큰을 사용합니다.
- **내장 stdio 브리지.** 생성된 설정은 동일한 실행 파일을 브리지 모드로 실행하므로 별도의 데몬이나 프로토콜 어댑터가 필요하지 않습니다.

MCP는 데스크톱 전용입니다. Android에는 MCP가 없으며 웹사이트 데모는 시뮬레이션된 서비스를 사용합니다. 설정, 수명 주기 및 보안 경계는 [`docs/mcp.md`](docs/mcp.md)를 참고하세요.

## 개발

Rust(`rust-toolchain.toml`에 고정된 버전으로 최초 사용 시 rustup이 자동 설치), Node.js 22, macOS에서는 Xcode Command Line Tools, Linux에서는 GTK/WebKit 개발 패키지(`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libssl-dev`)가 필요합니다. Android 빌드에는 JDK 17, Go 및 `scripts/ci-install-android-sdk.sh`로 패키지를 설치한 Android SDK(`ANDROID_HOME`)도 필요합니다.

```bash
cd apps/desktop && npm install && cd ../..
npm run fetch-singbox   # 현재 호스트에 맞는 고정 버전의 sing-box 코어 다운로드
npm run dev
```

| 명령 | 용도 |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` / `dev:linux-x64` / `dev:linux-arm64` | 특정 호스트 타깃으로 실행 |
| `npm run gate` | 전체 워크스페이스 게이트: fmt, clippy, Rust 테스트, 타입 검사, 빌드, 스모크 테스트, fixtures |
| `npm run fetch-singbox && npm run build` | macOS `.dmg` 빌드 |
| `npm run fetch-singbox -- win && npm run build:win` | Windows NSIS 설치 파일 빌드 |
| `npm run fetch-singbox -- linux-x64 && npm run build:linux` | Linux `.deb` 및 `.AppImage` 빌드(arm64에서는 `linux-arm64` 전달) |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` / `acceptance:linux` / `acceptance:linux-tun` | 실제 호스트에서의 라이브 인수 테스트 |
| `bash scripts/build-android-apk.sh debug arm64` | arm64 기기용 디버그 APK |
| `bash scripts/build-android-apk.sh debug amd64` | 에뮬레이터용 디버그 APK |

```
apps/desktop    macOS, Windows, Linux용 React UI + Tauri 셸
apps/mobile     Android 셸(데스크톱 UI 재사용)
apps/website    GitHub Pages 사이트 및 라이브 데모
crates/         Rust 워크스페이스(주요 크레이트): ice-app, ice-config, ice-core, ice-engine, ice-subscription, ice-proxy-sys, ice-tun-sys, ice-elevate, ice-helper, ice-types, ...
```

## 문서

| 문서 | 내용 |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | 시스템 구조, 컴포넌트 역할, 데이터 흐름 및 설계 경계 |
| [`docs/mcp.md`](docs/mcp.md) | 데스크톱 MCP 설정, 공유 런타임 모델, 도구 및 보안 |
| [`docs/tun.md`](docs/tun.md) | TUN 동작, 복구, 플랫폼 설정 및 제한 사항 |
| [`docs/testing.md`](docs/testing.md) | 로컬 및 CI 게이트 범위, 수동 실기기 테스트 |
| [`docs/release-process.md`](docs/release-process.md) | 버전 관리, 서명, 패키징 및 배포 절차 |
| [`CHANGELOG.md`](CHANGELOG.md) | 릴리스 기록 |

위 기술 문서는 현재 영어로만 제공됩니다.

## 라이선스

ice-box는 [GNU 일반 공중 사용 허가서 v3.0 이상](LICENSE)으로 배포되는 자유 소프트웨어입니다. 번들된 sing-box 코어 역시 [GPL-3.0-or-later](third_party/sing-box/LICENSE)이며 업스트림의 명칭 제한이 적용됩니다. 모든 서드파티 고지는 [NOTICE](NOTICE)를 참고하세요.

<div align="center">
<br>
<sub>Tauri, React, sing-box로 제작되었습니다.</sub>
</div>
