<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/brand/switch-logo/assets/logo.gif" alt="ice-box logo" width="144">
</a>

# ice-box

**أفضل sing-box GUI.**

عميل بروكسي خفيف لأنظمة macOS وWindows وLinux وAndroid — مبني على نواة [sing-box](https://github.com/SagerNet/sing-box).

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-007ec6?style=flat-square)](#التثبيت)
[![Core](https://img.shields.io/badge/sing--box-1.14.4-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**التنزيل**](https://github.com/yilong-musk/ice-box/releases/latest) · [**العرض التفاعلي**](https://yilong-musk.github.io/ice-box/) · [التثبيت](#التثبيت) · [التطوير](#التطوير) · [التوثيق](#التوثيق) · [سجل التغييرات](CHANGELOG.md)

[Bahasa Indonesia](README.id.md) · [Deutsch](README.de.md) · [English](README.md) · [Español](README.es.md) · [Français](README.fr.md) · [Português (BR)](README.pt-BR.md) · [Tiếng Việt](README.vi.md) · [Türkçe](README.tr.md)<br>[Русский](README.ru.md) · [Українська](README.uk.md) · **العربية** · [فارسی](README.fa.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [简体中文](README.zh-CN.md) · [繁體中文](README.zh-TW.md)

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/hero.png" alt="واجهة ice-box على سطح المكتب وAndroid" width="880">
</a>

<sub>جرّب واجهتي سطح المكتب وAndroid داخل المتصفح عبر <a href="https://yilong-musk.github.io/ice-box/">العرض التفاعلي</a>.</sub>

</div>

## الميزات

- **الاشتراكات**: تعرّف تلقائي على sing-box JSON وClash وروابط المشاركة وملفات JSON المحلية؛ عرض الرصيد وتاريخ الانتهاء؛ تحديث تلقائي على سطح المكتب.
- **التوجيه والعُقد**: أوضاع **القواعد** / **شامل** / **مباشر**، مفاتيح لكل قاعدة، قواعد مخصصة، اختبارات زمن الاستجابة، والمفضلة.
- **التقاط الحركة**: بروكسي النظام أو TUN على سطح المكتب ([`docs/tun.md`](docs/tun.md))؛ وVPN النظام مع توجيه لكل تطبيق على Android.
- **الاتصالات وشريط النظام (سطح المكتب)**: مراقب اتصالات مباشر مع البحث والفرز والإغلاق، بالإضافة إلى تحكم من شريط النظام (وعرض السرعة واختبارات زمن الاستجابة في شريط القوائم على macOS).
- **MCP للوكلاء (سطح المكتب)**: خادم MCP محلي اختياري بمصادقة على عنوان الاسترجاع المحلي مع جسر stdio مدمج يشارك الجلسة الجارية ([`docs/mcp.md`](docs/mcp.md)).
- **الأساسيات**: التشغيل عند تسجيل الدخول، وتحديثات موقّعة داخل التطبيق، وبطاقات مباشرة لحركة البيانات والذاكرة، وسمتان فاتحة وداكنة، و16 لغة بدعم كامل لاتجاه RTL (`ar`، `fa`).

## التثبيت

نزّل من [GitHub Releases](https://github.com/yilong-musk/ice-box/releases/latest):

| المنصة | المتطلبات | الحزمة | ملاحظات |
|---|---|---|---|
| **macOS** (Apple Silicon) | macOS 12+ | `ice-box_<version>_aarch64.dmg` | موقّع بتوقيع ad-hoc: اسمح به من **الخصوصية والأمان ← الفتح على أي حال**، أو شغّل `xattr -dr com.apple.quarantine /Applications/ice-box.app`. |
| **Windows** (x64) | Windows 10+ | `ice-box_<version>_x64-setup.exe` | مثبّت NSIS للمستخدم الحالي (غير موقّع؛ أكّد SmartScreen). يدعم TUN بروتوكول IPv4 TCP فقط ويتطلب حساب مسؤول (Administrator). |
| **Linux** (x64 / arm64) | Ubuntu 22.04+ | `.AppImage` / `.deb` | شغّل `chmod +x *.AppImage` أو `sudo apt install ./ice-box_*.deb`. وتحتاج أيقونة شريط النظام في GNOME إلى `gnome-shell-extension-appindicator`. |
| **Android** (arm64) | Android 14+ | `ice-box_<version>_android_arm64.apk` | موقّع بمفتاح المشروع (keystore)؛ اسمح بالتثبيت من هذا المصدر. |

- **البدء السريع**: استورد اشتراكًا (**الاشتراكات**) ← اختر الوضع والعقدة (**الرئيسية** / **العُقد**) ← اضغط زر التشغيل. يعمل بوضع الاتصال المباشر دون اشتراك.
- **التحديثات**: تحديثات موقّعة داخل التطبيق في **الإعدادات ← تحديثات التطبيق** (حزمة `.deb` تفتح صفحة Releases لاستخدام `apt`؛ ويُنزّل Android ملف APK لتثبيته عبر النظام).

## التطوير

يتطلب Rust (`rust-toolchain.toml`) وNode.js 22 وXcode CLT (على macOS) أو مكتبات تطوير GTK/WebKit (`libwebkit2gtk-4.1-dev` و`libgtk-3-dev` و`libayatana-appindicator3-dev` و`librsvg2-dev` و`libssl-dev`) على Linux. ويضيف Android كلًا من JDK 17 وGo و`ANDROID_HOME` (`scripts/ci-install-android-sdk.sh`).

```bash
cd apps/desktop && npm install && cd ../..
npm run fetch-singbox
npm run dev
```

| الأمر | الغرض |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` / `dev:linux-x64` / `dev:linux-arm64` | التشغيل على هدف مضيف محدد |
| `npm run gate` | بوابة مساحة العمل الكاملة (fmt وclippy واختبارات Rust والواجهة والبناء واختبارات smoke) |
| `npm run fetch-singbox && npm run build` | بناء حزمة `.dmg` لنظام macOS |
| `npm run fetch-singbox -- win && npm run build:win` | بناء مثبّت Windows |
| `npm run fetch-singbox -- linux-x64 && npm run build:linux` | بناء حزمتَي `.deb` و`.AppImage` لنظام Linux (`linux-arm64` على arm64) |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` / `acceptance:linux` / `acceptance:linux-tun` | بوابات القبول الحية على جهاز حقيقي |
| `bash scripts/build-android-apk.sh debug arm64` | بناء ملف Android APK تصحيحي (`amd64` للمحاكي) |

```text
apps/desktop    واجهة React + غلاف Tauri لسطح المكتب (macOS وWindows وLinux)
apps/mobile     غلاف Tauri لنظام Android (يعيد استخدام واجهة سطح المكتب)
apps/website    موقع GitHub Pages والعرض التفاعلي
crates/         مساحة عمل Rust (ice-app, ice-config, ice-core, ice-engine, ice-subscription, ice-proxy-sys, ice-tun-sys, ice-elevate, ice-helper, ice-types, ...)
```

## التوثيق

| المستند | النطاق |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | البنية المعمارية وحدود الوحدات وتدفق البيانات |
| [`docs/mcp.md`](docs/mcp.md) | إعداد MCP لسطح المكتب والأدوات ونموذج الأمان |
| [`docs/tun.md`](docs/tun.md) | التقاط TUN ورفع الصلاحيات والاسترداد وقيود المنصات |
| [`docs/testing.md`](docs/testing.md) | البوابات المحلية وبوابات CI والاختبارات اليدوية الحية |
| [`docs/release-process.md`](docs/release-process.md) | ترقيم الإصدارات والتوقيع والتحزيم والنشر |
| [`CHANGELOG.md`](CHANGELOG.md) | سجل الإصدارات |

المستندات التقنية أعلاه متوفرة حاليًا باللغة الإنجليزية فقط.

## الرخصة

مرخّص بموجب [GPL-3.0-or-later](LICENSE). نواة sing-box المدمجة: [GPL-3.0-or-later](third_party/sing-box/LICENSE) مع قيد التسمية الخاص بالمشروع الأصلي. إشعارات الأطراف الثالثة: [NOTICE](NOTICE).

<div align="center">
<br>
<sub>مبني باستخدام Tauri وReact وsing-box.</sub>
</div>
