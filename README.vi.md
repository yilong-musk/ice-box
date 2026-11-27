<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/brand/switch-logo/assets/logo.gif" alt="Logo ice-box" width="144">
</a>

# ice-box

**sing-box GUI tốt nhất.**

Ứng dụng proxy gọn nhẹ cho macOS, Windows, Linux và Android.<br>
Dễ thiết lập, khởi động nhanh, vận hành êm ái và thân thiện với Agent — xây dựng trên lõi [sing-box](https://github.com/SagerNet/sing-box).

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-007ec6?style=flat-square)](#cài-đặt)
[![Core](https://img.shields.io/badge/sing--box-1.14.4-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**Tải xuống**](https://github.com/yilong-musk/ice-box/releases/latest) · [**Demo trực tiếp**](https://yilong-musk.github.io/ice-box/) · [Cài đặt](#cài-đặt) · [TUN](#chế-độ-tun) · [Tài liệu](#tài-liệu) · [Nhật ký thay đổi](CHANGELOG.md)

[Bahasa Indonesia](README.id.md) · [Deutsch](README.de.md) · [English](README.md) · [Español](README.es.md) · [Français](README.fr.md) · [Português (BR)](README.pt-BR.md) · **Tiếng Việt** · [Türkçe](README.tr.md)<br>[Русский](README.ru.md) · [Українська](README.uk.md) · [العربية](README.ar.md) · [فارسی](README.fa.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [简体中文](README.zh-CN.md) · [繁體中文](README.zh-TW.md)

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/hero.png" alt="ice-box trên máy tính và Android: trạng thái proxy, chuyển đổi chế độ, nút đầu ra, biểu đồ lưu lượng trực tiếp và Trang chủ tối ưu cho điện thoại cùng danh sách nút" width="880">
</a>

<sub><a href="https://yilong-musk.github.io/ice-box/">Demo trực tiếp</a> chạy giao diện máy tính và Android ngay trong trình duyệt của bạn với phần nền mô phỏng.</sub>

</div>

## Tính năng

- **Gói đăng ký.** Tự động nhận diện sing-box JSON, Clash và danh sách liên kết chia sẻ, đồng thời hỗ trợ nhập tệp cấu hình sing-box cục bộ. Ứng dụng máy tính có thể tự động cập nhật hồ sơ; Android làm mới khi nhập hoặc khi bạn làm mới thủ công. Lưu lượng và ngày hết hạn từ nhà cung cấp được hiển thị trên từng gói đăng ký.
- **Định tuyến.** Các chế độ **Quy tắc**, **Toàn cục** và **Trực tiếp**, công tắc bật/tắt riêng cho từng quy tắc trong gói đăng ký, cùng quy tắc tùy chỉnh của riêng bạn xếp lên trên.
- **Nút.** Chọn đầu ra cho từng nhóm chiến lược, đo độ trễ của một nút hoặc cả danh sách cùng lúc và ghim các nút yêu thích.
- **Kết nối.** Trên máy tính (macOS, Windows và Linux), theo dõi mọi kết nối đang hoạt động: đích đến, quy tắc khớp, đầu ra sử dụng và lượng dữ liệu đã truyền. Hỗ trợ tìm kiếm, sắp xếp và đóng một hoặc toàn bộ kết nối.
- **Bắt lưu lượng.** Proxy hệ thống hoặc TUN trên máy tính; VPN hệ thống trên Android, kèm định tuyến theo từng ứng dụng (theo quy tắc, proxy, trực tiếp hoặc chặn).
- **Khay hệ thống.** Bật và tắt dịch vụ proxy cũng như đổi chế độ, gói đăng ký hoặc nút đầu ra mà không cần mở cửa sổ. Trên macOS, mục trên thanh menu còn hiển thị lưu lượng trực tiếp và cho phép đo độ trễ ngay trong menu.
- **Truy cập Agent.** Dịch vụ MCP cục bộ tùy chọn cho phép Agent thao tác trên cùng phiên đang chạy.
- **Vận hành êm ái.** Khởi chạy khi đăng nhập, cập nhật trong ứng dụng có xác minh chữ ký, thông số lưu lượng và bộ nhớ trực tiếp trên Trang chủ, 16 ngôn ngữ với hỗ trợ RTL đầy đủ (Bahasa Indonesia, Deutsch, English, Español, Français, Português (BR), Tiếng Việt, Türkçe, Русский, Українська, العربية, فارسی, 日本語, 한국어, 简体中文, 繁體中文), giao diện sáng và tối.

## Cài đặt

Tải bộ cài phù hợp với nền tảng của bạn từ [bản phát hành mới nhất](https://github.com/yilong-musk/ice-box/releases/latest).

| Nền tảng | Yêu cầu | Bộ cài | Lần chạy đầu tiên |
|---|---|---|---|
| **macOS** (Apple Silicon) | macOS 12 trở lên | `ice-box_<version>_aarch64.dmg` | Chỉ ký ad-hoc (không có Developer ID, chưa công chứng), nên lần chạy đầu tiên sẽ hiện cảnh báo **nhà phát triển không xác định**. Cho phép trong **Cài đặt hệ thống → Quyền riêng tư & Bảo mật → Vẫn mở** (macOS 15+), nhấp chuột phải chọn **Mở** (các bản cũ hơn), hoặc chạy `xattr -dr com.apple.quarantine /Applications/ice-box.app`. |
| **Windows** (x64) | Windows 10 trở lên | `ice-box_<version>_x64-setup.exe` | Bộ cài NSIS theo từng người dùng; không cần quyền quản trị viên. Không có chữ ký Authenticode nên SmartScreen có thể yêu cầu bạn xác nhận. |
| **Linux** (x64 / arm64) | Ubuntu 22.04+ / các bản phân phối hiện đại | `ice-box_<version>_amd64.AppImage` / `aarch64.AppImage`<br>`ice-box_<version>_amd64.deb` / `arm64.deb` | Cấp quyền thực thi cho AppImage (`chmod +x`) hoặc cài gói `.deb` bằng `sudo apt install ./ice-box_*.deb`. Biểu tượng khay hệ thống trên GNOME cần tiện ích mở rộng AppIndicator (`gnome-shell-extension-appindicator`). Trên Wayland + NVIDIA, ice-box tự động đặt `WEBKIT_DISABLE_DMABUF_RENDERER=1` nếu biến này chưa được cấu hình. |
| **Android** (arm64) | Android 14 trở lên | `ice-box_<version>_android_arm64.apk` | Được ký bằng keystore của dự án. Chưa phát hành trên Google Play; Android sẽ yêu cầu quyền cài đặt từ nguồn này. |

**Cập nhật.** Trên macOS, Windows (0.1.5+) và bản cài Linux AppImage, ice-box tự kiểm tra GitHub Releases trong nền và cài đặt bản cập nhật có xác minh chữ ký tại **Cài đặt → Cập nhật ứng dụng**; các bản cài macOS/Windows cũ hơn cần nâng cấp thủ công một lần trước. Bản cài Linux `.deb` kiểm tra cùng danh mục và mở trang tải bản phát hành để việc nâng cấp tiếp tục do `apt` / `dpkg` quản lý. Trên Android, **Cài đặt → Cập nhật ứng dụng** tải tệp APK arm64 và chuyển cho trình cài đặt hệ thống xác nhận.

## Bắt đầu nhanh

1. **Nhập gói đăng ký.** Mở **Gói đăng ký**, dán URL và nhấn **Nhập**; định dạng sẽ được tự động nhận diện. Để dùng tệp cấu hình sing-box cục bộ (JSON), hãy chọn **Tệp cấu hình**; tệp cục bộ không có URL từ xa nên không thể làm mới.
2. **Chọn chế độ và nút.** Tại **Trang chủ**, chọn **Quy tắc**, **Toàn cục** hoặc **Trực tiếp**. Trên máy tính, mở **Nút** để chọn đầu ra và đo độ trễ; trên Android, sử dụng danh sách nút ngay trên **Trang chủ**.
3. **Bật dịch vụ proxy.** Nhấn nút nguồn. Ứng dụng máy tính sẽ áp dụng proxy hệ thống hoặc khởi động TUN khi đã bật; Android sẽ kết nối VPN sau hộp thoại cấp quyền của hệ thống.

Chưa có gói đăng ký? ice-box khởi chạy ở chế độ chỉ kết nối trực tiếp, giúp bạn trải nghiệm lõi, cơ chế bắt lưu lượng và giao diện trước khi nhập bất cứ thứ gì.

## Khởi chạy khi đăng nhập

Trên macOS, Windows và Linux, bật **Cài đặt → Khởi động → Khởi chạy khi đăng nhập**. Khi khởi chạy lúc đăng nhập, ứng dụng sẽ ở trong khay hệ thống với cửa sổ ẩn, khởi chạy lõi và khôi phục trạng thái proxy gần nhất.

Tắt bằng chính công tắc này: tắt mục đó trong **Mục đăng nhập** của macOS sẽ không xóa cài đặt này. Hãy cài đặt ứng dụng trước khi bật trên macOS, vì việc chạy trực tiếp từ tệp ảnh đĩa sẽ bị từ chối. Trên Linux, mục khởi chạy được ghi vào `~/.config/autostart/com.yilong-musk.icebox.desktop` (ghi lại `$APPIMAGE` khi chạy từ AppImage).

Android không có mục khởi chạy khi đăng nhập. **Cài đặt → Hệ thống** giải thích về miễn trừ tối ưu hóa pin, DNS riêng tư nghiêm ngặt và VPN luôn bật.

## Chế độ TUN

Trên macOS, Windows và Linux, TUN bắt lưu lượng ở tầng mạng, bao phủ cả các ứng dụng không bao giờ đọc proxy hệ thống. TUN trên Windows chỉ chuyển tiếp IPv4 TCP (không hỗ trợ UDP/QUIC hay IPv6) và yêu cầu tài khoản Quản trị viên; proxy hệ thống vẫn là phương án thay thế ở đó. TUN trên Linux dùng Polkit (`pkexec`) để cài daemon systemd `ice-helper` và quản lý DNS qua `systemd-resolved` (`resolvectl`) hoặc tệp `/etc/resolv.conf` tĩnh. Xem [`docs/tun.md`](docs/tun.md) để biết cách thiết lập, hành vi công tắc, phạm vi hỗ trợ nền tảng và các giới hạn. Android sử dụng VPN hệ thống; xem [`docs/architecture.md`](docs/architecture.md#clients).

## Truy cập Agent với MCP

Ứng dụng máy tính (macOS, Windows và Linux) cung cấp dịch vụ MCP cục bộ tùy chọn. Mở **MCP** nằm giữa **Kết nối** và **Cài đặt**, bật dịch vụ và sao chép cấu hình client được tạo ra. Agent sẽ dùng chung cài đặt, gói đăng ký, dữ liệu, lõi và trạng thái proxy của phiên đang chạy thay vì khởi chạy một runtime khác.

- **Bộ công cụ đầy đủ.** Trạng thái, cài đặt, lưu lượng, gói đăng ký, nút và nhóm, quy tắc, kết nối trực tiếp, nhật ký, chẩn đoán, điều khiển proxy và khôi phục TUN được bảo vệ. Kết quả đi kèm nội dung có cấu trúc khi khả dụng và giữ nguyên mã lỗi ứng dụng.
- **An toàn theo mặc định.** MCP chỉ lắng nghe trên địa chỉ loopback có xác thực với mã điều khiển ngẫu nhiên, và mã này không bao giờ có trong cấu hình client được sao chép.
- **Cầu nối stdio tích hợp sẵn.** Cấu hình được tạo ra khởi chạy chính tệp thực thi đó ở chế độ cầu nối, nên không cần daemon riêng hay bộ chuyển đổi giao thức.

MCP chỉ dành cho máy tính: Android không có MCP và bản demo trên website dùng dịch vụ mô phỏng. Xem [`docs/mcp.md`](docs/mcp.md) để biết cách thiết lập, vòng đời và ranh giới bảo mật.

## Phát triển

Rust (phiên bản được ghim trong `rust-toolchain.toml`, được rustup cài tự động trong lần dùng đầu tiên), Node.js 22, Xcode Command Line Tools trên macOS và các gói phát triển GTK/WebKit (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libssl-dev`) trên Linux. Bản dựng Android còn cần JDK 17, Go và Android SDK (`ANDROID_HOME`) với các gói được cài bằng `scripts/ci-install-android-sdk.sh`.

```bash
cd apps/desktop && npm install && cd ../..
npm run fetch-singbox   # tải lõi sing-box đã ghim cho máy chủ này
npm run dev
```

| Lệnh | Mục đích |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` / `dev:linux-x64` / `dev:linux-arm64` | Chạy cho một nền tảng đích cụ thể |
| `npm run gate` | Toàn bộ cổng kiểm tra workspace: fmt, clippy, kiểm thử Rust, kiểm tra kiểu, bản dựng, smoke test, fixtures |
| `npm run fetch-singbox && npm run build` | Đóng gói `.dmg` cho macOS |
| `npm run fetch-singbox -- win && npm run build:win` | Đóng gói bộ cài NSIS cho Windows |
| `npm run fetch-singbox -- linux-x64 && npm run build:linux` | Đóng gói `.deb` và `.AppImage` cho Linux (truyền `linux-arm64` trên arm64) |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` / `acceptance:linux` / `acceptance:linux-tun` | Các cổng kiểm thử nghiệm thu trực tiếp trên máy thật |
| `bash scripts/build-android-apk.sh debug arm64` | APK gỡ lỗi cho thiết bị arm64 |
| `bash scripts/build-android-apk.sh debug amd64` | APK gỡ lỗi cho trình giả lập |

```
apps/desktop    Giao diện React + vỏ Tauri cho macOS, Windows và Linux
apps/mobile     Vỏ Android; tái sử dụng giao diện máy tính
apps/website    Trang GitHub Pages và Demo trực tiếp
crates/         Workspace Rust (các crate chính): ice-app, ice-config, ice-core, ice-engine, ice-subscription, ice-proxy-sys, ice-tun-sys, ice-elevate, ice-helper, ice-types, ...
```

## Tài liệu

| Tài liệu | Nội dung |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | Cấu trúc hệ thống, trách nhiệm thành phần, luồng dữ liệu và ranh giới thiết kế |
| [`docs/mcp.md`](docs/mcp.md) | Thiết lập MCP trên máy tính, mô hình runtime dùng chung, công cụ và bảo mật |
| [`docs/tun.md`](docs/tun.md) | Hành vi TUN, cơ chế khôi phục, cấu hình theo nền tảng và các giới hạn |
| [`docs/testing.md`](docs/testing.md) | Phạm vi cổng kiểm tra cục bộ và CI, kiểm thử trực tiếp thủ công |
| [`docs/release-process.md`](docs/release-process.md) | Đánh phiên bản, ký số, đóng gói và phát hành |
| [`CHANGELOG.md`](CHANGELOG.md) | Lịch sử phát hành |

Các tài liệu kỹ thuật trên hiện chỉ có bằng tiếng Anh.

## Giấy phép

ice-box là phần mềm tự do, được phát hành theo [Giấy phép Công cộng GNU phiên bản 3.0 trở lên](LICENSE). Lõi sing-box đi kèm cũng sử dụng [GPL-3.0-or-later](third_party/sing-box/LICENSE), kèm điều khoản hạn chế đặt tên từ dự án gốc; xem [NOTICE](NOTICE) cho toàn bộ thông báo của bên thứ ba.

<div align="center">
<br>
<sub>Xây dựng bằng Tauri, React và sing-box.</sub>
</div>
