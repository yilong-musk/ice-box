<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/brand/switch-logo/assets/logo.gif" alt="ice-box logo" width="144">
</a>

# ice-box

**sing-box GUI terbaik.**

Klien proksi ringan untuk macOS, Windows, Linux, dan Android.<br>
Mudah diatur, cepat dijalankan, tenang saat berjalan, dan ramah Agent — dibangun di atas core [sing-box](https://github.com/SagerNet/sing-box).

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-007ec6?style=flat-square)](#pemasangan)
[![Core](https://img.shields.io/badge/sing--box-1.14.4-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**Unduh**](https://github.com/yilong-musk/ice-box/releases/latest) · [**Demo Langsung**](https://yilong-musk.github.io/ice-box/) · [Pemasangan](#pemasangan) · [TUN](#mode-tun) · [Dokumentasi](#dokumentasi) · [Catatan Perubahan](CHANGELOG.md)

**Bahasa Indonesia** · [Deutsch](README.de.md) · [English](README.md) · [Español](README.es.md) · [Français](README.fr.md) · [Português (BR)](README.pt-BR.md) · [Tiếng Việt](README.vi.md) · [Türkçe](README.tr.md)<br>[Русский](README.ru.md) · [Українська](README.uk.md) · [العربية](README.ar.md) · [فارسی](README.fa.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [简体中文](README.zh-CN.md) · [繁體中文](README.zh-TW.md)

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/hero.png" alt="ice-box di desktop dan Android: status proksi, sakelar mode, jalur keluar, grafik trafik langsung, serta Beranda ramah ponsel dengan daftar node" width="880">
</a>

<sub><a href="https://yilong-musk.github.io/ice-box/">Demo Langsung</a> menjalankan antarmuka desktop dan Android di browser Anda dengan backend simulasi.</sub>

</div>

## Fitur

- **Langganan.** sing-box JSON, Clash, dan daftar tautan berbagi dideteksi secara otomatis, serta mendukung impor file konfigurasi sing-box lokal. Desktop dapat memperbarui profil secara otomatis; Android memperbarui saat impor atau muat ulang manual. Trafik dan masa berlaku dari penyedia ditampilkan pada setiap langganan.
- **Perutean.** Mode **Aturan**, **Global**, dan **Langsung**, sakelar untuk setiap aturan langganan, serta aturan khusus Anda sendiri di atasnya.
- **Node.** Pilih jalur keluar untuk setiap grup strategi, uji latensi satu node atau seluruh daftar sekaligus, dan sematkan favorit.
- **Koneksi.** Di desktop (macOS, Windows, dan Linux), pantau setiap koneksi aktif: tujuan, aturan yang cocok, jalur keluar yang dipakai, dan jumlah trafik yang telah dikirim. Cari, urutkan, dan tutup satu atau semua koneksi.
- **Pengambilalihan.** Proksi sistem atau TUN di desktop; VPN sistem di Android, dengan perutean per aplikasi (ikuti aturan, proksi, langsung, atau blokir).
- **Baki sistem.** Mulai dan hentikan layanan proksi serta ganti mode, langganan, atau jalur keluar tanpa membuka jendela. Di macOS, item bilah menu juga menampilkan trafik langsung dan menunya dapat menjalankan uji latensi.
- **Akses Agent.** Layanan MCP lokal opsional memungkinkan Agent mengoperasikan sesi yang sama yang sedang berjalan.
- **Perawatan tanpa repot.** Jalankan saat login, pembaruan dalam aplikasi terverifikasi tanda tangan, pembacaan trafik dan memori langsung di Beranda, 16 bahasa dengan dukungan RTL penuh (Bahasa Indonesia, Deutsch, English, Español, Français, Português (BR), Tiếng Việt, Türkçe, Русский, Українська, العربية, فارسی, 日本語, 한국어, 简体中文, 繁體中文), serta tema terang dan gelap.

## Pemasangan

Unduh pemasang untuk platform Anda dari [rilis terbaru](https://github.com/yilong-musk/ice-box/releases/latest).

| Platform | Persyaratan | Pemasang | Peluncuran pertama |
|---|---|---|---|
| **macOS** (Apple Silicon) | macOS 12 atau lebih baru | `ice-box_<version>_aarch64.dmg` | Hanya ditandatangani ad-hoc (tanpa Developer ID, tidak dinotarisasi), sehingga peluncuran pertama menampilkan peringatan **pengembang tidak dikenal**. Izinkan di **Pengaturan Sistem → Privasi & Keamanan → Tetap Buka** (macOS 15+), klik kanan **Buka** (versi lama), atau jalankan `xattr -dr com.apple.quarantine /Applications/ice-box.app`. |
| **Windows** (x64) | Windows 10 atau lebih baru | `ice-box_<version>_x64-setup.exe` | Pemasang NSIS per pengguna; tidak memerlukan hak administrator. Tidak memiliki tanda tangan Authenticode, sehingga SmartScreen mungkin meminta konfirmasi Anda. |
| **Linux** (x64 / arm64) | Ubuntu 22.04+ / distro modern | `ice-box_<version>_amd64.AppImage` / `aarch64.AppImage`<br>`ice-box_<version>_amd64.deb` / `arm64.deb` | Jadikan AppImage dapat dieksekusi (`chmod +x`) atau pasang `.deb` dengan `sudo apt install ./ice-box_*.deb`. Ikon baki GNOME membutuhkan ekstensi AppIndicator (`gnome-shell-extension-appindicator`). Di Wayland + NVIDIA, ice-box otomatis mengatur `WEBKIT_DISABLE_DMABUF_RENDERER=1` kecuali sudah dikonfigurasi sebelumnya. |
| **Android** (arm64) | Android 14 atau lebih baru | `ice-box_<version>_android_arm64.apk` | Ditandatangani dengan keystore proyek. Tidak tersedia di Google Play; Android akan meminta izin untuk memasang aplikasi dari sumber ini. |

**Pembaruan.** Di macOS, Windows (0.1.5+), dan pemasangan Linux AppImage, ice-box memeriksa GitHub Releases di latar belakang dan memasang pembaruan terverifikasi tanda tangan dari **Setelan → Pembaruan Aplikasi**; pemasangan macOS/Windows versi lebih lama memerlukan satu kali peningkatan manual terlebih dahulu. Pemasangan Linux `.deb` memeriksa katalog yang sama lalu membuka halaman unduhan rilis agar peningkatan tetap dikelola oleh `apt` / `dpkg`. Di Android, **Setelan → Pembaruan Aplikasi** mengunduh APK arm64 dan menyerahkannya ke pemasang sistem untuk dikonfirmasi.

## Mulai cepat

1. **Impor langganan.** Buka **Langganan**, tempel URL, lalu tekan **Impor**; formatnya dideteksi otomatis. Untuk memakai file konfigurasi sing-box lokal (JSON), pilih **File konfigurasi**; file lokal tidak memiliki URL jarak jauh sehingga tidak dapat diperbarui.
2. **Pilih mode dan node.** Di **Beranda**, pilih **Aturan**, **Global**, atau **Langsung**. Di desktop, buka **Node** untuk memilih jalur keluar dan menguji latensinya; di Android, gunakan daftar node di **Beranda**.
3. **Jalankan layanan proksi.** Tekan tombol daya. Desktop akan menerapkan proksi sistem atau memulai TUN saat diaktifkan; Android akan menyambungkan VPN setelah dialog izin sistem.

Belum punya langganan? ice-box dimulai dalam mode langsung saja, sehingga core, pengambilalihan trafik, dan antarmuka dapat dieksplorasi sebelum mengimpor apa pun.

## Jalankan saat login

Di macOS, Windows, dan Linux, aktifkan **Setelan → Mulai Otomatis → Jalankan saat login**. Peluncuran saat login akan tetap berada di baki sistem dengan jendela tersembunyi, menjalankan core, dan memulihkan status proksi terakhir.

Matikan lewat sakelar yang sama: menonaktifkan entri di **Item Masuk** macOS tidak akan menghapus pengaturan ini. Pasang aplikasi terlebih dahulu sebelum mengaktifkannya di macOS, karena menjalankan langsung dari disk image akan ditolak. Di Linux, entri ditulis ke `~/.config/autostart/com.yilong-musk.icebox.desktop` (mencatat `$APPIMAGE` saat dijalankan dari AppImage).

Android tidak memiliki item login. **Setelan → Sistem** menjelaskan pengecualian pengoptimalan baterai, DNS Pribadi ketat, dan VPN selalu aktif.

## Mode TUN

Di macOS, Windows, dan Linux, TUN mengambil alih trafik pada lapisan jaringan, mencakup aplikasi yang tidak pernah membaca proksi sistem. TUN Windows hanya menangani IPv4 TCP (tanpa UDP/QUIC atau IPv6) dan memerlukan akun Administrator; proksi sistem tetap menjadi alternatif di sana. TUN Linux menggunakan Polkit (`pkexec`) untuk memasang daemon systemd `ice-helper` dan mengelola DNS melalui `systemd-resolved` (`resolvectl`) atau `/etc/resolv.conf` statis. Lihat [`docs/tun.md`](docs/tun.md) untuk pengaturan, perilaku sakelar, cakupan platform, dan batasannya. Android menggunakan VPN sistem; lihat [`docs/architecture.md`](docs/architecture.md#clients).

## Akses Agent dengan MCP

Aplikasi desktop (macOS, Windows, dan Linux) menyediakan layanan MCP lokal opsional. Buka **MCP** di antara **Koneksi** dan **Setelan**, aktifkan, lalu salin konfigurasi klien yang dihasilkan. Agent berbagi setelan, langganan, data, core, dan status proksi dari sesi yang sedang berjalan alih-alih menjalankan runtime terpisah.

- **Set alat lengkap.** Status, setelan, trafik, langganan, node dan grup, aturan, koneksi aktif, log, diagnostik, kontrol proksi, serta pemulihan TUN terlindungi. Hasil menyertakan konten terstruktur jika tersedia dan mempertahankan kode galat aplikasi.
- **Aman secara bawaan.** MCP hanya mendengarkan pada loopback terautentikasi dengan token kontrol acak yang tidak pernah disertakan dalam konfigurasi klien yang disalin.
- **Jembatan stdio bawaan.** Konfigurasi yang dihasilkan menjalankan berkas eksekusi yang sama dalam mode jembatan, sehingga tidak diperlukan daemon atau adaptor protokol terpisah.

MCP hanya tersedia di desktop: Android tidak memiliki MCP, dan demo situs web menggunakan layanan simulasi. Lihat [`docs/mcp.md`](docs/mcp.md) untuk pengaturan, siklus hidup, dan batas keamanan.

## Pengembangan

Rust (versi yang ditetapkan di `rust-toolchain.toml`, yang dipasang otomatis oleh rustup saat pertama digunakan), Node.js 22, Xcode Command Line Tools di macOS, dan paket pengembangan GTK/WebKit (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libssl-dev`) di Linux. Build Android juga memerlukan JDK 17, Go, dan Android SDK (`ANDROID_HOME`) dengan paket yang dipasang oleh `scripts/ci-install-android-sdk.sh`.

```bash
cd apps/desktop && npm install && cd ../..
npm run fetch-singbox   # unduh core sing-box versi tetap untuk host ini
npm run dev
```

| Perintah | Tujuan |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` / `dev:linux-x64` / `dev:linux-arm64` | Jalankan untuk target host tertentu |
| `npm run gate` | Gerbang validasi penuh workspace: fmt, clippy, pengujian Rust, pemeriksaan tipe, build, smoke test, fixtures |
| `npm run fetch-singbox && npm run build` | Bangun `.dmg` macOS |
| `npm run fetch-singbox -- win && npm run build:win` | Bangun pemasang NSIS Windows |
| `npm run fetch-singbox -- linux-x64 && npm run build:linux` | Bangun `.deb` dan `.AppImage` Linux (gunakan `linux-arm64` pada arm64) |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` / `acceptance:linux` / `acceptance:linux-tun` | Pengujian penerimaan langsung pada host nyata |
| `bash scripts/build-android-apk.sh debug arm64` | APK debug untuk perangkat arm64 |
| `bash scripts/build-android-apk.sh debug amd64` | APK debug untuk emulator |

```
apps/desktop    Antarmuka React + shell Tauri untuk macOS, Windows, dan Linux
apps/mobile     Shell Android; menggunakan ulang antarmuka desktop
apps/website    Situs GitHub Pages dan Demo Langsung
crates/         Workspace Rust (crate utama): ice-app, ice-config, ice-core, ice-engine, ice-subscription, ice-proxy-sys, ice-tun-sys, ice-elevate, ice-helper, ice-types, ...
```

## Dokumentasi

| Dokumen | Isi yang tersedia |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | Struktur sistem, tanggung jawab komponen, alur data, dan batas desain |
| [`docs/mcp.md`](docs/mcp.md) | Pengaturan MCP desktop, model runtime bersama, alat, dan keamanan |
| [`docs/tun.md`](docs/tun.md) | Perilaku TUN, pemulihan, konfigurasi platform, dan batasan |
| [`docs/testing.md`](docs/testing.md) | Cakupan gerbang lokal dan CI, pengujian langsung manual |
| [`docs/release-process.md`](docs/release-process.md) | Penomoran versi, penandatanganan, pemaketan, dan publikasi |
| [`CHANGELOG.md`](CHANGELOG.md) | Riwayat rilis |

Dokumen teknis di atas saat ini tersedia dalam bahasa Inggris.

## Lisensi

ice-box adalah perangkat lunak bebas yang dirilis di bawah [GNU General Public License v3.0 atau yang lebih baru](LICENSE). Core sing-box yang dibundel juga berlisensi [GPL-3.0-or-later](third_party/sing-box/LICENSE), beserta pembatasan penamaan dari hulu; lihat [NOTICE](NOTICE) untuk seluruh pemberitahuan pihak ketiga.

<div align="center">
<br>
<sub>Dibangun dengan Tauri, React, dan sing-box.</sub>
</div>
