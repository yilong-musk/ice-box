<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/brand/switch-logo/assets/logo.gif" alt="ice-box logo" width="144">
</a>

# ice-box

**Найкращий sing-box GUI.**

Легкий проксі-клієнт для macOS, Windows, Linux і Android.<br>
Простий у налаштуванні, швидкий у запуску, тихий у роботі й дружній до агентів — на основі ядра [sing-box](https://github.com/SagerNet/sing-box).

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-007ec6?style=flat-square)](#встановлення)
[![Core](https://img.shields.io/badge/sing--box-1.14.4-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**Завантажити**](https://github.com/yilong-musk/ice-box/releases/latest) · [**Живе демо**](https://yilong-musk.github.io/ice-box/) · [Встановлення](#встановлення) · [TUN](#режим-tun) · [Документація](#документація) · [Журнал змін](CHANGELOG.md)

[Bahasa Indonesia](README.id.md) · [Deutsch](README.de.md) · [English](README.md) · [Español](README.es.md) · [Français](README.fr.md) · [Português (BR)](README.pt-BR.md) · [Tiếng Việt](README.vi.md) · [Türkçe](README.tr.md)<br>[Русский](README.ru.md) · **Українська** · [العربية](README.ar.md) · [فارسی](README.fa.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [简体中文](README.zh-CN.md) · [繁體中文](README.zh-TW.md)

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/hero.png" alt="ice-box на десктопі та Android: стан проксі, перемикачі режимів, вузол виходу, графік трафіку в реальному часі та мобільна Головна зі списком вузлів" width="880">
</a>

<sub><a href="https://yilong-musk.github.io/ice-box/">Живе демо</a> запускає інтерфейси для десктопа та Android у вашому браузері з імітованим бекендом.</sub>

</div>

## Можливості

- **Підписки.** sing-box JSON, Clash і списки посилань обміну розпізнаються автоматично; також можна імпортувати локальний файл конфігурації sing-box. Десктоп може автоматично оновлювати профілі; Android оновлює їх під час імпорту або вручну. На кожній підписці відображаються трафік і термін дії від провайдера.
- **Маршрутизація.** Режими **Правила**, **Глобальний** і **Напряму**, окремий перемикач для кожного правила підписки та власні правила поверх них.
- **Вузли.** Обирайте вихід для кожної групи стратегій, перевіряйте затримку одного вузла чи всього списку та закріплюйте вибрані.
- **З'єднання.** На десктопі (macOS, Windows і Linux) спостерігайте за кожним активним з'єднанням: куди воно спрямоване, яке правило спрацювало, який вихід використовується та скільки даних передано. Шукайте, сортуйте та закривайте одне або всі з'єднання.
- **Перехоплення.** Системний проксі або TUN на десктопі; системний VPN на Android із маршрутизацією для кожної програми (за правилами, проксі, напряму або блокувати).
- **Трей.** Запускайте й зупиняйте проксі-службу та перемикайте режим, підписку або вузол виходу, не відкриваючи вікно. На macOS елемент у рядку меню також показує трафік у реальному часі, а з меню можна запускати тести затримки.
- **Доступ для агентів.** Опціональна локальна служба MCP дозволяє агенту керувати тією самою запущеною сесією.
- **Спокійна робота.** Запуск під час входу, оновлення в застосунку з перевіркою підпису, показники трафіку та пам'яті в реальному часі на Головній, 16 мов із повною підтримкою RTL (Bahasa Indonesia, Deutsch, English, Español, Français, Português (BR), Tiếng Việt, Türkçe, Русский, Українська, العربية, فارسی, 日本語, 한국어, 简体中文, 繁體中文), світла й темна теми.

## Встановлення

Завантажте інсталятор для вашої платформи з [останнього релізу](https://github.com/yilong-musk/ice-box/releases/latest).

| Платформа | Вимоги | Інсталятор | Перший запуск |
|---|---|---|---|
| **macOS** (Apple Silicon) | macOS 12 або новіша | `ice-box_<version>_aarch64.dmg` | Має лише ad-hoc підпис (без Developer ID і нотаризації), тому під час першого запуску з'являється попередження про **невідомого розробника**. Дозвольте запуск у **Системні параметри → Приватність і безпека → Усе одно відкрити** (macOS 15+), через контекстне меню **Відкрити** (старіші версії) або виконайте `xattr -dr com.apple.quarantine /Applications/ice-box.app`. |
| **Windows** (x64) | Windows 10 або новіша | `ice-box_<version>_x64-setup.exe` | Інсталятор NSIS для поточного користувача; права адміністратора не потрібні. Підпис Authenticode відсутній, тому SmartScreen може попросити підтвердження. |
| **Linux** (x64 / arm64) | Ubuntu 22.04+ / сучасні дистрибутиви | `ice-box_<version>_amd64.AppImage` / `aarch64.AppImage`<br>`ice-box_<version>_amd64.deb` / `arm64.deb` | Зробіть AppImage виконуваним (`chmod +x`) або встановіть `.deb` командою `sudo apt install ./ice-box_*.deb`. Для іконки в треї GNOME потрібне розширення AppIndicator (`gnome-shell-extension-appindicator`). У середовищі Wayland + NVIDIA ice-box автоматично задає `WEBKIT_DISABLE_DMABUF_RENDERER=1`, якщо змінну ще не налаштовано. |
| **Android** (arm64) | Android 14 або новіша | `ice-box_<version>_android_arm64.apk` | Підписаний ключем проєкту (keystore). Застосунку немає в Google Play; Android запитає дозвіл на встановлення з цього джерела. |

**Оновлення.** На macOS, Windows (0.1.5+) і в Linux AppImage застосунок ice-box у фоні перевіряє GitHub Releases і встановлює оновлення з перевіркою підпису через **Параметри → Оновлення застосунку**; старіші інсталяції macOS/Windows спершу потребують одного ручного оновлення. Інсталяції Linux `.deb` перевіряють той самий каталог і відкривають сторінку завантаження релізу, щоб оновлення залишалися під керуванням `apt` / `dpkg`. На Android розділ **Параметри → Оновлення застосунку** завантажує arm64 APK і передає його системному інсталятору для підтвердження.

## Швидкий старт

1. **Імпортуйте підписку.** Відкрийте **Підписки**, вставте URL і натисніть **Імпортувати** — формат визначиться автоматично. Щоб натомість використати локальний файл конфігурації sing-box (JSON), оберіть **Файл конфігурації**; він не має віддаленого URL, тому його не можна оновлювати.
2. **Оберіть режим і вузол.** На екрані **Головна** оберіть **Правила**, **Глобальний** або **Напряму**. На десктопі відкрийте **Вузли**, щоб обрати вихід і перевірити його затримку; на Android скористайтеся списком вузлів на екрані **Головна**.
3. **Запустіть проксі-службу.** Натисніть кнопку живлення. Десктоп застосовує системний проксі або запускає TUN, якщо його увімкнено; Android підключає VPN після системного запиту дозволу.

Ще немає підписки? ice-box запускається в режимі лише прямого підключення, тож ядро, перехоплення трафіку та інтерфейс можна випробувати ще до імпорту.

## Запуск під час входу

На macOS, Windows і Linux увімкніть **Параметри → Автозапуск → Запускати під час входу**. Запуск під час входу залишає застосунок у треї з прихованим вікном, запускає ядро та відновлює попередній стан проксі.

Вимикайте цю функцію тим самим перемикачем: вимкнення запису в **Елементи входу** на macOS не очищає налаштування. Перед увімкненням на macOS встановіть застосунок, оскільки запуск безпосередньо з образу диска відхиляється. У Linux запис створюється в `~/.config/autostart/com.yilong-musk.icebox.desktop` (під час запуску з AppImage зберігається `$APPIMAGE`).

На Android немає елементів входу. Розділ **Параметри → Система** пояснює виняток з оптимізації батареї, суворий Приватний DNS і постійний VPN.

## Режим TUN

На macOS, Windows і Linux TUN перехоплює трафік на мережевому рівні, охоплюючи програми, які не зчитують системний проксі. TUN у Windows підтримує лише IPv4 TCP (без UDP/QUIC та IPv6) і потребує облікового запису адміністратора; системний проксі залишається там альтернативою. У Linux TUN використовує Polkit (`pkexec`) для встановлення systemd-демона `ice-helper` і керує DNS через `systemd-resolved` (`resolvectl`) або статичний `/etc/resolv.conf`. Налаштування, поведінку перемикача, підтримку платформ та обмеження описано в [`docs/tun.md`](docs/tun.md). Android використовує системний VPN — див. [`docs/architecture.md`](docs/architecture.md#clients).

## Доступ для агентів через MCP

Десктопний застосунок (macOS, Windows і Linux) надає опціональну локальну службу MCP. Відкрийте розділ **MCP** між **З'єднання** та **Параметри**, увімкніть службу та скопіюйте згенеровану конфігурацію клієнта. Агент використовує ті самі параметри, підписки, дані, ядро та стан проксі запущеної сесії замість запуску окремого середовища виконання.

- **Повний набір інструментів.** Стан, параметри, трафік, підписки, вузли й групи, правила, активні з'єднання, журнали, діагностика, керування проксі та захищене відновлення TUN. Результати містять структурований вміст (коли доступно) та зберігають коди помилок застосунку.
- **Безпечно за замовчуванням.** MCP слухає лише автентифікований loopback із випадковим токеном керування, який ніколи не включається до скопійованої конфігурації клієнта.
- **Вбудований stdio-міст.** Згенерована конфігурація запускає той самий виконуваний файл у режимі моста, тому окремий демон або адаптер протоколу не потрібен.

MCP доступний лише на десктопі: на Android MCP немає, а вебдемо використовує імітовану службу. Налаштування, життєвий цикл і межі безпеки описано в [`docs/mcp.md`](docs/mcp.md).

## Розробка

Rust (версія зафіксована в `rust-toolchain.toml`, rustup встановить її під час першого використання), Node.js 22, Xcode Command Line Tools на macOS та пакети розробки GTK/WebKit (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libssl-dev`) на Linux. Для збірки Android також потрібні JDK 17, Go та Android SDK (`ANDROID_HOME`) з пакетами, які встановлює `scripts/ci-install-android-sdk.sh`.

```bash
cd apps/desktop && npm install && cd ../..
npm run fetch-singbox   # завантажити зафіксоване ядро sing-box для цього хоста
npm run dev
```

| Команда | Призначення |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` / `dev:linux-x64` / `dev:linux-arm64` | Запуск для конкретної цільової платформи |
| `npm run gate` | Повна перевірка робочого простору: fmt, clippy, тести Rust, перевірка типів, збірки, smoke-тести, fixtures |
| `npm run fetch-singbox && npm run build` | Збірка `.dmg` для macOS |
| `npm run fetch-singbox -- win && npm run build:win` | Збірка інсталятора NSIS для Windows |
| `npm run fetch-singbox -- linux-x64 && npm run build:linux` | Збірка `.deb` та `.AppImage` для Linux (передайте `linux-arm64` на arm64) |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` / `acceptance:linux` / `acceptance:linux-tun` | Приймальні перевірки на реальному хості |
| `bash scripts/build-android-apk.sh debug arm64` | Налагоджувальний APK для пристрою arm64 |
| `bash scripts/build-android-apk.sh debug amd64` | Налагоджувальний APK для емулятора |

```
apps/desktop    React UI + оболонка Tauri для macOS, Windows і Linux
apps/mobile     Оболонка Android; повторно використовує десктопний інтерфейс
apps/website    Сайт на GitHub Pages і живе демо
crates/         Робочий простір Rust (основні крейти): ice-app, ice-config, ice-core, ice-engine, ice-subscription, ice-proxy-sys, ice-tun-sys, ice-elevate, ice-helper, ice-types, ...
```

## Документація

| Документ | Зміст |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | Структура системи, зони відповідальності компонентів, потоки даних і проєктні межі |
| [`docs/mcp.md`](docs/mcp.md) | Налаштування MCP на десктопі, модель спільного середовища виконання, інструменти та безпека |
| [`docs/tun.md`](docs/tun.md) | Поведінка TUN, відновлення, налаштування для платформ та обмеження |
| [`docs/testing.md`](docs/testing.md) | Покриття локальних і CI-перевірок, ручні живі тести |
| [`docs/release-process.md`](docs/release-process.md) | Версіонування, підписування, пакування та публікація |
| [`CHANGELOG.md`](CHANGELOG.md) | Історія релізів |

Наведені вище технічні документи наразі доступні лише англійською мовою.

## Ліцензія

ice-box — це вільне програмне забезпечення, випущене під ліцензією [GNU General Public License v3.0 або новішою](LICENSE). Вбудоване ядро sing-box також має ліцензію [GPL-3.0-or-later](third_party/sing-box/LICENSE) з upstream-обмеженням щодо назви; усі повідомлення щодо сторонніх компонентів дивіться у файлі [NOTICE](NOTICE).

<div align="center">
<br>
<sub>Створено за допомогою Tauri, React і sing-box.</sub>
</div>
