// SPDX-License-Identifier: GPL-3.0-or-later

import type { MarketingKey } from "./zh";

export const fr: Record<MarketingKey, string> = {
  "head.title": "ice-box — la meilleure interface sing-box",
  "head.description":
    "ice-box est un client proxy léger pour macOS, Windows, Linux et Android : simple à configurer, rapide à démarrer, économe en ressources et prêt pour les Agents via MCP.",
  "nav.languageAria": "Choisir la langue",
  "nav.themeToLight": "Passer en mode clair",
  "nav.themeToDark": "Passer en mode sombre",
  "nav.features": "Fonctionnalités",
  "nav.agent": "Agent",
  "nav.download": "Télécharger",
  "hero.release": "{version} est disponible",
  "hero.title": "La meilleure <span class=\"accent\">sing-box GUI</span>",
  "hero.subtitle":
    "Un client proxy léger pour macOS, Windows, Linux et Android. Importez un abonnement, choisissez un nœud et activez. Discret en arrière-plan, et votre Agent peut collaborer avec vous via MCP.",
  "hero.installer": "Téléchargement gratuit",
  "hero.installerPlatform": "Télécharger pour {platform}",
  "hero.tryDemo": "Essayer la démo",
  "hero.platforms": "macOS · Windows · Linux · Android",
  "demo.windowBar": "démo interactive",
  "demo.iframeTitle": "Démo en direct de l'application bureau ice-box",
  "features.kicker": "Fonctionnalités",
  "features.title": "Tout l'essentiel, à portée de main",
  "features.body":
    "De l'import d'un abonnement à l'inspection d'une connexion, chaque action courante se fait en un ou deux clics.",
  "features.groupDaily": "Au quotidien",
  "features.groupBackground": "En arrière-plan",
  "features.subs.title": "Abonnements",
  "features.subs.body":
    "Détecte automatiquement sing-box JSON, Clash et les liens partagés, et importe aussi les fichiers locaux. Sur bureau, les abonnements s'actualisent seuls avec trafic restant et expiration visibles.",
  "features.routing.title": "Routage",
  "features.routing.body":
    "Passez à tout moment entre Règles, Global et Direct. Activez ou désactivez chaque règle d'abonnement et ajoutez vos propres règles par-dessus.",
  "features.nodes.title": "Nœuds",
  "features.nodes.body":
    "Choisissez une sortie par groupe de stratégie, testez la latence d'un nœud ou d'une liste entière et épinglez vos favoris.",
  "features.connections.title": "Connexions",
  "features.connections.body":
    "Sur bureau (macOS, Windows et Linux), suivez chaque connexion active : destination, règle appliquée, sortie et trafic. Recherchez, triez ou fermez-les en un clic.",
  "features.capture.title": "Capture",
  "features.capture.body":
    "Utilisez le proxy système ou TUN sur bureau. Sur Android, ice-box fonctionne comme VPN système avec routage par application : suivre les règles, proxy, direct ou bloquer.",
  "features.tray.title": "Barre système",
  "features.tray.body":
    "Démarrez ou arrêtez le proxy et changez de mode, d'abonnement ou de sortie sans ouvrir la fenêtre. Sur macOS, la barre des menus affiche aussi le débit en direct.",
  "features.upkeep.title": "Sans souci",
  "features.upkeep.body":
    "Lancement au démarrage, mises à jour signées intégrées, trafic et mémoire en direct sur l'Accueil, 16 langues avec prise en charge RTL complète et thèmes clair et sombre.",
  "agent.kicker": "Conçu pour les Agents",
  "agent.title": "Votre Agent travaille dans la même session que vous",
  "agent.body":
    "L'application bureau inclut un service MCP local optionnel. Activez-le et donnez la configuration à votre Agent : il peut lire l'état, changer de mode et de nœuds, gérer abonnements et règles et inspecter les connexions sur les mêmes données que vous voyez.",
  "agent.point1": "Aucun processus supplémentaire, aucun dossier de données dupliqué",
  "agent.point2": "Vos changements et ceux de l'Agent sont visibles instantanément",
  "agent.point3": "Désactivé par défaut, écoute uniquement en local",
  "agent.terminalTitle": "Session Agent",
  "agent.prompt": "Passe en mode Global, puis montre-moi les connexions les plus actives",
  "agent.reply":
    "Passé en mode Global. Sur 8 connexions, googlevideo.com (YouTube) consomme le plus, via Los Angeles / edge-03.",
  "phoneDemo.iframeTitle": "Démo en direct de l'application Android ice-box",
  "download.kicker": "Télécharger",
  "download.title": "Choisissez votre plateforme",
  "download.body": "Les installeurs sont hébergés sur GitHub Releases. Aucune inscription requise.",
  "download.tabListAria": "Choisir le système d'exploitation",
  "download.actionPlatform": "Télécharger pour {platform}",
  "download.yours": "Cet appareil",
  "download.allReleases": "Voir toutes les versions et les notes de version",
  "download.macos.name": "macOS",
  "download.macos.kind": "Apple Silicon",
  "download.macos.requires": "macOS 12 ou ultérieur",
  "download.macos.note":
    "Si macOS indique un développeur non vérifié au premier lancement, autorisez l'app dans Réglages Système → Confidentialité et sécurité.",
  "download.windows.name": "Windows",
  "download.windows.kind": "64 bits",
  "download.windows.requires": "Windows 10 ou ultérieur",
  "download.windows.note":
    "S'installe pour l'utilisateur courant sans droits administrateur. SmartScreen peut demander confirmation.",
  "download.linux.name": "Linux",
  "download.linux.requires": "Ubuntu 22.04+ et distributions récentes",
  "download.linux.note":
    "AppImage inclut des mises à jour signées intégrées ; .deb se met à jour via le gestionnaire de paquets. Le proxy système prend en charge GNOME/KDE ; TUN requiert systemd et Polkit.",
  "download.linux.archAria": "Choisir l'architecture Linux",
  "download.linux.appimageAction": "Télécharger AppImage",
  "download.linux.debAction": "Télécharger .deb",
  "download.android.name": "Android",
  "download.android.kind": "arm64",
  "download.android.requires": "Android 14 ou ultérieur",
  "download.android.note":
    "Pas encore sur les boutiques d'applications. Lorsqu'Android vous le demande, autorisez les installations depuis cette source.",
  "download.cli.subtitle":
    "CLI et démon sans interface pour macOS, Windows et Linux — conçus pour les agents, serveurs et scripts.",
  "download.cli.shellAria": "Choisir la commande d'installation CLI",
  "download.cli.copy": "Copier",
  "download.cli.copied": "Copié",
  "download.cli.archives": "Archives directes",
  "download.cli.docs": "Doc CLI",
  "footer.source": "Code source",
  "footer.releases": "Versions",
  "footer.changelog": "Notes de version",
  "footer.license": "Sous licence GPL-3.0-or-later",
};
