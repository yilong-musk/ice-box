// SPDX-License-Identifier: GPL-3.0-or-later

import type { MarketingKey } from "./zh";

export const ru: Record<MarketingKey, string> = {
  "head.title": "ice-box — лучший GUI для sing-box",
  "head.description":
    "ice-box — лёгкий прокси-клиент для macOS, Windows, Linux и Android: простая настройка, быстрый запуск, низкое потребление ресурсов и поддержка агентов через MCP.",
  "nav.languageAria": "Выбрать язык",
  "nav.themeToLight": "Переключить на светлую тему",
  "nav.themeToDark": "Переключить на тёмную тему",
  "nav.features": "Возможности",
  "nav.agent": "Агент",
  "nav.download": "Скачать",
  "hero.release": "Вышла версия {version}",
  "hero.title": "Лучший <span class=\"accent\">sing-box GUI</span>",
  "hero.subtitle":
    "Лёгкий прокси-клиент для macOS, Windows, Linux и Android. Импортируйте подписку, выберите узел и включите прокси в один клик. Работает тихо в фоне и поддерживает управление через MCP.",
  "hero.installer": "Скачать бесплатно",
  "hero.installerPlatform": "Скачать для {platform}",
  "hero.tryDemo": "Попробовать демо",
  "hero.platforms": "macOS · Windows · Linux · Android",
  "demo.windowBar": "интерактивное демо",
  "demo.iframeTitle": "Интерактивное демо настольного приложения ice-box",
  "features.kicker": "Возможности",
  "features.title": "Всё нужное под рукой",
  "features.body":
    "От импорта подписки до проверки подключений — повседневные действия занимают один-два клика.",
  "features.groupDaily": "На каждый день",
  "features.groupBackground": "В фоновом режиме",
  "features.subs.title": "Подписки",
  "features.subs.body":
    "Автоматически распознаёт sing-box JSON, Clash и ссылки, а также локальные конфиги. На ПК подписки обновляются сами, показывая остаток трафика и срок действия.",
  "features.routing.title": "Маршрутизация",
  "features.routing.body":
    "Мгновенное переключение между режимами «Правила», «Глобальный» и «Напрямую». Отключайте отдельные правила подписки и добавляйте свои поверх.",
  "features.nodes.title": "Узлы",
  "features.nodes.body":
    "Выбирайте выход для каждой группы, проверяйте задержку одного узла или всего списка и закрепляйте избранные.",
  "features.connections.title": "Подключения",
  "features.connections.body":
    "На ПК (macOS, Windows, Linux) видно каждое активное подключение: адрес, сработавшее правило, выход и объём трафика. Поиск, сортировка и закрытие в один клик.",
  "features.capture.title": "Перехват трафика",
  "features.capture.body":
    "Системный прокси или TUN на ПК. На Android — системный VPN с раздельным маршрутизированием по приложениям: по правилам, прокси, напрямую или блокировка.",
  "features.tray.title": "Трей",
  "features.tray.body":
    "Запускайте и останавливайте прокси, меняйте режим, подписку и узел, не открывая окно. В строке меню macOS также отображается текущая скорость.",
  "features.upkeep.title": "Без лишних хлопот",
  "features.upkeep.body":
    "Автозапуск, обновления с проверкой подписи, трафик и память на Главной, 16 языков интерфейса (включая RTL), светлая и тёмная темы.",
  "agent.kicker": "Для AI-агентов",
  "agent.title": "Агент работает с вами в одной сессии",
  "agent.body":
    "В настольную версию встроен локальный сервис MCP. Включите его и передайте конфиг агенту: он сможет читать статус, менять режимы и узлы, управлять подписками, правилами и подключениями поверх тех же данных, что видите вы.",
  "agent.point1": "Без лишних фоновых процессов и копирования данных",
  "agent.point2": "Изменения ваши и агента видны друг другу мгновенно",
  "agent.point3": "Выключен по умолчанию, слушает только локальный компьютер",
  "agent.terminalTitle": "Сессия агента",
  "agent.prompt": "Переключи на глобальный режим и покажи самые активные подключения",
  "agent.reply":
    "Включён глобальный режим. Из 8 подключений больше всего трафика у googlevideo.com (YouTube) через Los Angeles / edge-03.",
  "phoneDemo.iframeTitle": "Интерактивное демо Android-приложения ice-box",
  "download.kicker": "Скачать",
  "download.title": "Выберите вашу платформу",
  "download.body": "Установщики размещены на GitHub Releases. Регистрация не требуется.",
  "download.tabListAria": "Выберите операционную систему",
  "download.actionPlatform": "Скачать для {platform}",
  "download.yours": "Это устройство",
  "download.allReleases": "Все релизы и список изменений",
  "download.macos.name": "macOS",
  "download.macos.kind": "Apple Silicon",
  "download.macos.requires": "macOS 12 или новее",
  "download.macos.note":
    "Если при первом запуске macOS сообщит о неподтверждённом разработчике, разрешите запуск в Системные настройки → Конфиденциальность и безопасность.",
  "download.windows.name": "Windows",
  "download.windows.kind": "64-бит",
  "download.windows.requires": "Windows 10 или новее",
  "download.windows.note":
    "Устанавливается для текущего пользователя без прав администратора. SmartScreen может запросить подтверждение.",
  "download.linux.name": "Linux",
  "download.linux.requires": "Ubuntu 22.04+ и современные дистрибутивы",
  "download.linux.note":
    "AppImage поддерживает подписанные обновления в приложении; .deb обновляется пакетным менеджером. Системный прокси поддерживает GNOME/KDE; для TUN нужны systemd и Polkit.",
  "download.linux.archAria": "Выберите архитектуру Linux",
  "download.linux.appimageAction": "Скачать AppImage",
  "download.linux.debAction": "Скачать .deb",
  "download.android.name": "Android",
  "download.android.kind": "arm64",
  "download.android.requires": "Android 14 или новее",
  "download.android.note":
    "Пока не опубликовано в магазинах приложений. При запросе Android разрешите установку из этого источника.",
  "footer.source": "Исходный код",
  "footer.releases": "Релизы",
  "footer.changelog": "Список изменений",
  "footer.license": "Лицензия GPL-3.0-or-later",
};
