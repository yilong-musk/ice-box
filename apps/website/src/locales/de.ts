// SPDX-License-Identifier: GPL-3.0-or-later

import type { MarketingKey } from "./zh";

export const de: Record<MarketingKey, string> = {
  "head.title": "ice-box — die beste sing-box GUI",
  "head.description":
    "ice-box ist ein schlanker Proxy-Client für macOS, Windows, Linux und Android: einfach einzurichten, schnell gestartet, sparsam im Betrieb und bereit für Agenten über MCP.",
  "nav.languageAria": "Sprache wählen",
  "nav.themeToLight": "Zum hellen Design wechseln",
  "nav.themeToDark": "Zum dunklen Design wechseln",
  "nav.features": "Funktionen",
  "nav.agent": "Agent",
  "nav.download": "Download",
  "hero.release": "{version} ist da",
  "hero.title": "Die beste <span class=\"accent\">sing-box GUI</span>",
  "hero.subtitle":
    "Ein schlanker Proxy-Client für macOS, Windows, Linux und Android. Abonnement importieren, Knoten wählen und einschalten. Läuft ruhig im Hintergrund und arbeitet über MCP direkt mit deinem Agenten zusammen.",
  "hero.installer": "Kostenlos laden",
  "hero.installerPlatform": "Für {platform} laden",
  "hero.tryDemo": "Live-Demo testen",
  "hero.platforms": "macOS · Windows · Linux · Android",
  "demo.windowBar": "Live-Produktdemo",
  "demo.iframeTitle": "Live-Demo der ice-box Desktop-App",
  "features.kicker": "Funktionen",
  "features.title": "Alles Wichtige direkt griffbereit",
  "features.body":
    "Vom Abonnement-Import bis zur Verbindungsanalyse ist jede tägliche Aufgabe nur ein bis zwei Klicks entfernt.",
  "features.groupDaily": "Im Alltag",
  "features.groupBackground": "Im Hintergrund",
  "features.subs.title": "Abonnements",
  "features.subs.body":
    "Erkennt sing-box JSON, Clash und Share-Links automatisch und importiert auch lokale Konfigurationen. Auf dem Desktop aktualisieren sich Abos selbstständig, inklusive Restvolumen und Ablaufdatum.",
  "features.routing.title": "Routing",
  "features.routing.body":
    "Wechsle jederzeit zwischen Regeln, Global und Direkt. Schalte einzelne Abonnement-Regeln gezielt um und lege eigene Regeln darüber.",
  "features.nodes.title": "Knoten",
  "features.nodes.body":
    "Wähle einen Ausgang je Strategiegruppe, teste die Latenz einzelner Knoten oder ganzer Listen und hefte Favoriten oben an.",
  "features.connections.title": "Verbindungen",
  "features.connections.body":
    "Sieh auf dem Desktop (macOS, Windows, Linux) jede aktive Verbindung: Ziel, passende Regel, Ausgang und Datenvolumen. Suchen, sortieren oder mit einem Klick schließen.",
  "features.capture.title": "Erfassung",
  "features.capture.body":
    "Nutze Systemproxy oder TUN auf dem Desktop. Unter Android läuft ice-box als System-VPN mit App-spezifischem Routing: Regeln folgen, Proxy, Direkt oder Blockieren.",
  "features.tray.title": "Tray",
  "features.tray.body":
    "Starte oder stoppe den Proxy und wechsle Modus, Abonnement oder Ausgang, ohne das Fenster zu öffnen. Unter macOS zeigt die Menüleiste auch den Live-Durchsatz.",
  "features.upkeep.title": "Sorgenfrei",
  "features.upkeep.body":
    "Autostart, signaturgeprüfte In-App-Updates, Live-Traffic und Speicher auf der Startseite, 16 Sprachen mit voller RTL-Unterstützung sowie helles und dunkles Design.",
  "agent.kicker": "Agent-freundlich",
  "agent.title": "Dein Agent arbeitet in derselben Sitzung wie du",
  "agent.body":
    "Die Desktop-App enthält einen optionalen lokalen MCP-Dienst. Aktiviere ihn und gib die Konfiguration deinem Agenten: Er liest den Status, wechselt Modi und Knoten, verwaltet Abonnements und Regeln und prüft Verbindungen auf genau denselben Daten, die du siehst.",
  "agent.point1": "Kein zusätzlicher Hintergrundprozess, kein doppeltes Datenverzeichnis",
  "agent.point2": "Deine Änderungen und die des Agenten sind sofort gegenseitig sichtbar",
  "agent.point3": "Standardmäßig aus und lauscht nur auf diesem Gerät",
  "agent.terminalTitle": "Agent-Sitzung",
  "agent.prompt": "Wechsle in den globalen Modus und zeige die aktivsten Verbindungen",
  "agent.reply":
    "In den globalen Modus gewechselt. Von 8 Verbindungen hat googlevideo.com (YouTube) den höchsten Verkehr über Los Angeles / edge-03.",
  "phoneDemo.iframeTitle": "Live-Demo der ice-box Android-App",
  "download.kicker": "Download",
  "download.title": "Wähle deine Plattform",
  "download.body": "Die Installer liegen direkt bei GitHub Releases. Keine Registrierung nötig.",
  "download.tabListAria": "Betriebssystem wählen",
  "download.actionPlatform": "Für {platform} laden",
  "download.yours": "Dieses Gerät",
  "download.allReleases": "Alle Releases und das Changelog ansehen",
  "download.macos.name": "macOS",
  "download.macos.kind": "Apple Silicon",
  "download.macos.requires": "macOS 12 oder neuer",
  "download.macos.note":
    "Wenn macOS beim ersten Start einen nicht verifizierten Entwickler meldet, erlaube die App unter Systemeinstellungen → Datenschutz & Sicherheit.",
  "download.windows.name": "Windows",
  "download.windows.kind": "64-Bit",
  "download.windows.requires": "Windows 10 oder neuer",
  "download.windows.note":
    "Wird für den aktuellen Benutzer ohne Administratorrechte installiert. SmartScreen bittet eventuell um Bestätigung.",
  "download.linux.name": "Linux",
  "download.linux.requires": "Ubuntu 22.04+ und moderne Distributionen",
  "download.linux.note":
    "AppImage bietet signierte In-App-Updates; .deb wird über die Paketverwaltung aktualisiert. Systemproxy unterstützt GNOME/KDE; TUN benötigt systemd und Polkit.",
  "download.linux.archAria": "Linux-Architektur wählen",
  "download.linux.appimageAction": "AppImage laden",
  "download.linux.debAction": ".deb laden",
  "download.android.name": "Android",
  "download.android.kind": "arm64",
  "download.android.requires": "Android 14 oder neuer",
  "download.android.note":
    "Noch in keinem App-Store verfügbar. Erlaube bei der Android-Abfrage Installationen aus dieser Quelle.",
  "footer.source": "Quellcode",
  "footer.releases": "Releases",
  "footer.changelog": "Changelog",
  "footer.license": "Lizenziert unter GPL-3.0-or-later",
};
