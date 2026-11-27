<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/brand/switch-logo/assets/logo.gif" alt="ice-box logo" width="144">
</a>

# ice-box

**Die beste sing-box GUI.**

Ein schlanker Proxy-Client für macOS, Windows, Linux und Android.<br>
Einfach einzurichten, schnell gestartet, ruhig im Betrieb und Agent-freundlich — aufgebaut auf einem [sing-box](https://github.com/SagerNet/sing-box)-Kern.

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-007ec6?style=flat-square)](#installation)
[![Core](https://img.shields.io/badge/sing--box-1.14.4-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**Download**](https://github.com/yilong-musk/ice-box/releases/latest) · [**Live-Demo**](https://yilong-musk.github.io/ice-box/) · [Installation](#installation) · [TUN](#tun-modus) · [Dokumentation](#dokumentation) · [Changelog](CHANGELOG.md)

[Bahasa Indonesia](README.id.md) · **Deutsch** · [English](README.md) · [Español](README.es.md) · [Français](README.fr.md) · [Português (BR)](README.pt-BR.md) · [Tiếng Việt](README.vi.md) · [Türkçe](README.tr.md)<br>[Русский](README.ru.md) · [Українська](README.uk.md) · [العربية](README.ar.md) · [فارسی](README.fa.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [简体中文](README.zh-CN.md) · [繁體中文](README.zh-TW.md)

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/hero.png" alt="ice-box auf Desktop und Android: Proxy-Status, Modus-Schalter, Ausgangsknoten, Live-Datenverkehrsdiagramm und die mobile Startseite mit Knotenliste" width="880">
</a>

<sub>Die <a href="https://yilong-musk.github.io/ice-box/">Live-Demo</a> führt die Desktop- und Android-Oberflächen im Browser gegen ein simuliertes Backend aus.</sub>

</div>

## Funktionen

- **Abonnements.** sing-box JSON, Clash und Share-Link-Listen werden automatisch erkannt; auch eine lokale sing-box-Konfigurationsdatei lässt sich importieren. Auf dem Desktop können Profile automatisch aktualisiert werden; Android aktualisiert beim Import oder per manuellem Neu-Laden. Datenvolumen und Ablaufdatum des Anbieters werden bei jedem Abonnement angezeigt.
- **Routing.** Die Modi **Regeln**, **Global** und **Direkt**, ein Schalter für jede Abonnement-Regel sowie eigene Regeln darüber.
- **Knoten.** Wähle einen Ausgang je Strategiegruppe, teste die Latenz eines einzelnen Knotens oder einer ganzen Liste und hefte Favoriten an.
- **Verbindungen.** Sieh auf dem Desktop (macOS, Windows und Linux) jede aktive Verbindung: Ziel, passende Regel, genutzter Ausgang und übertragene Datenmenge. Suchen, sortieren und einzelne oder alle Verbindungen schließen.
- **Erfassung.** Systemproxy oder TUN auf dem Desktop; System-VPN unter Android mit App-spezifischem Routing (Regeln folgen, Proxy, Direkt oder Blockieren).
- **Tray.** Starte und stoppe den Proxy-Dienst und wechsle Modus, Abonnement oder Ausgangsknoten, ohne das Fenster zu öffnen. Unter macOS zeigt das Menüleisten-Symbol zudem den Live-Datenverkehr und das Menü führt Latenztests aus.
- **Agent-Zugriff.** Ein optionaler lokaler MCP-Dienst ermöglicht es einem Agenten, dieselbe laufende Sitzung zu steuern.
- **Sorgenfrei.** Bei Anmeldung starten, signaturgeprüfte In-App-Updates, Live-Datenverkehr und Speicheranzeige unter Start, 16 Sprachen mit voller RTL-Unterstützung (Bahasa Indonesia, Deutsch, English, Español, Français, Português (BR), Tiếng Việt, Türkçe, Русский, Українська, العربية, فارسی, 日本語, 한국어, 简体中文, 繁體中文) sowie helles und dunkles Design.

## Installation

Lade den passenden Installer für deine Plattform aus dem [neuesten Release](https://github.com/yilong-musk/ice-box/releases/latest) herunter.

| Plattform | Voraussetzungen | Installer | Erster Start |
|---|---|---|---|
| **macOS** (Apple Silicon) | macOS 12 oder neuer | `ice-box_<version>_aarch64.dmg` | Nur Ad-hoc-signiert (keine Developer ID, nicht notarisiert), daher erscheint beim ersten Start eine Warnung wegen eines **nicht verifizierten Entwicklers**. Erlaube die App unter **Systemeinstellungen → Datenschutz & Sicherheit → Dennoch öffnen** (macOS 15+), per Rechtsklick → **Öffnen** (ältere Versionen) oder mit `xattr -dr com.apple.quarantine /Applications/ice-box.app`. |
| **Windows** (x64) | Windows 10 oder neuer | `ice-box_<version>_x64-setup.exe` | Benutzerbezogener NSIS-Installer; keine Administratorrechte erforderlich. Ohne Authenticode-Signatur bittet SmartScreen eventuell um Bestätigung. |
| **Linux** (x64 / arm64) | Ubuntu 22.04+ / moderne Distributionen | `ice-box_<version>_amd64.AppImage` / `aarch64.AppImage`<br>`ice-box_<version>_amd64.deb` / `arm64.deb` | Mache das AppImage ausführbar (`chmod +x`) oder installiere das `.deb`-Paket mit `sudo apt install ./ice-box_*.deb`. Das GNOME-Tray-Symbol benötigt die AppIndicator-Erweiterung (`gnome-shell-extension-appindicator`). Unter Wayland + NVIDIA setzt ice-box automatisch `WEBKIT_DISABLE_DMABUF_RENDERER=1`, sofern noch nicht konfiguriert. |
| **Android** (arm64) | Android 14 oder neuer | `ice-box_<version>_android_arm64.apk` | Mit dem Projekt-Keystore signiert. Nicht bei Google Play gelistet; Android bittet um Erlaubnis für Installationen aus dieser Quelle. |

**Updates.** Unter macOS, Windows (0.1.5+) und bei Linux-AppImage-Installationen prüft ice-box im Hintergrund GitHub Releases und installiert signaturgeprüfte Updates unter **Einstellungen → App-Updates**; ältere macOS-/Windows-Installationen benötigen zunächst einmalig ein manuelles Upgrade. Linux-`.deb`-Installationen prüfen denselben Katalog und öffnen die Release-Downloadseite, damit Upgrades weiterhin über `apt` / `dpkg` verwaltet werden. Unter Android lädt **Einstellungen → App-Updates** die arm64-APK herunter und übergibt sie zur Bestätigung an den System-Installer.

## Schnellstart

1. **Abonnement importieren.** Öffne **Abonnements**, füge die URL ein und klicke auf **Importieren**; das Format wird automatisch erkannt. Wenn du stattdessen eine lokale sing-box-Konfigurationsdatei (JSON) verwenden möchtest, wähle **Konfigurationsdatei**; sie hat keine Remote-URL und kann daher nicht aktualisiert werden.
2. **Modus und Knoten wählen.** Wähle unter **Start** zwischen **Regeln**, **Global** und **Direkt**. Öffne auf dem Desktop **Knoten**, um einen Ausgang zu wählen und seine Latenz zu testen; unter Android nutze die Knotenliste unter **Start**.
3. **Proxy-Dienst starten.** Drücke die Ein-/Aus-Schaltfläche. Auf dem Desktop wird der Systemproxy angewendet oder TUN gestartet, wenn aktiviert; unter Android verbindet sich das VPN nach der Systemberechtigungsabfrage.

Noch kein Abonnement? ice-box startet im reinen Direktmodus, sodass Kern, Erfassung und Oberfläche schon vor jedem Import ausprobiert werden können.

## Bei Anmeldung starten

Aktiviere unter macOS, Windows und Linux **Einstellungen → Autostart → Bei Anmeldung starten**. Beim Start nach der Anmeldung bleibt die App mit ausgeblendetem Fenster im Tray, startet den Kern und stellt den letzten Proxy-Status wieder her.

Deaktiviere die Funktion über denselben Schalter: Das Deaktivieren des Eintrags unter **Anmeldeobjekte** in macOS setzt diese Einstellung nicht zurück. Installiere die App unter macOS vor dem Aktivieren, da ein Start direkt aus dem Disk-Image abgelehnt wird. Unter Linux wird der Eintrag nach `~/.config/autostart/com.yilong-musk.icebox.desktop` geschrieben (bei Ausführung als AppImage wird `$APPIMAGE` eingetragen).

Android hat kein Anmeldeobjekt. **Einstellungen → System** erläutert die Ausnahme von der Akkuoptimierung, strenges Privates DNS und dauerhaftes VPN.

## TUN-Modus

Unter macOS, Windows und Linux erfasst TUN den Datenverkehr auf der Netzwerkschicht und deckt damit auch Apps ab, die den Systemproxy ignorieren. Windows-TUN überträgt nur IPv4-TCP (kein UDP/QUIC und kein IPv6) und erfordert ein Administratorkonto; der Systemproxy bleibt dort die Alternative. Linux-TUN nutzt Polkit (`pkexec`), um einen systemd-Hilfsdienst `ice-helper` zu installieren, und verwaltet DNS über `systemd-resolved` (`resolvectl`) oder eine statische `/etc/resolv.conf`. Einrichtung, Umschaltverhalten, Plattformabdeckung und Einschränkungen stehen in [`docs/tun.md`](docs/tun.md). Android nutzt das System-VPN; siehe [`docs/architecture.md`](docs/architecture.md#clients).

## Agent-Zugriff über MCP

Die Desktop-App (macOS, Windows und Linux) stellt einen optionalen lokalen MCP-Dienst bereit. Öffne **MCP** zwischen **Verbindungen** und **Einstellungen**, aktiviere den Dienst und kopiere die erzeugte Client-Konfiguration. Der Agent teilt Einstellungen, Abonnements, Daten, Kern und Proxy-Status der laufenden Sitzung, anstatt eine weitere Laufzeit zu starten.

- **Vollständiger Werkzeugsatz.** Status, Einstellungen, Datenverkehr, Abonnements, Knoten und Gruppen, Regeln, aktive Verbindungen, Protokolle, Diagnose, Proxy-Steuerung und geschützte TUN-Wiederherstellung. Ergebnisse liefern, wo verfügbar, strukturierte Inhalte und bewahren die Anwendungs-Fehlercodes.
- **Standardmäßig sicher.** MCP lauscht ausschließlich auf authentifiziertem Loopback mit einem zufälligen Kontroll-Token, das niemals in der kopierten Client-Konfiguration enthalten ist.
- **Integrierte stdio-Bridge.** Die erzeugte Konfiguration startet dieselbe ausführbare Datei im Bridge-Modus, sodass kein separater Daemon oder Protokolladapter benötigt wird.

MCP steht nur auf dem Desktop zur Verfügung: Android hat kein MCP, und die Website-Demo nutzt einen simulierten Dienst. Siehe [`docs/mcp.md`](docs/mcp.md) für Einrichtung, Lebenszyklus und Sicherheitsgrenzen.

## Entwicklung

Rust (die in `rust-toolchain.toml` festgelegte Version, die rustup beim ersten Einsatz installiert), Node.js 22, Xcode Command Line Tools unter macOS sowie GTK/WebKit-Entwicklungspakete (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libssl-dev`) unter Linux. Android-Builds benötigen zusätzlich JDK 17, Go und ein Android SDK (`ANDROID_HOME`) mit den über `scripts/ci-install-android-sdk.sh` installierten Paketen.

```bash
cd apps/desktop && npm install && cd ../..
npm run fetch-singbox   # festgelegten sing-box-Kern für diesen Host herunterladen
npm run dev
```

| Befehl | Zweck |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` / `dev:linux-x64` / `dev:linux-arm64` | Gegen ein bestimmtes Host-Ziel ausführen |
| `npm run gate` | Vollständiges Workspace-Gate: fmt, clippy, Rust-Tests, Typprüfungen, Builds, Smoke-Tests, Fixtures |
| `npm run fetch-singbox && npm run build` | macOS-`.dmg` bauen |
| `npm run fetch-singbox -- win && npm run build:win` | Windows-NSIS-Installer bauen |
| `npm run fetch-singbox -- linux-x64 && npm run build:linux` | Linux-`.deb` und `.AppImage` bauen (auf arm64 `linux-arm64` übergeben) |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` / `acceptance:linux` / `acceptance:linux-tun` | Live-Akzeptanzgates auf einem echten Host |
| `bash scripts/build-android-apk.sh debug arm64` | Debug-APK für ein arm64-Gerät |
| `bash scripts/build-android-apk.sh debug amd64` | Debug-APK für den Emulator |

```
apps/desktop    React-Oberfläche + Tauri-Shell für macOS, Windows und Linux
apps/mobile     Android-Shell; verwendet die Desktop-Oberfläche weiter
apps/website    GitHub-Pages-Website und Live-Demo
crates/         Rust-Workspace (Haupt-Crates): ice-app, ice-config, ice-core, ice-engine, ice-subscription, ice-proxy-sys, ice-tun-sys, ice-elevate, ice-helper, ice-types, ...
```

## Dokumentation

| Dokument | Inhalt |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | Systemstruktur, Zuständigkeiten der Komponenten, Datenfluss und Architekturgrenzen |
| [`docs/mcp.md`](docs/mcp.md) | Desktop-MCP-Einrichtung, Shared-Runtime-Modell, Werkzeuge und Sicherheit |
| [`docs/tun.md`](docs/tun.md) | TUN-Verhalten, Wiederherstellung, Plattformkonfiguration und Einschränkungen |
| [`docs/testing.md`](docs/testing.md) | Lokale und CI-Gate-Abdeckung, manuelle Live-Tests |
| [`docs/release-process.md`](docs/release-process.md) | Versionierung, Signierung, Paketierung und Veröffentlichung |
| [`CHANGELOG.md`](CHANGELOG.md) | Versionshistorie |

Die oben aufgeführten Dokumente liegen derzeit auf Englisch vor.

## Lizenz

ice-box ist freie Software und erscheint unter der [GNU General Public License v3.0 oder neuer](LICENSE). Der gebündelte sing-box-Kern steht ebenfalls unter [GPL-3.0-or-later](third_party/sing-box/LICENSE) samt der zugehörigen Namensbeschränkung des Upstream-Projekts; alle Drittanbieterhinweise finden sich in [NOTICE](NOTICE).

<div align="center">
<br>
<sub>Gebaut mit Tauri, React und sing-box.</sub>
</div>
