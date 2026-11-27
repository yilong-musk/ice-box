<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/brand/switch-logo/assets/logo.gif" alt="Logo ice-box" width="144">
</a>

# ice-box

**La meilleure sing-box GUI.**

Un client proxy léger pour macOS, Windows, Linux et Android.<br>
Simple à configurer, rapide à démarrer, discret une fois lancé et pensé pour les Agents — basé sur le cœur [sing-box](https://github.com/SagerNet/sing-box).

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-007ec6?style=flat-square)](#installation)
[![Core](https://img.shields.io/badge/sing--box-1.14.4-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**Télécharger**](https://github.com/yilong-musk/ice-box/releases/latest) · [**Démo en direct**](https://yilong-musk.github.io/ice-box/) · [Installation](#installation) · [TUN](#mode-tun) · [Documentation](#documentation) · [Notes de version](CHANGELOG.md)

[Bahasa Indonesia](README.id.md) · [Deutsch](README.de.md) · [English](README.md) · [Español](README.es.md) · **Français** · [Português (BR)](README.pt-BR.md) · [Tiếng Việt](README.vi.md) · [Türkçe](README.tr.md)<br>[Русский](README.ru.md) · [Українська](README.uk.md) · [العربية](README.ar.md) · [فارسی](README.fa.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [简体中文](README.zh-CN.md) · [繁體中文](README.zh-TW.md)

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/hero.png" alt="ice-box sur bureau et Android : état du proxy, changement de mode, nœud de sortie, courbe de trafic en direct et Accueil mobile avec la liste des nœuds" width="880">
</a>

<sub>La <a href="https://yilong-musk.github.io/ice-box/">Démo en direct</a> exécute les interfaces bureau et Android dans votre navigateur avec un backend simulé.</sub>

</div>

## Fonctionnalités

- **Abonnements.** Les formats sing-box JSON, Clash et les listes de liens partagés sont détectés automatiquement, et un fichier de configuration sing-box local peut aussi être importé. L'application bureau peut mettre à jour les profils automatiquement ; Android actualise lors de l'import ou d'une actualisation manuelle. Le trafic et la date d'expiration signalés par le fournisseur s'affichent sur chaque abonnement.
- **Routage.** Modes **Règles**, **Global** et **Direct**, un interrupteur pour chaque règle d'abonnement, et vos propres règles personnalisées par-dessus.
- **Nœuds.** Choisissez une sortie par groupe de stratégie, testez la latence d'un nœud ou d'une liste entière à la fois et épinglez vos favoris.
- **Connexions.** Sur bureau (macOS, Windows et Linux), observez chaque connexion active : destination, règle appliquée, sortie utilisée et volume transféré. Recherchez, triez et fermez une ou toutes les connexions.
- **Capture.** Proxy système ou TUN sur bureau ; VPN système sur Android, avec routage par application (suivre les règles, proxy, direct ou bloquer).
- **Barre système.** Démarrez et arrêtez le service proxy et changez de mode, d'abonnement ou de nœud de sortie sans ouvrir la fenêtre. Sur macOS, l'élément de la barre des menus affiche également le trafic en direct et le menu permet de lancer des tests de latence.
- **Accès Agent.** Un service MCP local optionnel permet à un Agent de piloter la même session en cours d'exécution.
- **Sans souci.** Lancer à l'ouverture de session, mises à jour intégrées vérifiées par signature, trafic et mémoire en direct sur l'Accueil, 16 langues avec prise en charge RTL complète (Bahasa Indonesia, Deutsch, English, Español, Français, Português (BR), Tiếng Việt, Türkçe, Русский, Українська, العربية, فارسی, 日本語, 한국어, 简体中文, 繁體中文), thèmes clair et sombre.

## Installation

Téléchargez l'installeur correspondant à votre plateforme depuis la [dernière version](https://github.com/yilong-musk/ice-box/releases/latest).

| Plateforme | Prérequis | Installeur | Premier lancement |
|---|---|---|---|
| **macOS** (Apple Silicon) | macOS 12 ou ultérieur | `ice-box_<version>_aarch64.dmg` | Signature ad-hoc uniquement (pas de Developer ID, non notarisé) : le premier lancement affiche un avertissement de **développeur non identifié**. Autorisez l'app dans **Réglages Système → Confidentialité et sécurité → Ouvrir quand même** (macOS 15+), faites un clic droit puis **Ouvrir** (versions antérieures), ou exécutez `xattr -dr com.apple.quarantine /Applications/ice-box.app`. |
| **Windows** (x64) | Windows 10 ou ultérieur | `ice-box_<version>_x64-setup.exe` | Installeur NSIS par utilisateur ; aucun droit administrateur requis. Il ne dispose pas de signature Authenticode, SmartScreen peut donc demander confirmation. |
| **Linux** (x64 / arm64) | Ubuntu 22.04+ / distributions récentes | `ice-box_<version>_amd64.AppImage` / `aarch64.AppImage`<br>`ice-box_<version>_amd64.deb` / `arm64.deb` | Rendez l'AppImage exécutable (`chmod +x`) ou installez le `.deb` avec `sudo apt install ./ice-box_*.deb`. L'icône dans la barre système sous GNOME nécessite l'extension AppIndicator (`gnome-shell-extension-appindicator`). Sous Wayland + NVIDIA, ice-box définit automatiquement `WEBKIT_DISABLE_DMABUF_RENDERER=1` s'il n'est pas déjà configuré. |
| **Android** (arm64) | Android 14 ou ultérieur | `ice-box_<version>_android_arm64.apk` | Signé avec le keystore du projet. Non publié sur Google Play ; Android demande l'autorisation d'installer depuis cette source. |

**Mises à jour.** Sur macOS, Windows (0.1.5+) et les installations Linux AppImage, ice-box vérifie GitHub Releases en arrière-plan et installe des mises à jour vérifiées par signature depuis **Réglages → Mises à jour** ; les installations macOS/Windows plus anciennes nécessitent d'abord une mise à niveau manuelle. Les installations Linux `.deb` consultent le même catalogue et ouvrent la page de téléchargement de la version afin que les mises à jour restent gérées par `apt` / `dpkg`. Sur Android, **Réglages → Mises à jour** télécharge l'APK arm64 et le remet à l'installeur système pour confirmation.

## Démarrage rapide

1. **Importer un abonnement.** Ouvrez **Abonnements**, collez l'URL et cliquez sur **Importer** ; le format est détecté automatiquement. Pour utiliser un fichier de configuration sing-box local (JSON) à la place, choisissez **Fichier de config** ; il n'a pas d'URL distante et ne peut donc pas être actualisé.
2. **Choisir un mode et un nœud.** Dans **Accueil**, choisissez **Règles**, **Global** ou **Direct**. Sur bureau, ouvrez **Nœuds** pour choisir une sortie et tester sa latence ; sur Android, utilisez la liste des nœuds sur l'**Accueil**.
3. **Démarrer le service proxy.** Appuyez sur le bouton d'alimentation. Sur bureau, l'application applique le proxy système ou démarre TUN lorsqu'il est activé ; sur Android, elle connecte le VPN après la demande d'autorisation système.

Pas encore d'abonnement ? ice-box démarre en mode direct uniquement, ce qui permet de découvrir le cœur, la capture et l'interface avant de rien importer.

## Lancer à l'ouverture de session

Sur macOS, Windows et Linux, activez **Réglages → Démarrage → Lancer à l'ouverture de session**. Un démarrage à l'ouverture de session reste dans la barre système avec la fenêtre masquée, lance le cœur et restaure le dernier état du proxy.

Désactivez-le avec le même interrupteur : désactiver l'entrée dans **Éléments d'ouverture** de macOS ne l'efface pas. Installez l'application avant de l'activer sur macOS, car l'exécution directe depuis l'image disque est refusée. Sur Linux, l'entrée est écrite dans `~/.config/autostart/com.yilong-musk.icebox.desktop` (en enregistrant `$APPIMAGE` lors d'une exécution depuis un AppImage).

Android ne dispose pas d'élément d'ouverture de session. **Réglages → Système** explique l'exemption d'optimisation de la batterie, le DNS privé strict et le VPN permanent.

## Mode TUN

Sur macOS, Windows et Linux, TUN capture le trafic au niveau de la couche réseau, couvrant ainsi les applications qui ne tiennent jamais compte du proxy système. Le mode TUN sous Windows ne transporte que le trafic IPv4 TCP (pas d'UDP/QUIC ni d'IPv6) et nécessite un compte Administrateur ; le proxy système reste l'alternative dans ce cas. Sous Linux, TUN utilise Polkit (`pkexec`) pour installer un démon systemd `ice-helper` et gère le DNS via `systemd-resolved` (`resolvectl`) ou un fichier `/etc/resolv.conf` statique. Consultez [`docs/tun.md`](docs/tun.md) pour la configuration, le comportement du basculement, la prise en charge par plateforme et les limites. Android utilise le VPN système ; voir [`docs/architecture.md`](docs/architecture.md#clients).

## Accès Agent via MCP

L'application bureau (macOS, Windows et Linux) expose un service MCP local optionnel. Ouvrez **MCP** entre **Connexions** et **Réglages**, activez-le et copiez la configuration client générée. L'Agent partage les réglages, abonnements, données, cœur et état du proxy de la session en cours au lieu de lancer un autre runtime.

- **Jeu d'outils complet.** État, réglages, trafic, abonnements, nœuds et groupes, règles, connexions en direct, journaux, diagnostics, contrôle du proxy et récupération protégée de TUN. Les résultats incluent du contenu structuré lorsqu'il est disponible et préservent les codes d'erreur de l'application.
- **Sécurisé par défaut.** MCP écoute uniquement sur loopback authentifié avec un jeton de contrôle aléatoire qui n'est jamais inclus dans la configuration client copiée.
- **Pont stdio intégré.** La configuration générée lance le même exécutable en mode pont, aucun démon séparé ni adaptateur de protocole n'est donc nécessaire.

MCP est réservé au bureau : Android ne propose pas MCP et la démo du site web utilise un service simulé. Consultez [`docs/mcp.md`](docs/mcp.md) pour la configuration, le cycle de vie et les limites de sécurité.

## Développement

Rust (la version épinglée dans `rust-toolchain.toml`, que rustup installe à la première utilisation), Node.js 22, Xcode Command Line Tools sur macOS, et les paquets de développement GTK/WebKit (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libssl-dev`) sur Linux. Les builds Android nécessitent aussi JDK 17, Go et un SDK Android (`ANDROID_HOME`) avec les paquets installés par `scripts/ci-install-android-sdk.sh`.

```bash
cd apps/desktop && npm install && cd ../..
npm run fetch-singbox   # télécharger le cœur sing-box épinglé pour cet hôte
npm run dev
```

| Commande | Objectif |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` / `dev:linux-x64` / `dev:linux-arm64` | Lancer pour une cible hôte spécifique |
| `npm run gate` | Porte complète de l'espace de travail : fmt, clippy, tests Rust, vérifications de types, builds, tests smoke, fixtures |
| `npm run fetch-singbox && npm run build` | Construire le `.dmg` macOS |
| `npm run fetch-singbox -- win && npm run build:win` | Construire l'installeur NSIS Windows |
| `npm run fetch-singbox -- linux-x64 && npm run build:linux` | Construire les paquets Linux `.deb` et `.AppImage` (passez `linux-arm64` sur arm64) |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` / `acceptance:linux` / `acceptance:linux-tun` | Tests d'acceptation en direct sur une machine réelle |
| `bash scripts/build-android-apk.sh debug arm64` | APK de débogage pour un appareil arm64 |
| `bash scripts/build-android-apk.sh debug amd64` | APK de débogage pour l'émulateur |

```
apps/desktop    Interface React + coque Tauri pour macOS, Windows et Linux
apps/mobile     Coque Android ; réutilise l'interface bureau
apps/website    Site GitHub Pages et Démo en direct
crates/         Espace de travail Rust (crates principaux) : ice-app, ice-config, ice-core, ice-engine, ice-subscription, ice-proxy-sys, ice-tun-sys, ice-elevate, ice-helper, ice-types, ...
```

## Documentation

| Document | Contenu |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | Structure du système, responsabilités des composants, flux de données et limites de conception |
| [`docs/mcp.md`](docs/mcp.md) | Configuration MCP bureau, modèle de runtime partagé, outils et sécurité |
| [`docs/tun.md`](docs/tun.md) | Comportement de TUN, récupération, configuration par plateforme et limites |
| [`docs/testing.md`](docs/testing.md) | Couverture des portes locales et CI, tests manuels en conditions réelles |
| [`docs/release-process.md`](docs/release-process.md) | Gestion des versions, signature, empaquetage et publication |
| [`CHANGELOG.md`](CHANGELOG.md) | Historique des versions |

Ces documents techniques sont actuellement rédigés en anglais.

## Licence

ice-box est un logiciel libre, publié sous la [licence publique générale GNU v3.0 ou ultérieure](LICENSE). Le cœur sing-box embarqué est également sous [GPL-3.0-or-later](third_party/sing-box/LICENSE), avec sa restriction de nommage en amont ; consultez [NOTICE](NOTICE) pour l'ensemble des mentions tierces.

<div align="center">
<br>
<sub>Construit avec Tauri, React et sing-box.</sub>
</div>
