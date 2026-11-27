<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/brand/switch-logo/assets/logo.gif" alt="ice-box logo" width="144">
</a>

# ice-box

**بهترین رابط گرافیکی sing-box.**

کلاینت پروکسی سبک برای macOS، Windows، Linux و Android.<br>
راه‌اندازی ساده، شروع سریع، اجرا در آرامش و سازگار با ایجنت‌ها — بر پایه هسته [sing-box](https://github.com/SagerNet/sing-box).

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-007ec6?style=flat-square)](#نصب)
[![Core](https://img.shields.io/badge/sing--box-1.14.4-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**دانلود**](https://github.com/yilong-musk/ice-box/releases/latest) · [**دموی زنده**](https://yilong-musk.github.io/ice-box/) · [نصب](#نصب) · [TUN](#حالت-tun) · [مستندات](#مستندات) · [تاریخچه تغییرات](CHANGELOG.md)

[Bahasa Indonesia](README.id.md) · [Deutsch](README.de.md) · [English](README.md) · [Español](README.es.md) · [Français](README.fr.md) · [Português (BR)](README.pt-BR.md) · [Tiếng Việt](README.vi.md) · [Türkçe](README.tr.md)<br>[Русский](README.ru.md) · [Українська](README.uk.md) · [العربية](README.ar.md) · **فارسی** · [日本語](README.ja.md) · [한국어](README.ko.md) · [简体中文](README.zh-CN.md) · [繁體中文](README.zh-TW.md)

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/hero.png" alt="برنامه ice-box در دسکتاپ و Android: وضعیت پروکسی، تغییر حالت‌ها، نود خروجی، نمودار زنده ترافیک و صفحه خانه مناسب موبایل همراه با فهرست نودها" width="880">
</a>

<sub><a href="https://yilong-musk.github.io/ice-box/">دموی زنده</a> رابط کاربری دسکتاپ و Android را در مرورگر شما با یک بک‌اند شبیه‌سازی‌شده اجرا می‌کند.</sub>

</div>

## ویژگی‌ها

- **اشتراک‌ها.** فرمت‌های sing-box JSON، Clash و فهرست لینک‌های اشتراک را به‌طور خودکار تشخیص می‌دهد و امکان وارد کردن فایل پیکربندی محلی sing-box نیز وجود دارد. نسخه دسکتاپ می‌تواند اشتراک‌ها را به‌طور خودکار بروزرسانی کند؛ Android هنگام وارد کردن یا تازه‌سازی دستی بروزرسانی می‌شود. حجم مصرفی و تاریخ انقضای ارائه‌دهنده روی هر اشتراک نمایش داده می‌شود.
- **مسیریابی.** حالت‌های **قانون**، **سراسری** و **مستقیم**، کلید مجزا برای روشن یا خاموش کردن هر قانون اشتراک و امکان افزودن قوانین سفارشی خودتان روی آن‌ها.
- **نودها.** انتخاب خروجی برای هر گروه استراتژی، تست تاخیر برای یک نود یا کل فهرست به‌صورت یکجا و سنجاق کردن برگزیده‌ها.
- **اتصال‌ها.** در دسکتاپ (macOS، Windows و Linux)، هر اتصال فعال را مشاهده کنید: مقصد، قانون منطبق، خروجی مورد استفاده و حجم داده‌های منتقل‌شده. امکان جستجو، مرتب‌سازی و بستن یک یا همه اتصال‌ها وجود دارد.
- **هدایت ترافیک.** پروکسی سیستم یا TUN در دسکتاپ؛ VPN سیستم در Android با مسیریابی مجزا برای هر برنامه (طبق قوانین، پروکسی، مستقیم یا مسدود).
- **سینی سیستم.** بدون باز کردن پنجره، سرویس پروکسی را شروع یا متوقف کنید و حالت، اشتراک یا نود خروجی را تغییر دهید. در macOS آیتم نوار منو ترافیک زنده را نیز نمایش می‌دهد و منو امکان اجرای تست تاخیر را دارد.
- **دسترسی ایجنت.** یک سرویس محلی اختیاری MCP به ایجنت اجازه می‌دهد در همان نشست در حال اجرا کار کند.
- **بی‌دردسر.** اجرا هنگام ورود به سیستم، بروزرسانی درون‌برنامه‌ای با تایید امضا، نمایش زنده ترافیک و حافظه در **خانه**، ۱۶ زبان با پشتیبانی کامل RTL (Bahasa Indonesia, Deutsch, English, Español, Français, Português (BR), Tiếng Việt, Türkçe, Русский, Українська, العربية, فارسی, 日本語, 한국어, 简体中文, 繁體中文)، و پوسته‌های روشن و تیره.

## نصب

فایل نصب متناسب با پلتفرم خود را از [آخرین نسخه](https://github.com/yilong-musk/ice-box/releases/latest) دریافت کنید.

| پلتفرم | پیش‌نیاز | فایل نصب | اجرای اول |
|---|---|---|---|
| **macOS** (Apple Silicon) | macOS 12 یا جدیدتر | `ice-box_<version>_aarch64.dmg` | فقط دارای امضای ad-hoc است (بدون Developer ID و بدون notarization)، بنابراین در اجرای اول هشدار **توسعه‌دهنده ناشناس** نمایش داده می‌شود. از مسیر **System Settings → Privacy & Security → Open Anyway** (در macOS 15+) به آن اجازه دهید، در نسخه‌های قدیمی‌تر راست‌کلیک کرده و **Open** را بزنید، یا دستور `xattr -dr com.apple.quarantine /Applications/ice-box.app` را اجرا کنید. |
| **Windows** (x64) | Windows 10 یا جدیدتر | `ice-box_<version>_x64-setup.exe` | نصب‌کننده NSIS ویژه کاربر فعلی؛ بدون نیاز به دسترسی مدیر سیستم (Administrator). امضای Authenticode ندارد، بنابراین ممکن است SmartScreen از شما تایید بخواهد. |
| **Linux** (x64 / arm64) | Ubuntu 22.04+ / توزیع‌های مدرن | `ice-box_<version>_amd64.AppImage` / `aarch64.AppImage`<br>`ice-box_<version>_amd64.deb` / `arm64.deb` | فایل AppImage را قابل اجرا کنید (`chmod +x`) یا بسته `.deb` را با دستور `sudo apt install ./ice-box_*.deb` نصب کنید. آیکون سینی سیستم در GNOME به افزونه AppIndicator (`gnome-shell-extension-appindicator`) نیاز دارد. در محیط Wayland + NVIDIA، در صورتی که از قبل تنظیم نشده باشد، ice-box به‌طور خودکار متغیر `WEBKIT_DISABLE_DMABUF_RENDERER=1` را تنظیم می‌کند. |
| **Android** (arm64) | Android 14 یا جدیدتر | `ice-box_<version>_android_arm64.apk` | با کلید پروژه (keystore) امضا شده است. در Google Play منتشر نشده و Android برای نصب از این منبع از شما مجوز می‌خواهد. |

**بروزرسانی.** در macOS، Windows (نسخه 0.1.5+) و نصب‌های AppImage در Linux، برنامه ice-box در پس‌زمینه GitHub Releases را بررسی کرده و بروزرسانی‌های دارای امضای تاییدشده را از مسیر **تنظیمات ← بروزرسانی برنامه** نصب می‌کند؛ نسخه‌های قدیمی‌تر macOS و Windows ابتدا به یک بار ارتقای دستی نیاز دارند. نصب‌های `.deb` در Linux نیز همین فهرست را بررسی می‌کنند و صفحه دانلود نسخه جدید را باز می‌کنند تا ارتقا همچنان توسط `apt` / `dpkg` مدیریت شود. در Android، بخش **تنظیمات ← بروزرسانی برنامه** فایل arm64 APK را دانلود کرده و برای تایید به نصب‌کننده سیستم می‌سپارد.

## شروع سریع

1. **وارد کردن اشتراک.** بخش **اشتراک‌ها** را باز کنید، لینک را جای‌گذاری کرده و روی **وارد کردن** بزنید؛ فرمت به‌طور خودکار تشخیص داده می‌شود. اگر می‌خواهید از یک فایل پیکربندی محلی sing-box (JSON) استفاده کنید، **فایل پیکربندی** را انتخاب کنید؛ این فایل آدرس راه دور ندارد و قابل بروزرسانی نیست.
2. **انتخاب حالت و نود.** در صفحه **خانه** یکی از حالت‌های **قانون**، **سراسری** یا **مستقیم** را انتخاب کنید. در دسکتاپ برای انتخاب خروجی و تست تاخیر آن وارد **نودها** شوید؛ در Android از فهرست نودها در صفحه **خانه** استفاده کنید.
3. **شروع سرویس پروکسی.** دکمه پاور را بزنید. دسکتاپ پروکسی سیستم را اعمال می‌کند یا در صورت فعال بودن، TUN را اجرا می‌کند؛ Android پس از تایید مجوز سیستم به VPN متصل می‌شود.

هنوز اشتراکی ندارید؟ برنامه ice-box در حالت مستقیم اجرا می‌شود تا پیش از وارد کردن هر اشتراکی بتوانید هسته، نحوه هدایت ترافیک و رابط کاربری را بررسی کنید.

## اجرا هنگام ورود به سیستم

در macOS، Windows و Linux گزینه **تنظیمات ← شروع به کار ← اجرا هنگام ورود به سیستم** را فعال کنید. هنگام اجرای خودکار پس از ورود به سیستم، برنامه بدون باز کردن پنجره در سینی سیستم می‌ماند، هسته را اجرا می‌کند و آخرین وضعیت پروکسی را بازیابی می‌کند.

برای خاموش کردن نیز از همین کلید استفاده کنید: غیرفعال کردن آن در بخش **Login Items** در macOS این تنظیم را پاک نمی‌کند. پیش از فعال کردن این گزینه در macOS برنامه را نصب کنید، زیرا اجرای مستقیم از روی disk image رد می‌شود. در Linux این ورودی در مسیر `~/.config/autostart/com.yilong-musk.icebox.desktop` نوشته می‌شود (و هنگام اجرا از AppImage مقدار `$APPIMAGE` ثبت می‌گردد).

در Android گزینه اجرا هنگام ورود وجود ندارد. بخش **تنظیمات ← سیستم** درباره معافیت بهینه‌سازی باتری، DNS خصوصی سخت‌گیرانه و VPN همیشگی توضیح می‌دهد.

## حالت TUN

در macOS، Windows و Linux، حالت TUN ترافیک را در لایه شبکه دریافت می‌کند و برنامه‌هایی را که از پروکسی سیستم پیروی نمی‌کنند نیز پوشش می‌دهد. حالت TUN در Windows فقط IPv4 TCP را منتقل می‌کند (بدون UDP/QUIC یا IPv6) و به حساب کاربری مدیر سیستم (Administrator) نیاز دارد؛ در این موارد پروکسی سیستم همچنان جایگزین است. حالت TUN در Linux از Polkit (`pkexec`) برای نصب سرویس systemd به نام `ice-helper` استفاده می‌کند و DNS را از طریق `systemd-resolved` (`resolvectl`) یا فایل ایستای `/etc/resolv.conf` مدیریت می‌کند. برای جزئیات راه‌اندازی، رفتار کلید تغییر حالت، پوشش پلتفرم‌ها و محدودیت‌ها [`docs/tun.md`](docs/tun.md) را ببینید. نسخه Android از VPN سیستم استفاده می‌کند؛ [`docs/architecture.md`](docs/architecture.md#clients) را ببینید.

## دسترسی ایجنت با MCP

برنامه دسکتاپ (macOS، Windows و Linux) یک سرویس محلی اختیاری MCP ارائه می‌دهد. بخش **MCP** را بین **اتصال‌ها** و **تنظیمات** باز کنید، آن را فعال کرده و پیکربندی تولیدشده کلاینت را کپی کنید. ایجنت به جای اجرای یک نمونه جداگانه، تنظیمات، اشتراک‌ها، داده‌ها، هسته و وضعیت پروکسی همان نشست در حال اجرا را به اشتراک می‌گذارد.

- **مجموعه کامل ابزارها.** وضعیت، تنظیمات، ترافیک، اشتراک‌ها، نودها و گروه‌ها، قوانین، اتصال‌های زنده، گزارش‌ها، عیب‌یابی، کنترل پروکسی و بازیابی محافظت‌شده TUN. نتایج در صورت امکان محتوای ساختاریافته دارند و کدهای خطای برنامه را حفظ می‌کنند.
- **ایمن به‌طور پیش‌فرض.** سرویس MCP فقط روی آدرس loopback با احراز هویت و توکن کنترلی تصادفی گوش می‌دهد و این توکن هرگز در پیکربندی کپی‌شده کلاینت گنجانده نمی‌شود.
- **پل stdio داخلی.** پیکربندی تولیدشده همان فایل اجرایی را در حالت پل (bridge) راه‌اندازی می‌کند، بنابراین نیازی به سرویس پس‌زمینه جداگانه یا مبدل پروتکل نیست.

سرویس MCP فقط برای دسکتاپ است: Android فاقد MCP است و دموی وب‌سایت از یک سرویس شبیه‌سازی‌شده استفاده می‌کند. برای جزئیات راه‌اندازی، چرخه حیات و مرزهای امنیتی [`docs/mcp.md`](docs/mcp.md) را ببینید.

## توسعه

برای توسعه به Rust (نسخه تعیین‌شده در `rust-toolchain.toml` که rustup در اولین استفاده نصب می‌کند)، Node.js 22، ابزارهای Xcode Command Line Tools در macOS و بسته‌های توسعه GTK/WebKit (`libwebkit2gtk-4.1-dev`، `libgtk-3-dev`، `libayatana-appindicator3-dev`، `librsvg2-dev`، `libssl-dev`) در Linux نیاز است. ساخت نسخه Android علاوه بر این به JDK 17، Go و Android SDK (`ANDROID_HOME`) همراه با بسته‌های نصب‌شده توسط `scripts/ci-install-android-sdk.sh` نیاز دارد.

```bash
cd apps/desktop && npm install && cd ../..
# دانلود نسخه تعیین‌شده هسته sing-box برای این سیستم
npm run fetch-singbox
npm run dev
```

| دستور | کاربرد |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` / `dev:linux-x64` / `dev:linux-arm64` | اجرا برای یک هدف میزبان مشخص |
| `npm run gate` | گیت کامل فضای کار: fmt، clippy، تست‌های Rust، بررسی نوع‌ها، بیلدها، تست‌های smoke و fixtures |
| `npm run fetch-singbox && npm run build` | ساخت فایل `.dmg` برای macOS |
| `npm run fetch-singbox -- win && npm run build:win` | ساخت نصب‌کننده NSIS برای Windows |
| `npm run fetch-singbox -- linux-x64 && npm run build:linux` | ساخت بسته‌های `.deb` و `.AppImage` برای Linux (در arm64 مقدار `linux-arm64` بدهید) |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` / `acceptance:linux` / `acceptance:linux-tun` | گیت‌های پذیرش زنده روی دستگاه واقعی |
| `bash scripts/build-android-apk.sh debug arm64` | فایل APK دیباگ برای دستگاه arm64 |
| `bash scripts/build-android-apk.sh debug amd64` | فایل APK دیباگ برای شبیه‌ساز |

```
apps/desktop    رابط کاربری React + پوسته Tauri برای macOS، Windows و Linux
apps/mobile     پوسته Android؛ از رابط کاربری دسکتاپ دوباره استفاده می‌کند
apps/website    سایت GitHub Pages و دموی زنده
crates/         فضای کار Rust (crateهای اصلی): ice-app, ice-config, ice-core, ice-engine, ice-subscription, ice-proxy-sys, ice-tun-sys, ice-elevate, ice-helper, ice-types, ...
```

## مستندات

| سند | محتوا |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | ساختار سیستم، مسئولیت اجزا، جریان داده و مرزهای طراحی |
| [`docs/mcp.md`](docs/mcp.md) | راه‌اندازی MCP دسکتاپ، مدل زمان اجرای مشترک، ابزارها و امنیت |
| [`docs/tun.md`](docs/tun.md) | رفتار TUN، بازیابی، پیکربندی پلتفرم‌ها و محدودیت‌ها |
| [`docs/testing.md`](docs/testing.md) | پوشش گیت‌های محلی و CI و تست‌های زنده دستی |
| [`docs/release-process.md`](docs/release-process.md) | نسخه‌گذاری، امضای دیجیتال، بسته‌بندی و انتشار |
| [`CHANGELOG.md`](CHANGELOG.md) | تاریخچه نسخه‌ها |

مستندات بالا در حال حاضر به زبان انگلیسی هستند.

## مجوز

برنامه ice-box نرم‌افزاری آزاد است و تحت مجوز [GNU General Public License v3.0 یا جدیدتر](LICENSE) منتشر می‌شود. هسته داخلی sing-box نیز دارای مجوز [GPL-3.0-or-later](third_party/sing-box/LICENSE) همراه با محدودیت نام‌گذاری بالادستی خود است؛ برای مشاهده همه اعلان‌های شخص ثالث [NOTICE](NOTICE) را ببینید.

<div align="center">
<br>
<sub>ساخته‌شده با Tauri، React و sing-box.</sub>
</div>
