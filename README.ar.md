<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/brand/switch-logo/assets/logo.gif" alt="ice-box logo" width="144">
</a>

# ice-box

**أفضل sing-box GUI.**

عميل بروكسي خفيف لأنظمة macOS وWindows وLinux وAndroid.<br>
سهل الإعداد، سريع التشغيل، هادئ أثناء العمل، وصديق للوكلاء — مبني على نواة [sing-box](https://github.com/SagerNet/sing-box).

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-007ec6?style=flat-square)](#التثبيت)
[![Core](https://img.shields.io/badge/sing--box-1.14.4-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**التنزيل**](https://github.com/yilong-musk/ice-box/releases/latest) · [**العرض التفاعلي**](https://yilong-musk.github.io/ice-box/) · [التثبيت](#التثبيت) · [TUN](#وضع-tun) · [التوثيق](#التوثيق) · [سجل التغييرات](CHANGELOG.md)

[Bahasa Indonesia](README.id.md) · [Deutsch](README.de.md) · [English](README.md) · [Español](README.es.md) · [Français](README.fr.md) · [Português (BR)](README.pt-BR.md) · [Tiếng Việt](README.vi.md) · [Türkçe](README.tr.md)<br>[Русский](README.ru.md) · [Українська](README.uk.md) · **العربية** · [فارسی](README.fa.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [简体中文](README.zh-CN.md) · [繁體中文](README.zh-TW.md)

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/hero.png" alt="تطبيق ice-box على سطح المكتب وAndroid: حالة البروكسي، تبديل الأوضاع، عقدة المخرج، منحنى حركة البيانات المباشر، وشاشة الرئيسية المصممة للهاتف مع قائمة العُقد" width="880">
</a>

<sub>يشغّل <a href="https://yilong-musk.github.io/ice-box/">العرض التفاعلي</a> واجهتي سطح المكتب وAndroid داخل المتصفح بالاعتماد على واجهة خلفية مُحاكاة.</sub>

</div>

## الميزات

- **الاشتراكات.** يتعرّف تلقائيًا على sing-box JSON وClash وقوائم روابط المشاركة، ويمكن استيراد ملف إعدادات sing-box محلي أيضًا. يدعم تطبيق سطح المكتب التحديث التلقائي للاشتراكات، بينما يحدّثها تطبيق Android عند الاستيراد أو التحديث اليدوي. وتُعرض بيانات استهلاك المزوّد وتاريخ الانتهاء على كل اشتراك.
- **التوجيه.** أوضاع **القواعد** و**شامل** و**مباشر**، ومفتاح تفعيل لكل قاعدة من قواعد الاشتراك، مع إمكانية إضافة قواعدك المخصصة فوقها.
- **العُقد.** اختر مخرجًا لكل مجموعة استراتيجية، واختبر زمن الاستجابة لعقدة واحدة أو للقائمة كاملةً دفعة واحدة، وثبّت عُقدك المفضلة.
- **الاتصالات.** على سطح المكتب (macOS وWindows وLinux)، راقب كل اتصال نشط: وجهته، القاعدة المطابقة، المخرج المستخدم، وحجم البيانات المنقولة. ابحث فيها ورتّبها وأغلق اتصالًا واحدًا أو جميع الاتصالات.
- **الالتقاط.** بروكسي النظام أو TUN على سطح المكتب، وVPN النظام على Android مع توجيه مخصص لكل تطبيق (اتباع القواعد، أو بروكسي، أو مباشر، أو حظر).
- **شريط النظام.** شغّل خدمة البروكسي وأوقفها وبدّل الوضع أو الاشتراك أو عقدة المخرج دون فتح النافذة. وعلى macOS يعرض عنصر شريط القوائم حركة البيانات الفورية أيضًا، وتتيح القائمة إجراء اختبارات زمن الاستجابة.
- **وصول الوكيل.** تتيح خدمة MCP محلية اختيارية للوكيل تشغيل الجلسة الجارية نفسها.
- **عناية هادئة.** التشغيل عند تسجيل الدخول، وتحديثات داخل التطبيق موثّقة بالتوقيع، وقراءات مباشرة لحركة البيانات والذاكرة في **الرئيسية**، و16 لغة بدعم كامل لاتجاه RTL (Bahasa Indonesia, Deutsch, English, Español, Français, Português (BR), Tiếng Việt, Türkçe, Русский, Українська, العربية, فارسی, 日本語, 한국어, 简体中文, 繁體中文)، وسمتان فاتحة وداكنة.

## التثبيت

نزّل حزمة التثبيت المناسبة لمنصتك من [أحدث إصدار](https://github.com/yilong-musk/ice-box/releases/latest).

| المنصة | المتطلبات | حزمة التثبيت | التشغيل الأول |
|---|---|---|---|
| **macOS** (Apple Silicon) | macOS 12 أو أحدث | `ice-box_<version>_aarch64.dmg` | موقّع بتوقيع ad-hoc فقط (دون Developer ID وغير موثّق لدى Apple)، لذا يعرض التشغيل الأول تحذير **مطوّر غير معروف**. اسمح به من **إعدادات النظام ← الخصوصية والأمان ← الفتح على أي حال** (macOS 15+)، أو بالنقر بزر الفأرة الأيمن ثم اختيار **فتح** (في الإصدارات الأقدم)، أو بتشغيل `xattr -dr com.apple.quarantine /Applications/ice-box.app`. |
| **Windows** (x64) | Windows 10 أو أحدث | `ice-box_<version>_x64-setup.exe` | مثبّت NSIS للمستخدم الحالي دون الحاجة إلى صلاحيات المسؤول. لا يحمل توقيع Authenticode، لذا قد يطلب منك SmartScreen التأكيد. |
| **Linux** (x64 / arm64) | Ubuntu 22.04+ والتوزيعات الحديثة | `ice-box_<version>_amd64.AppImage` / `aarch64.AppImage`<br>`ice-box_<version>_amd64.deb` / `arm64.deb` | اجعل ملف AppImage قابلًا للتنفيذ (`chmod +x`) أو ثبّت حزمة `.deb` عبر `sudo apt install ./ice-box_*.deb`. تحتاج أيقونة شريط النظام في GNOME إلى إضافة AppIndicator (`gnome-shell-extension-appindicator`). وعلى Wayland مع NVIDIA، يضبط ice-box المتغير `WEBKIT_DISABLE_DMABUF_RENDERER=1` تلقائيًا ما لم يكن مضبوطًا مسبقًا. |
| **Android** (arm64) | Android 14 أو أحدث | `ice-box_<version>_android_arm64.apk` | موقّع بمفتاح المشروع (keystore). لا يتوفر على Google Play، وسيطلب Android الإذن بالتثبيت من هذا المصدر. |

**التحديث.** على أنظمة macOS وWindows (الإصدار 0.1.5+) وتثبيتات Linux AppImage، يتحقق ice-box من GitHub Releases في الخلفية ويثبّت التحديثات الموثّقة بالتوقيع من **الإعدادات ← تحديثات التطبيق**؛ وتحتاج تثبيتات macOS وWindows الأقدم إلى ترقية يدوية واحدة أولًا. أما تثبيتات Linux `.deb` فتتحقق من دليل التحديثات نفسه وتفتح صفحة تنزيل الإصدار لتظل الترقيات مُدارة عبر `apt` / `dpkg`. وعلى Android، ينزّل قسم **الإعدادات ← تحديثات التطبيق** ملف arm64 APK ويسلّمه إلى مثبّت النظام للتأكيد.

## البدء السريع

1. **استيراد اشتراك.** افتح **الاشتراكات**، الصق الرابط، واضغط على **استيراد**؛ وسيُكتشف التنسيق تلقائيًا. ولاستخدام ملف إعدادات sing-box محلي (JSON) بدلًا من ذلك، اختر **ملف إعدادات**؛ وبما أنه لا يملك رابطًا بعيدًا فلا يمكن تحديثه.
2. **اختيار الوضع والعقدة.** في **الرئيسية**، اختر **القواعد** أو **شامل** أو **مباشر**. وعلى سطح المكتب افتح **العُقد** لاختيار مخرج واختبار زمن استجابته، بينما تستخدم على Android قائمة العُقد في **الرئيسية**.
3. **تشغيل خدمة البروكسي.** اضغط على زر التشغيل. يطبّق تطبيق سطح المكتب بروكسي النظام أو يشغّل TUN عند تفعيله، أما Android فيتصل بـ VPN بعد نافذة إذن النظام.

لا تملك اشتراكًا بعد؟ يبدأ ice-box بوضع الاتصال المباشر فقط، ما يتيح لك استكشاف النواة والالتقاط والواجهة قبل استيراد أي اشتراك.

## التشغيل عند تسجيل الدخول

على أنظمة macOS وWindows وLinux، فعّل **الإعدادات ← بدء التشغيل ← التشغيل عند تسجيل الدخول**. عند بدء التشغيل مع تسجيل الدخول يبقى التطبيق في شريط النظام مع إخفاء النافذة، ويشغّل النواة، ويستعيد آخر حالة للبروكسي.

أوقفه من المفتاح نفسه: فتعطيل الإدخال من **عناصر تسجيل الدخول** (Login Items) في macOS لا يمسحه. ثبّت التطبيق قبل تفعيل هذا الخيار على macOS، لأن التشغيل المباشر من صورة القرص مرفوض. وعلى Linux، يُكتب الإدخال في `~/.config/autostart/com.yilong-musk.icebox.desktop` (مع تسجيل `$APPIMAGE` عند التشغيل من AppImage).

لا يحتوي Android على عنصر تسجيل دخول. ويشرح قسم **الإعدادات ← النظام** استثناء تحسين البطارية، وإعداد DNS الخاص الصارم، وVPN دائم التشغيل.

## وضع TUN

على أنظمة macOS وWindows وLinux، يلتقط TUN حركة البيانات على مستوى الشبكة، ليشمل التطبيقات التي لا تقرأ بروكسي النظام أبدًا. ينقل TUN على Windows حركة IPv4 TCP فقط (دون UDP/QUIC أو IPv6) ويتطلب حساب مسؤول (Administrator)؛ ويظل بروكسي النظام هو البديل هناك. أما TUN على Linux فيستخدم Polkit (`pkexec`) لتثبيت خدمة systemd الخلفية `ice-helper` ويدير DNS عبر `systemd-resolved` (`resolvectl`) أو ملف `/etc/resolv.conf` ثابت. راجع [`docs/tun.md`](docs/tun.md) للاطلاع على الإعداد وسلوك التبديل وتغطية المنصات والقيود. ويستخدم Android خدمة VPN للنظام؛ راجع [`docs/architecture.md`](docs/architecture.md#clients).

## وصول الوكيل عبر MCP

يوفر تطبيق سطح المكتب (macOS وWindows وLinux) خدمة MCP محلية اختيارية. افتح **MCP** بين **الاتصالات** و**الإعدادات**، وفعّلها، ثم انسخ إعدادات العميل المُنشأة. يشارك الوكيل إعدادات الجلسة الجارية واشتراكاتها وبياناتها ونواتها وحالة البروكسي الخاصة بها بدلًا من تشغيل وقت تشغيل آخر.

- **مجموعة أدوات كاملة.** الحالة، والإعدادات، وحركة البيانات، والاشتراكات، والعُقد والمجموعات، والقواعد، والاتصالات النشطة، والسجلات، والتشخيص، والتحكم في البروكسي، واسترداد TUN المحمي. وتحمل النتائج محتوى منظمًا عند توفره وتحافظ على رموز أخطاء التطبيق.
- **آمن افتراضيًا.** تستمع خدمة MCP فقط على عنوان الاسترجاع المحلي (loopback) بمصادقة ورمز تحكم عشوائي لا يُضمَّن أبدًا في إعدادات العميل المنسوخة.
- **جسر stdio مدمج.** تشغّل الإعدادات المُنشأة الملف التنفيذي نفسه في وضع الجسر، فلا حاجة إلى خدمة خلفية منفصلة أو محوّل بروتوكول.

تتوفر خدمة MCP على سطح المكتب فقط: لا يحتوي Android على MCP، ويستخدم العرض التفاعلي على الموقع خدمة مُحاكاة. راجع [`docs/mcp.md`](docs/mcp.md) للاطلاع على الإعداد ودورة الحياة وحدود الأمان.

## التطوير

يتطلب التطوير لغة Rust (الإصدار المثبّت في `rust-toolchain.toml` والذي يثبّته rustup عند أول استخدام)، وNode.js 22، وXcode Command Line Tools على macOS، وحزم تطوير GTK/WebKit (`libwebkit2gtk-4.1-dev` و`libgtk-3-dev` و`libayatana-appindicator3-dev` و`librsvg2-dev` و`libssl-dev`) على Linux. كما تحتاج بناءات Android إلى JDK 17 وGo وAndroid SDK (`ANDROID_HOME`) مع الحزم التي يثبّتها السكربت `scripts/ci-install-android-sdk.sh`.

```bash
cd apps/desktop && npm install && cd ../..
# تنزيل نواة sing-box المثبّتة لهذا الجهاز
npm run fetch-singbox
npm run dev
```

| الأمر | الغرض |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` / `dev:linux-x64` / `dev:linux-arm64` | التشغيل على هدف مضيف محدد |
| `npm run gate` | بوابة مساحة العمل الكاملة: fmt وclippy واختبارات Rust وفحص الأنواع والبناء واختبارات smoke والـ fixtures |
| `npm run fetch-singbox && npm run build` | بناء حزمة `.dmg` لنظام macOS |
| `npm run fetch-singbox -- win && npm run build:win` | بناء مثبّت NSIS لنظام Windows |
| `npm run fetch-singbox -- linux-x64 && npm run build:linux` | بناء حزمتَي `.deb` و`.AppImage` لنظام Linux (مرّر `linux-arm64` على arm64) |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` / `acceptance:linux` / `acceptance:linux-tun` | بوابات القبول الحية على جهاز حقيقي |
| `bash scripts/build-android-apk.sh debug arm64` | ملف APK تصحيحي (debug) لجهاز arm64 |
| `bash scripts/build-android-apk.sh debug amd64` | ملف APK تصحيحي (debug) للمحاكي |

```
apps/desktop    واجهة React + غلاف Tauri لأنظمة macOS وWindows وLinux
apps/mobile     غلاف Android؛ يعيد استخدام واجهة سطح المكتب
apps/website    موقع GitHub Pages والعرض التفاعلي
crates/         مساحة عمل Rust (الحزم الرئيسية): ice-app, ice-config, ice-core, ice-engine, ice-subscription, ice-proxy-sys, ice-tun-sys, ice-elevate, ice-helper, ice-types, ...
```

## التوثيق

| المستند | ما ستجده فيه |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | بنية النظام ومسؤوليات المكوّنات وتدفق البيانات وحدود التصميم |
| [`docs/mcp.md`](docs/mcp.md) | إعداد MCP لسطح المكتب ونموذج وقت التشغيل المشترك والأدوات والأمان |
| [`docs/tun.md`](docs/tun.md) | سلوك TUN والاسترداد وإعداد المنصات والقيود |
| [`docs/testing.md`](docs/testing.md) | تغطية البوابات المحلية وبوابات CI والاختبارات اليدوية الحية |
| [`docs/release-process.md`](docs/release-process.md) | ترقيم الإصدارات والتوقيع والتحزيم والنشر |
| [`CHANGELOG.md`](CHANGELOG.md) | سجل الإصدارات |

المستندات المذكورة أعلاه متوفرة حاليًا باللغة الإنجليزية فقط.

## الرخصة

يُعد ice-box برنامجًا حرًا يصدر بموجب [رخصة جنو العمومية الإصدار 3.0 أو أحدث](LICENSE). كما تخضع نواة sing-box المدمجة أيضًا لرخصة [GPL-3.0-or-later](third_party/sing-box/LICENSE)، مع قيد التسمية الخاص بالمشروع الأصلي؛ راجع [NOTICE](NOTICE) للاطلاع على جميع إشعارات الأطراف الثالثة.

<div align="center">
<br>
<sub>مبني باستخدام Tauri وReact وsing-box.</sub>
</div>
