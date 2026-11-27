<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/brand/switch-logo/assets/logo.gif" alt="ice-box logo" width="144">
</a>

# ice-box

**sing-box GUI terbaik.**

Klien proksi ringan untuk macOS, Windows, Linux, dan Android — dibangun di atas [sing-box](https://github.com/SagerNet/sing-box).

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-007ec6?style=flat-square)](#pemasangan)
[![Core](https://img.shields.io/badge/sing--box-1.14.4-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**Unduh**](https://github.com/yilong-musk/ice-box/releases/latest) · [**Demo Langsung**](https://yilong-musk.github.io/ice-box/) · [Pemasangan](#pemasangan) · [Pengembangan](#pengembangan) · [Dokumentasi](#dokumentasi) · [Catatan Perubahan](CHANGELOG.md)

**Bahasa Indonesia** · [Deutsch](README.de.md) · [English](README.md) · [Español](README.es.md) · [Français](README.fr.md) · [Português (BR)](README.pt-BR.md) · [Tiếng Việt](README.vi.md) · [Türkçe](README.tr.md)<br>[Русский](README.ru.md) · [Українська](README.uk.md) · [العربية](README.ar.md) · [فارسی](README.fa.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [简体中文](README.zh-CN.md) · [繁體中文](README.zh-TW.md)

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/hero.png" alt="Antarmuka desktop dan Android ice-box" width="880">
</a>

<sub>Coba antarmuka desktop dan Android langsung di browser melalui <a href="https://yilong-musk.github.io/ice-box/">Demo Langsung</a>.</sub>

</div>

## Fitur

- **Langganan**: Mendeteksi otomatis sing-box JSON, Clash, tautan berbagi, dan file JSON lokal; menampilkan kuota/masa berlaku; pembaruan otomatis di desktop.
- **Perutean & Node**: Mode **Aturan** / **Global** / **Langsung**, sakelar per aturan, aturan khusus, uji latensi, dan favorit.
- **Pengambilalihan Trafik**: Proksi sistem atau TUN di desktop ([`docs/tun.md`](docs/tun.md)); VPN sistem dengan perutean per aplikasi di Android.
- **Koneksi & Baki Sistem (Desktop)**: Pemeriksa koneksi aktif dengan pencarian/pengurutan/penutupan, ditambah kontrol baki sistem (kecepatan langsung & uji latensi di bilah menu macOS).
- **MCP untuk Agent (Desktop)**: Server MCP loopback lokal terautentikasi opsional dengan jembatan stdio bawaan yang berbagi sesi aktif ([`docs/mcp.md`](docs/mcp.md)).
- **Esensial**: Jalankan saat login, pembaruan dalam aplikasi bertanda tangan, kartu trafik/memori langsung, tema terang/gelap, dan 16 bahasa dengan dukungan RTL penuh (`ar`, `fa`).

## Pemasangan

Unduh dari [GitHub Releases](https://github.com/yilong-musk/ice-box/releases/latest):

| Platform | Persyaratan | Paket | Catatan |
|---|---|---|---|
| **macOS** (Apple Silicon) | macOS 12+ | `ice-box_<version>_aarch64.dmg` | Ditandatangani ad-hoc: izinkan di **Privasi & Keamanan → Tetap Buka**, atau jalankan `xattr -dr com.apple.quarantine /Applications/ice-box.app`. |
| **Windows** (x64) | Windows 10+ | `ice-box_<version>_x64-setup.exe` | Pemasang NSIS per pengguna (tanpa tanda tangan; konfirmasi SmartScreen). TUN hanya IPv4 TCP dan memerlukan Administrator. |
| **Linux** (x64 / arm64) | Ubuntu 22.04+ | `.AppImage` / `.deb` | `chmod +x *.AppImage` atau `sudo apt install ./ice-box_*.deb`. Baki GNOME memerlukan `gnome-shell-extension-appindicator`. |
| **Android** (arm64) | Android 14+ | `ice-box_<version>_android_arm64.apk` | Ditandatangani dengan keystore proyek; izinkan pemasangan dari sumber ini. |

- **Mulai cepat**: Impor langganan (**Langganan**) → pilih mode & node (**Beranda** / **Node**) → tekan tombol daya. Berjalan dalam mode langsung saja tanpa langganan.
- **Pembaruan**: Pembaruan dalam aplikasi bertanda tangan di **Setelan → Pembaruan Aplikasi** (`.deb` membuka Releases untuk `apt`; Android mengunduh APK untuk pemasangan sistem).

## Pengembangan

Memerlukan Rust (`rust-toolchain.toml`), Node.js 22, Xcode CLT (macOS), atau pustaka pengembangan GTK/WebKit (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libssl-dev`) di Linux. Android menambahkan JDK 17, Go, dan `ANDROID_HOME` (`scripts/ci-install-android-sdk.sh`).

```bash
cd apps/desktop && npm install && cd ../..
npm run fetch-singbox
npm run dev
```

| Perintah | Tujuan |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` / `dev:linux-x64` / `dev:linux-arm64` | Jalankan untuk target host tertentu |
| `npm run gate` | Gerbang penuh workspace (fmt, clippy, pengujian Rust/frontend, build, smoke test) |
| `npm run fetch-singbox && npm run build` | Bangun `.dmg` macOS |
| `npm run fetch-singbox -- win && npm run build:win` | Bangun pemasang Windows |
| `npm run fetch-singbox -- linux-x64 && npm run build:linux` | Bangun `.deb` & `.AppImage` Linux (`linux-arm64` di arm64) |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` / `acceptance:linux` / `acceptance:linux-tun` | Gerbang penerimaan langsung pada host nyata |
| `bash scripts/build-android-apk.sh debug arm64` | Bangun APK debug Android (`amd64` untuk emulator) |

```text
apps/desktop    Antarmuka React + shell desktop Tauri (macOS, Windows, Linux)
apps/mobile     Shell Tauri Android (menggunakan ulang antarmuka desktop)
apps/website    Situs GitHub Pages dan Demo Langsung
crates/         Workspace Rust (ice-app, ice-config, ice-core, ice-engine, ice-subscription, ice-proxy-sys, ice-tun-sys, ice-elevate, ice-helper, ice-types, ...)
```

## Dokumentasi

| Dokumen | Cakupan |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | Arsitektur, batas modul, dan alur data |
| [`docs/mcp.md`](docs/mcp.md) | Pengaturan MCP desktop, alat, dan model keamanan |
| [`docs/tun.md`](docs/tun.md) | Pengambilalihan TUN, elevasi, pemulihan, dan batasan platform |
| [`docs/testing.md`](docs/testing.md) | Gerbang lokal/CI dan pengujian langsung manual |
| [`docs/release-process.md`](docs/release-process.md) | Penomoran versi, penandatanganan, pemaketan, dan publikasi |
| [`CHANGELOG.md`](CHANGELOG.md) | Riwayat rilis |

Dokumen teknis di atas saat ini tersedia dalam bahasa Inggris.

## Lisensi

[GPL-3.0-or-later](LICENSE). Core sing-box yang dibundel: [GPL-3.0-or-later](third_party/sing-box/LICENSE) dengan pembatasan penamaan hulu. Pemberitahuan pihak ketiga: [NOTICE](NOTICE).

<div align="center">
<br>
<sub>Dibangun dengan Tauri, React, dan sing-box.</sub>
</div>
