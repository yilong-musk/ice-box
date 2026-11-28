// SPDX-License-Identifier: GPL-3.0-or-later

import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import faviconUrl from "../../desktop/public/favicon.png";
import runningStill from "../../desktop/src/assets/switch-logo/32/running-still.png";
import runningStrip from "../../desktop/src/assets/switch-logo/32/running.png";
import { version as appVersion } from "../../desktop/package.json";
import {
  LANGUAGE_LABELS,
  LANGUAGE_STORAGE_KEY,
  SUPPORTED_LANGUAGES,
  isLanguagePreference,
  languageDirection,
  resolveLanguage,
} from "../../desktop/src/lib/language";
import {
  THEME_STORAGE_KEY,
  isThemePreference,
  readThemePreference,
  resolveTheme,
} from "../../desktop/src/lib/theme";
import {
  CLI_INSTALL_COMMAND,
  CLI_TARGETS,
  INSTALLER_PLATFORMS,
  cliDownloadUrl,
  defaultCliShell,
  detectLinuxArch,
  detectPlatform,
  installerDownloadUrl,
  installerFilename,
  linuxInstallerDownloadUrl,
  linuxInstallerFilename,
  linuxInstallerVariantsForArch,
  linuxPrimaryInstallerVariant,
  refineLinuxArch,
} from "./downloads";
import {
  applyMarketingLanguage,
  marketingText,
  resolveInitialLanguage,
} from "./i18n";
import { DEMO_RESIZE_MESSAGE } from "./browser-window-chrome";
import { phoneStatusIcons } from "./phone-status-icons.js";
import "./phone-demo.css";
import "./style.css";

/** Same source as the desktop app and the Live Demo title bar (`APP_VERSION`). */
const displayVersion = `v${appVersion}`;
const REPO_URL = "https://github.com/yilong-musk/ice-box";

const favicon = document.createElement("link");
favicon.rel = "icon";
favicon.type = "image/png";
favicon.href = faviconUrl;
document.head.appendChild(favicon);

/** Resolved before the first render, so the first paint is already localized
 * (the static index.html keeps its English copy as the no-JS fallback). */
let language = resolveInitialLanguage();

/**
 * Line glyphs on a 24-unit grid, stroked in the text color. The feature
 * glyphs are the app's own navigation icons (desktop `NavIcon.tsx`), so a
 * row here and the page it describes in the app share one symbol.
 */
const ICONS = {
  // App navigation glyphs.
  subs: '<path d="m12 3.75 8.5 4.5-8.5 4.5-8.5-4.5z"/><path d="m3.5 12.25 8.5 4.5 8.5-4.5"/><path d="m3.5 16 8.5 4.5 8.5-4.5"/>',
  rules: '<path d="M7 3.5v17"/><path d="M7 14c0-4 2.5-7 7-7h5.5"/><path d="m16.75 4.25 2.75 2.75-2.75 2.75"/>',
  nodes: '<circle cx="12" cy="5.5" r="2.25"/><circle cx="5.5" cy="17.5" r="2.25"/><circle cx="18.5" cy="17.5" r="2.25"/><path d="M10.9 7.5 6.6 15.5M13.1 7.5l4.3 8M7.75 17.5h8.5"/>',
  connections: '<path d="M8 19.5v-15M4.5 8 8 4.5 11.5 8"/><path d="M16 4.5v15M12.5 16l3.5 3.5 3.5-3.5"/>',
  mcp: '<path d="M12 8V4H8"/><rect x="4" y="8" width="16" height="12" rx="2.5"/><path d="M2 14h2M20 14h2"/><path d="M9 13v2M15 13v2"/>',
  settings: '<path d="M4 7.5h8.5M17.5 7.5H20"/><circle cx="15" cy="7.5" r="2.5"/><path d="M4 16.5h2.5M11.5 16.5H20"/><circle cx="9" cy="16.5" r="2.5"/>',
  // Page glyphs, drawn to the same grid and stroke.
  shield: '<path d="M12 3.5c2.2 1.6 4.7 2.4 7.5 2.5v6c0 4.4-3 7.4-7.5 8.5-4.5-1.1-7.5-4.1-7.5-8.5V6c2.8-.1 5.3-.9 7.5-2.5z"/><path d="m9 12 2.2 2.2L15.5 10"/>',
  menubar: '<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><path d="M3 9h18"/><path d="M15.5 6.75h2.5"/>',
  download: '<path d="M12 4v11"/><path d="m7.5 10.5 4.5 4.5 4.5-4.5"/><path d="M4.5 16.5v1.5a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2v-1.5"/>',
  play: '<path d="M8 5.6v12.8a.8.8 0 0 0 1.2.7l10.2-6.4a.8.8 0 0 0 0-1.4L9.2 4.9A.8.8 0 0 0 8 5.6z"/>',
  arrowRight: '<path d="M5 12h14"/><path d="m13 6 6 6-6 6"/>',
  chevronRight: '<path d="m9.5 6 6 6-6 6"/>',
  chevronDown: '<path d="m6 9 6 6 6-6"/>',
  globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18"/><path d="M12 3a13.5 13.5 0 0 1 0 18 13.5 13.5 0 0 1 0-18"/>',
  check: '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 16v-4"/><path d="M12 8h.01"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M4.6 4.6l1.4 1.4M18 18l1.4 1.4M2.5 12h2M19.5 12h2M4.6 19.4 6 18M18 6l1.4-1.4"/>',
  moon: '<path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z"/>',
  terminal: '<path d="m5 7 5 5-5 5"/><path d="M12 19h7"/>',
  copy: '<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>',
  github:
    '<path d="M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.4 5.4 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4"/><path d="M9 18c-4.51 2-5-2-7-2"/>',
};

function icon(name, className = "icon") {
  return `<svg class="${className}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICONS[name]}</svg>`;
}

/**
 * Solid platform marks on a 24-unit grid for the download rail and detail
 * panes, using evenodd cutouts so transparent details hold up on any surface.
 */
const PLATFORM_MARKS = {
  macos:
    '<path fill="currentColor" d="M16.92 12.63c-.02-2.08 1.7-3.08 1.78-3.13-.97-1.42-2.48-1.61-3.02-1.63-1.28-.13-2.51.76-3.16.76-.66 0-1.67-.74-2.75-.72-1.41.02-2.72.83-3.44 2.1-1.48 2.56-.38 6.35 1.05 8.43.7 1.02 1.54 2.16 2.64 2.12 1.06-.04 1.46-.68 2.75-.68 1.28 0 1.65.68 2.76.66 1.14-.02 1.86-1.03 2.55-2.05.81-1.18 1.14-2.32 1.16-2.38-.03-.01-2.29-.88-2.32-3.48zM14.84 6.5c.58-.71.98-1.69.87-2.67-.84.03-1.87.56-2.47 1.27-.54.62-1.01 1.62-.89 2.58.94.07 1.9-.48 2.49-1.18z"/>',
  windows:
    '<rect x="3.2" y="3.2" width="8" height="8" rx="1" fill="currentColor"/><rect x="12.8" y="3.2" width="8" height="8" rx="1" fill="currentColor"/><rect x="3.2" y="12.8" width="8" height="8" rx="1" fill="currentColor"/><rect x="12.8" y="12.8" width="8" height="8" rx="1" fill="currentColor"/>',
  linux:
    '<path fill="currentColor" fill-rule="evenodd" d="M5.5 4.5h13A2.5 2.5 0 0 1 21 7v10a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17V7a2.5 2.5 0 0 1 2.5-2.5Zm1.8 4.3 3.7 3.2-3.7 3.2-1.05-1.2L8.55 12 6.25 10l1.05-1.2ZM12 13.8h5v1.6h-5v-1.6Z"/>',
  android:
    '<path fill="currentColor" fill-rule="evenodd" d="M2.6 18.2a9.4 9.4 0 0 1 18.8 0H2.6Zm5.7-5.2a1.15 1.15 0 1 0 0 2.3 1.15 1.15 0 0 0 0-2.3Zm7.4 0a1.15 1.15 0 1 0 0 2.3 1.15 1.15 0 0 0 0-2.3Z"/><path d="M7.6 10.55 5.2 6.2M16.4 10.55 18.8 6.2" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>',
};

function platformMark(platform, className = "platform-mark") {
  return `<svg class="${className}" viewBox="0 0 24 24" aria-hidden="true">${PLATFORM_MARKS[platform]}</svg>`;
}

/** `data-i18n` fills textContent, `data-i18n-html` fills trusted markup from
 * the site's own dictionary (the hero title carries an accent span). */
const t = (key) => `data-i18n="${key}"`;

/**
 * Features as two grouped lists, the way the app's own settings read: what
 * you reach for every day, and what runs without you. Each row gets an app
 * glyph on a tinted tile.
 */
const FEATURE_GROUPS = [
  [
    "features.groupDaily",
    [
      ["subs", "subs", "blue"],
      ["routing", "rules", "orange"],
      ["nodes", "nodes", "indigo"],
      ["connections", "connections", "purple"],
    ],
  ],
  [
    "features.groupBackground",
    [
      ["capture", "shield", "green"],
      ["tray", "menubar", "teal"],
      ["upkeep", "settings", "graphite"],
    ],
  ],
];

const PLATFORM_SHORT_META = {
  macos: "arm64 · .dmg",
  windows: "x64 · .exe",
  linux: "AppImage · .deb",
  android: "arm64 · .apk",
};

const LINUX_ARCH_OPTIONS = [
  ["amd64", "x64"],
  ["arm64", "arm64"],
];

const CLI_SHELL_OPTIONS = [
  ["unix", "macOS / Linux"],
  ["windows", "Windows"],
];

const CLI_ARCHIVE_LABELS = {
  "darwin-aarch64": "macOS (arm64)",
  "windows-x86_64": "Windows (x64)",
  "linux-x86_64": "Linux (x64)",
  "linux-aarch64": "Linux (arm64)",
};

const windowLights = `<span class="window-lights" dir="ltr" aria-hidden="true"><i></i><i></i><i></i></span>`;

const currentPlatform = detectPlatform(navigator.userAgent, navigator.maxTouchPoints);
let selectedPlatform = currentPlatform ?? "macos";
let detectedLinuxArch =
  currentPlatform === "linux" ? detectLinuxArch(navigator.userAgent) : "amd64";
let selectedLinuxArch = detectedLinuxArch;
let linuxArchManuallySelected = false;
let selectedCliShell = defaultCliShell(currentPlatform);
let cliCopied = false;
let cliCopyTimer = null;

function platformPrimaryUrl(platform) {
  return platform === "linux"
    ? linuxInstallerDownloadUrl(appVersion, linuxPrimaryInstallerVariant(detectedLinuxArch))
    : installerDownloadUrl(appVersion, platform);
}

/** The hero button downloads this device's installer straight away; with no
 * installer for this device it scrolls to the platform list instead. */
const heroDownloadHref =
  currentPlatform == null ? "#download" : platformPrimaryUrl(currentPlatform);

const root = document.querySelector("#root");

root.innerHTML = `
  <header class="site-nav">
    <div class="nav-inner page-wrap">
      <a class="brand" href="#top"><span class="brand-logo" aria-hidden="true" style="--brand-logo-still: url('${runningStill}'); --brand-logo-strip: url('${runningStrip}')"></span><span class="brand-name">ice-box</span></a>
      <nav class="nav-links" aria-label="Primary">
        <a href="#features" ${t("nav.features")}></a>
        <a href="#agent" ${t("nav.agent")}></a>
        <a href="#download" ${t("nav.download")}></a>
      </nav>
      <div class="nav-actions">
        <a class="icon-button" href="${REPO_URL}" target="_blank" rel="noreferrer" aria-label="GitHub">${icon("github")}</a>
        <button class="icon-button nav-theme" type="button"><span class="theme-icon theme-icon-dark">${icon("moon")}</span><span class="theme-icon theme-icon-light">${icon("sun")}</span></button>
        <div class="nav-language-wrap">
          <button
            class="nav-language"
            type="button"
            aria-haspopup="listbox"
            aria-expanded="false"
            aria-controls="nav-language-menu"
          >
            ${icon("globe", "icon nav-language-icon")}
            <span class="nav-language-label"></span>
            ${icon("chevronDown", "icon nav-language-chevron")}
          </button>
          <div
            id="nav-language-menu"
            class="nav-language-menu"
            role="listbox"
            hidden
          >
            ${SUPPORTED_LANGUAGES.map(
              (code) => `<button
              type="button"
              class="nav-language-option"
              role="option"
              data-language="${code}"
              aria-selected="false"
            >
              <span class="nav-language-option-label" lang="${code}" dir="${languageDirection(code)}">${LANGUAGE_LABELS[code]}</span>
              <span class="nav-language-check" aria-hidden="true">${icon("check", "icon icon-xs")}</span>
            </button>`,
            ).join("")}
          </div>
        </div>
      </div>
    </div>
  </header>
  <main id="top">
    <section class="hero page-wrap">
      <a class="hero-release reveal" href="#download"><span class="status-dot" aria-hidden="true"></span><span class="hero-release-label"></span>${icon("chevronRight", "icon icon-xs")}</a>
      <h1 class="reveal" data-i18n-html="hero.title"></h1>
      <p class="hero-subtitle reveal" ${t("hero.subtitle")}></p>
      <div class="hero-actions reveal">
        <a class="button button-primary button-large hero-download" href="${heroDownloadHref}">${icon("download")}<span class="hero-download-label"></span></a>
        <a class="button button-large" href="#demo">${icon("play")}<span ${t("hero.tryDemo")}></span></a>
      </div>
      <p class="hero-meta reveal"><span ${t("hero.platforms")}></span><span class="dot-sep"></span><span>GPL-3.0-or-later</span><span class="dot-sep"></span><span>sing-box</span></p>
    </section>

    <section class="demo-section page-wrap" id="demo">
      <div class="showcase">
        <div class="desk-window reveal"><div class="demo-screen"><iframe src="./demo.html" loading="lazy"></iframe></div></div>
        <div class="phone-float reveal" id="android-demo">
          <div class="phone-demo-stage">
            <div class="phone-frame">
              <span class="phone-sim" aria-hidden="true"></span>
              <div class="phone-screen">
                <div class="phone-status-bar" dir="ltr" aria-hidden="true">
                  <span class="phone-clock"></span>
                  <div class="phone-status-icons">${phoneStatusIcons}</div>
                </div>
                <div class="phone-viewport">
                  <iframe src="./mobile-demo.html" loading="lazy"></iframe>
                  <span class="phone-gesture" aria-hidden="true"></span>
                </div>
                <span class="phone-camera" aria-hidden="true"></span>
              </div>
            </div>
          </div>
        </div>
      </div>
      <p class="demo-caption"><span class="status-dot" aria-hidden="true"></span><strong>ice-box</strong><span ${t("demo.windowBar")}></span><span>${displayVersion}</span></p>
    </section>

    <section class="section page-wrap features" id="features" aria-labelledby="features-title">
      <div class="section-head reveal">
        <p class="kicker" ${t("features.kicker")}></p>
        <h2 id="features-title" ${t("features.title")}></h2>
        <p class="section-body" ${t("features.body")}></p>
      </div>
      <div class="feature-groups">
        ${FEATURE_GROUPS.map(
          ([groupKey, rows]) => `<div class="feature-group reveal">
          <p class="group-label" ${t(groupKey)}></p>
          <ul class="grouped-list">
            ${rows
              .map(
                ([key, glyph, tone]) => `<li class="feature-row" data-feature="${key}">
              <span class="feature-tile" data-tone="${tone}">${icon(glyph)}</span>
              <div class="feature-text">
                <h3 ${t(`features.${key}.title`)}></h3>
                <p ${t(`features.${key}.body`)}></p>
              </div>
            </li>`,
              )
              .join("")}
          </ul>
        </div>`,
        ).join("")}
      </div>
    </section>

    <section class="section page-wrap agent" id="agent" aria-labelledby="agent-title">
      <div class="agent-copy reveal">
        <p class="kicker" ${t("agent.kicker")}></p>
        <h2 id="agent-title" ${t("agent.title")}></h2>
        <p class="section-body" ${t("agent.body")}></p>
        <ul class="check-list">
          ${[1, 2, 3].map((n) => `<li>${icon("check", "icon icon-check")}<span ${t(`agent.point${n}`)}></span></li>`).join("")}
        </ul>
      </div>
      <div class="agent-window reveal" role="img" aria-labelledby="agent-window-title">
        <div class="window-bar">${windowLights}<span id="agent-window-title" class="window-title">${icon("mcp", "icon icon-xs")}<span ${t("agent.terminalTitle")}></span></span></div>
        <div class="chat">
          <p class="bubble bubble-user" ${t("agent.prompt")}></p>
          <div class="tool-calls" dir="ltr">
            <p class="tool-call"><span class="tool-name">ice-box.set_proxy_mode</span><span class="tool-args">{"mode":"global"}</span><span class="tool-ok">${icon("check", "icon icon-xs")}</span></p>
            <p class="tool-call"><span class="tool-name">ice-box.list_connections</span><span class="tool-args">{}</span><span class="tool-ok">${icon("check", "icon icon-xs")}</span></p>
          </div>
          <p class="bubble bubble-agent" ${t("agent.reply")}></p>
          <p class="config" dir="ltr"><span class="cfg-comment">// mcpServers.ice-box</span><br><span class="cfg-key">"command"</span>: <span class="cfg-str">".../ice-box"</span>,<br><span class="cfg-key">"args"</span>: [<span class="cfg-str">"--mcp-stdio"</span>, <span class="cfg-str">"--data-dir"</span>, <span class="cfg-str">"..."</span>]</p>
        </div>
      </div>
    </section>

    <section class="section page-wrap download" id="download" aria-labelledby="download-title">
      <div class="section-head section-head-center reveal">
        <p class="kicker" ${t("download.kicker")}></p>
        <h2 id="download-title" ${t("download.title")}></h2>
        <p class="section-body" ${t("download.body")}></p>
      </div>
      <div class="download-panel reveal">
        <div class="download-rail" role="tablist">
          ${INSTALLER_PLATFORMS.map(
            (platform) => `<button
            type="button"
            class="download-tab"
            role="tab"
            id="download-tab-${platform}"
            aria-controls="download-pane-${platform}"
            aria-selected="${platform === selectedPlatform ? "true" : "false"}"
            tabindex="${platform === selectedPlatform ? "0" : "-1"}"
            data-download-tab="${platform}"
          >
            <span class="download-tab-icon">${platformMark(platform)}</span>
            <span class="download-tab-text">
              <span class="download-tab-name"></span>
              <span class="download-tab-sub">${PLATFORM_SHORT_META[platform]}</span>
            </span>
            <span class="download-current-dot" aria-hidden="true" hidden></span>
          </button>`,
          ).join("")}
        </div>
        <div class="download-stage">
          ${INSTALLER_PLATFORMS.map(
            (platform) => `<div
            class="download-pane"
            role="tabpanel"
            id="download-pane-${platform}"
            aria-labelledby="download-tab-${platform}"
            data-download-panel="${platform}"
            ${platform === selectedPlatform ? "" : "hidden"}
          >
            <div class="download-pane-head">
              <span class="download-emblem">${platformMark(platform)}</span>
              <div class="download-pane-identity">
                <div class="download-pane-title-row">
                  <h3 class="download-name"></h3>
                  <span class="download-version">${displayVersion}</span>
                  <span class="download-yours" hidden></span>
                </div>
                <p class="download-meta-line"><span class="download-kind"></span><span class="dot-sep" aria-hidden="true"></span><span class="download-requires"></span></p>
              </div>
              ${
                platform === "linux"
                  ? `<div class="download-arch-group" role="group" data-linux-arch-group>${LINUX_ARCH_OPTIONS.map(
                      ([arch, label]) =>
                        `<button type="button" class="download-arch-option" data-linux-arch="${arch}" aria-pressed="${arch === selectedLinuxArch ? "true" : "false"}">${label}</button>`,
                    ).join("")}</div>`
                  : ""
              }
            </div>
            ${
              platform === "linux"
                ? `<div class="download-action-row download-action-row-linux">
              <a class="button button-primary button-large download-button" data-linux-format="appimage" href="${linuxInstallerDownloadUrl(appVersion, linuxInstallerVariantsForArch(selectedLinuxArch).appimage)}">${icon("download")}<span class="download-action"></span></a>
              <a class="button button-large download-button-secondary" data-linux-format="deb" href="${linuxInstallerDownloadUrl(appVersion, linuxInstallerVariantsForArch(selectedLinuxArch).deb)}">${icon("download")}<span class="download-action-secondary"></span></a>
            </div>
            <p class="download-files"><span data-linux-file="appimage"></span><span class="dot-sep" aria-hidden="true"></span><span data-linux-file="deb"></span></p>`
                : `<div class="download-action-row">
              <a class="button button-primary button-large download-button" href="${platformPrimaryUrl(platform)}">${icon("download")}<span class="download-action"></span></a>
              <span class="download-file"></span>
            </div>`
            }
            <div class="download-note-box">
              ${icon("info", "icon download-note-icon")}
              <p class="download-note"></p>
            </div>
          </div>`,
          ).join("")}
        </div>
      </div>
      <div class="download-cli-card reveal" data-cli-card>
        <div class="download-cli-head">
          <span class="download-emblem download-cli-emblem">${icon("terminal", "platform-mark")}</span>
          <div class="download-pane-identity">
            <div class="download-pane-title-row">
              <h3 class="download-cli-title">ice-box CLI</h3>
              <span class="download-version">${displayVersion}</span>
            </div>
            <p class="download-cli-subtitle" ${t("download.cli.subtitle")}></p>
          </div>
          <div class="download-arch-group" role="group" data-cli-shell-group>
            ${CLI_SHELL_OPTIONS.map(
              ([shell, label]) =>
                `<button type="button" class="download-arch-option" data-cli-shell="${shell}" aria-pressed="${shell === selectedCliShell ? "true" : "false"}">${label}</button>`,
            ).join("")}
          </div>
        </div>
        <div class="download-cli-command" dir="ltr">
          <pre class="download-cli-code"><code data-cli-command>${CLI_INSTALL_COMMAND[selectedCliShell]}</code></pre>
          <button type="button" class="button download-cli-copy" data-cli-copy>
            <span class="download-cli-copy-icon">${icon("copy", "icon icon-xs")}</span>
            <span class="download-cli-copied-icon" hidden>${icon("check", "icon icon-xs")}</span>
            <span class="download-cli-copy-label"></span>
          </button>
        </div>
        <div class="download-cli-footer">
          <div class="download-cli-archives">
            <span class="download-cli-archives-label" ${t("download.cli.archives")}></span>
            <div class="download-cli-links">
              ${CLI_TARGETS.map(
                (target) =>
                  `<a class="download-cli-link" data-cli-archive="${target}" href="${cliDownloadUrl(appVersion, target)}">${icon("download", "icon icon-xs")}<span>${CLI_ARCHIVE_LABELS[target]}</span></a>`,
              ).join("")}
            </div>
          </div>
          <a class="text-link download-cli-docs" href="${REPO_URL}/blob/main/docs/cli.md" target="_blank" rel="noreferrer"><span ${t("download.cli.docs")}></span>${icon("arrowRight", "icon icon-xs")}</a>
        </div>
      </div>
      <p class="download-more reveal"><a class="text-link" href="${REPO_URL}/releases" target="_blank" rel="noreferrer"><span ${t("download.allReleases")}></span>${icon("arrowRight", "icon icon-xs")}</a></p>
    </section>
  </main>
  <footer class="site-footer">
    <div class="footer-inner page-wrap">
      <p class="footer-legal"><span>© 2026 ice-box</span><span class="dot-sep"></span><span ${t("footer.license")}></span><span class="dot-sep"></span><span>${displayVersion}</span></p>
      <nav class="footer-links" aria-label="Footer">
        <a href="${REPO_URL}" target="_blank" rel="noreferrer">${icon("github", "icon icon-xs")}<span ${t("footer.source")}></span></a>
        <a href="${REPO_URL}/releases" target="_blank" rel="noreferrer"><span ${t("footer.releases")}></span></a>
        <a href="${REPO_URL}/blob/main/CHANGELOG.md" target="_blank" rel="noreferrer"><span ${t("footer.changelog")}></span></a>
      </nav>
    </div>
  </footer>
`;

const parts = {
  heroDownload: root.querySelector(".hero-download"),
  heroDownloadLabel: root.querySelector(".hero-download-label"),
  heroReleaseLabel: root.querySelector(".hero-release-label"),
  demoFrame: root.querySelector(".demo-screen iframe"),
  deskWindow: root.querySelector(".desk-window"),
  phoneDemoFrame: root.querySelector(".phone-viewport iframe"),
  phoneFrame: root.querySelector(".phone-frame"),
  phoneClock: root.querySelector(".phone-clock"),
  languageWrap: root.querySelector(".nav-language-wrap"),
  languageToggle: root.querySelector(".nav-language"),
  languageLabel: root.querySelector(".nav-language-label"),
  languageMenu: root.querySelector(".nav-language-menu"),
  languageOptions: Array.from(root.querySelectorAll(".nav-language-option")),
  themeToggle: root.querySelector(".nav-theme"),
  nav: root.querySelector(".site-nav"),
  downloadRail: root.querySelector(".download-rail"),
  linuxArchGroup: root.querySelector("[data-linux-arch-group]"),
  cliShellGroup: root.querySelector("[data-cli-shell-group]"),
  cliCommand: root.querySelector("[data-cli-command]"),
  cliCopyButton: root.querySelector("[data-cli-copy]"),
  cliCopyLabel: root.querySelector(".download-cli-copy-label"),
  cliCopyIcon: root.querySelector(".download-cli-copy-icon"),
  cliCopiedIcon: root.querySelector(".download-cli-copied-icon"),
};

const reducedMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;

/**
 * The Live Demo lays the desktop app out in a slot for its expanded window
 * (860x620) and scales that slot to the available width, so the demo matches
 * the app and the README screenshots. Below 1100px the window is not scaled
 * and the demo uses the full container width instead.
 *
 * The app opens collapsed, at its configured 740px window; expanding the
 * sidebar grows the real window by the sidebar delta (and collapsing shrinks
 * it back), keeping its left edge. The demo asks for the same resize
 * (`resizeWindowWidthBy` in browser-window-chrome.ts) and the frame follows
 * through `--demo-width`, at the same scale.
 */
const DEMO_WINDOW = { width: 860, height: 620 };
const DEMO_SHELL_MIN_WIDTH = 1100;
/** tauri.conf.json's window width; matches `--demo-width` in style.css. */
let demoWidth = 740;

function fitDemoFrame() {
  const frame = parts.demoFrame;
  if (window.matchMedia(`(max-width: ${DEMO_SHELL_MIN_WIDTH}px)`).matches) {
    frame.style.zoom = "";
    return;
  }
  frame.style.zoom = String(parts.deskWindow.clientWidth / DEMO_WINDOW.width);
}

function setDemoWidth(width) {
  demoWidth = width;
  parts.deskWindow.style.setProperty("--demo-width", String(width));
}

fitDemoFrame();
window.addEventListener("resize", fitDemoFrame);

window.addEventListener("message", (event) => {
  if (event.source !== parts.demoFrame.contentWindow) return;
  if (event.origin !== window.location.origin) return;
  const data = event.data;
  if (data?.type !== DEMO_RESIZE_MESSAGE || !Number.isFinite(data.delta)) return;
  const min = Number.isFinite(data.minWidth) ? data.minWidth : 0;
  // Same rule as the app (never under the minimum); the frame never grows
  // past the default window it was laid out for.
  setDemoWidth(Math.min(DEMO_WINDOW.width, Math.max(min, demoWidth + data.delta)));
});

/** The phone frame's status bar shows the visitor's local time, refreshed
 * every half minute so the live demo does not look frozen. */
function updatePhoneClock() {
  const now = new Date();
  parts.phoneClock.textContent = `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;
}

updatePhoneClock();
window.setInterval(updatePhoneClock, 30_000);

/**
 * Site theme. The page shares `ice-box.theme` with the Live Demo, so a pick
 * here also switches both embedded apps (they bridge the storage event), and
 * a pick inside a demo switches the page. `index.html` sets `data-theme`
 * before the first paint; this keeps it current.
 */

/** The visitor's pick when storage is blocked. It holds for this visit only,
 * so it also keeps an OS theme change from reverting the pick. */
let sessionTheme = null;

/** The theme the embedded apps resolve from the shared stored preference. */
function storedTheme() {
  return resolveTheme(readThemePreference());
}

function siteTheme() {
  return sessionTheme ?? storedTheme();
}

/** The toggle's label names the theme a click switches to. */
function syncThemeToggle() {
  const label = marketingText(
    siteTheme() === "dark" ? "nav.themeToLight" : "nav.themeToDark",
    language,
  );
  parts.themeToggle.setAttribute("aria-label", label);
  parts.themeToggle.title = label;
}

/** Same values as the app's page background (`--background` in index.css). */
const THEME_COLOR = { dark: "#151619", light: "#f3f3f6" };

function applySiteTheme() {
  const appTheme = storedTheme();
  const theme = sessionTheme ?? appTheme;
  document.documentElement.dataset.theme = theme;
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute("content", THEME_COLOR[theme]);
  // The phone frame draws Android system chrome around the embedded app, so
  // it follows the app's theme, which a session-only pick does not reach.
  parts.phoneFrame.dataset.theme = appTheme;
  syncThemeToggle();
}

applySiteTheme();
// Only a "system" preference follows the OS; resolveTheme reads it fresh.
window
  .matchMedia?.("(prefers-color-scheme: dark)")
  .addEventListener?.("change", applySiteTheme);

parts.themeToggle.addEventListener("click", () => {
  const next = siteTheme() === "dark" ? "light" : "dark";
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, next);
    sessionTheme = null;
  } catch {
    // Blocked storage: the page switches, the demos keep their own theme.
    sessionTheme = next;
  }
  applySiteTheme();
});

/** The nav gains a hairline once the page scrolls under it. */
function updateNavState() {
  parts.nav.classList.toggle("is-scrolled", window.scrollY > 8);
}

updateNavState();
window.addEventListener("scroll", updateNavState, { passive: true });

/** Sections fade in as they enter the viewport. Without IntersectionObserver,
 * or with reduced motion, everything is shown at once. */
function setUpReveal() {
  const items = root.querySelectorAll(".reveal");
  if (reducedMotion || typeof IntersectionObserver !== "function") {
    for (const item of items) item.classList.add("is-visible");
    return;
  }
  const observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        entry.target.classList.add("is-visible");
        observer.unobserve(entry.target);
      }
    },
    { rootMargin: "0px 0px -8% 0px", threshold: 0.08 },
  );
  for (const item of items) observer.observe(item);
}

setUpReveal();

const downloadRailHorizontal = window.matchMedia?.("(max-width: 960px)");

function syncDownloadRailOrientation() {
  parts.downloadRail.setAttribute(
    "aria-orientation",
    downloadRailHorizontal?.matches ? "horizontal" : "vertical",
  );
}

syncDownloadRailOrientation();
downloadRailHorizontal?.addEventListener?.("change", syncDownloadRailOrientation);

function selectDownloadPlatform(platform, { focusTab = false } = {}) {
  selectedPlatform = platform;
  for (const candidate of INSTALLER_PLATFORMS) {
    const active = candidate === platform;
    const tab = root.querySelector(`[data-download-tab="${candidate}"]`);
    const pane = root.querySelector(`[data-download-panel="${candidate}"]`);
    tab.setAttribute("aria-selected", active ? "true" : "false");
    tab.tabIndex = active ? 0 : -1;
    pane.hidden = !active;
    if (active && focusTab) {
      tab.focus();
    }
  }
}

parts.downloadRail.addEventListener("click", (event) => {
  const tab = event.target.closest("[data-download-tab]");
  if (!tab) return;
  selectDownloadPlatform(tab.dataset.downloadTab);
});

parts.downloadRail.addEventListener("keydown", (event) => {
  if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
  const tab = event.target.closest("[data-download-tab]");
  if (!tab) return;
  const index = INSTALLER_PLATFORMS.indexOf(tab.dataset.downloadTab);
  if (index === -1) return;
  let nextIndex = -1;
  if (event.key === "ArrowDown" || event.key === "ArrowRight") {
    nextIndex = (index + 1) % INSTALLER_PLATFORMS.length;
  } else if (event.key === "ArrowUp" || event.key === "ArrowLeft") {
    nextIndex = (index - 1 + INSTALLER_PLATFORMS.length) % INSTALLER_PLATFORMS.length;
  } else if (event.key === "Home") {
    nextIndex = 0;
  } else if (event.key === "End") {
    nextIndex = INSTALLER_PLATFORMS.length - 1;
  }
  if (nextIndex === -1) return;
  event.preventDefault();
  selectDownloadPlatform(INSTALLER_PLATFORMS[nextIndex], { focusTab: true });
});

parts.linuxArchGroup.addEventListener("click", (event) => {
  const button = event.target.closest("[data-linux-arch]");
  if (!button) return;
  const nextArch = button.dataset.linuxArch;
  if (nextArch !== "amd64" && nextArch !== "arm64") return;
  selectedLinuxArch = nextArch;
  linuxArchManuallySelected = true;
  applyDownloadCopy(language);
});

function syncCliCopyButton(next) {
  const label = marketingText(
    cliCopied ? "download.cli.copied" : "download.cli.copy",
    next,
  );
  parts.cliCopyLabel.textContent = label;
  parts.cliCopyButton.setAttribute("aria-label", label);
  parts.cliCopyButton.classList.toggle("is-copied", cliCopied);
  parts.cliCopyIcon.hidden = cliCopied;
  parts.cliCopiedIcon.hidden = !cliCopied;
}

function selectCliShell(shell) {
  if (shell !== "unix" && shell !== "windows") return;
  selectedCliShell = shell;
  if (cliCopyTimer != null) {
    window.clearTimeout(cliCopyTimer);
    cliCopyTimer = null;
  }
  cliCopied = false;
  for (const option of parts.cliShellGroup.querySelectorAll("[data-cli-shell]")) {
    option.setAttribute(
      "aria-pressed",
      option.dataset.cliShell === selectedCliShell ? "true" : "false",
    );
  }
  parts.cliCommand.textContent = CLI_INSTALL_COMMAND[selectedCliShell];
  syncCliCopyButton(language);
}

async function copyCliCommand() {
  const command = CLI_INSTALL_COMMAND[selectedCliShell];
  let copied = false;
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(command);
      copied = true;
    } catch {
      // Fall back to a hidden textarea when clipboard permissions are unavailable.
    }
  }
  if (!copied) {
    const area = document.createElement("textarea");
    area.value = command;
    area.setAttribute("readonly", "");
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.select();
    try {
      copied = document.execCommand("copy");
    } catch {
      copied = false;
    }
    area.remove();
  }
  if (!copied) return;
  cliCopied = true;
  syncCliCopyButton(language);
  if (cliCopyTimer != null) window.clearTimeout(cliCopyTimer);
  cliCopyTimer = window.setTimeout(() => {
    cliCopied = false;
    cliCopyTimer = null;
    syncCliCopyButton(language);
  }, 2000);
}

parts.cliShellGroup.addEventListener("click", (event) => {
  const button = event.target.closest("[data-cli-shell]");
  if (!button) return;
  selectCliShell(button.dataset.cliShell);
});

parts.cliCopyButton.addEventListener("click", () => {
  void copyCliCommand();
});

/** Platform rail tabs, detail panes, requirements, and first-launch notes. */
function applyDownloadCopy(next) {
  parts.downloadRail.setAttribute(
    "aria-label",
    marketingText("download.tabListAria", next),
  );
  parts.linuxArchGroup.setAttribute(
    "aria-label",
    marketingText("download.linux.archAria", next),
  );
  parts.cliShellGroup.setAttribute(
    "aria-label",
    marketingText("download.cli.shellAria", next),
  );
  syncCliCopyButton(next);
  const yours = marketingText("download.yours", next);
  const actionTemplate = marketingText("download.actionPlatform", next);

  for (const platform of INSTALLER_PLATFORMS) {
    const tab = root.querySelector(`[data-download-tab="${platform}"]`);
    const pane = root.querySelector(`[data-download-panel="${platform}"]`);
    const isCurrent = platform === currentPlatform;
    const name = marketingText(`download.${platform}.name`, next);

    tab.classList.toggle("is-current", isCurrent);
    tab.querySelector(".download-tab-name").textContent = name;
    tab.querySelector(".download-current-dot").hidden = !isCurrent;
    tab.setAttribute("aria-label", isCurrent ? `${name}, ${yours}` : name);

    pane.classList.toggle("is-current", isCurrent);
    pane.querySelector(".download-name").textContent = name;
    const yoursEl = pane.querySelector(".download-yours");
    yoursEl.textContent = yours;
    yoursEl.hidden = !isCurrent;
    pane.querySelector(".download-requires").textContent = marketingText(
      `download.${platform}.requires`,
      next,
    );
    pane.querySelector(".download-note").textContent = marketingText(
      `download.${platform}.note`,
      next,
    );

    if (platform === "linux") {
      const archLabel = selectedLinuxArch === "arm64" ? "arm64" : "x64";
      const variants = linuxInstallerVariantsForArch(selectedLinuxArch);
      const appimageFile = linuxInstallerFilename(appVersion, variants.appimage);
      const debFile = linuxInstallerFilename(appVersion, variants.deb);
      const appimageLabel = marketingText("download.linux.appimageAction", next);
      const debLabel = marketingText("download.linux.debAction", next);

      pane.querySelector(".download-kind").textContent = archLabel;
      for (const option of parts.linuxArchGroup.querySelectorAll("[data-linux-arch]")) {
        option.setAttribute(
          "aria-pressed",
          option.dataset.linuxArch === selectedLinuxArch ? "true" : "false",
        );
      }

      const appimageBtn = pane.querySelector('[data-linux-format="appimage"]');
      appimageBtn.href = linuxInstallerDownloadUrl(appVersion, variants.appimage);
      appimageBtn.querySelector(".download-action").textContent = appimageLabel;
      appimageBtn.setAttribute("aria-label", `${appimageLabel} (${archLabel})`);

      const debBtn = pane.querySelector('[data-linux-format="deb"]');
      debBtn.href = linuxInstallerDownloadUrl(appVersion, variants.deb);
      debBtn.querySelector(".download-action-secondary").textContent = debLabel;
      debBtn.setAttribute("aria-label", `${debLabel} (${archLabel})`);

      pane.querySelector('[data-linux-file="appimage"]').textContent = appimageFile;
      pane.querySelector('[data-linux-file="deb"]').textContent = debFile;
      continue;
    }

    const actionLabel = actionTemplate.replaceAll("{platform}", name);
    pane.querySelector(".download-kind").textContent = marketingText(
      `download.${platform}.kind`,
      next,
    );
    pane.querySelector(".download-file").textContent =
      installerFilename(appVersion, platform);

    const button = pane.querySelector(".download-button");
    button.href = platformPrimaryUrl(platform);
    button.querySelector(".download-action").textContent = actionLabel;
    button.setAttribute(
      "aria-label",
      isCurrent ? `${actionLabel}, ${yours}` : actionLabel,
    );
  }
}

/** Primary action: the matching installer, or a plain download when we have none. */
function primaryDownloadLabel(next) {
  if (currentPlatform == null) return marketingText("hero.installer", next);
  const platformName = marketingText(`download.${currentPlatform}.name`, next);
  return marketingText("hero.installerPlatform", next).replaceAll(
    "{platform}",
    platformName,
  );
}

/** Write every locale-dependent piece; runs once before the first paint and
 * again on each language change. The embedded demo is never re-rendered, so
 * its state survives: it follows through the shared `ice-box.language` key. */
function applyCopy(next) {
  language = next;
  applyMarketingLanguage(language);
  for (const el of root.querySelectorAll("[data-i18n]")) {
    el.textContent = marketingText(el.dataset.i18n, language);
  }
  for (const el of root.querySelectorAll("[data-i18n-html]")) {
    el.innerHTML = marketingText(el.dataset.i18nHtml, language);
  }
  parts.heroDownloadLabel.textContent = primaryDownloadLabel(language);
  parts.heroReleaseLabel.textContent = marketingText("hero.release", language).replaceAll(
    "{version}",
    displayVersion,
  );
  applyDownloadCopy(language);
  parts.demoFrame.title = marketingText("demo.iframeTitle", language);
  parts.phoneDemoFrame.title = marketingText("phoneDemo.iframeTitle", language);
  syncThemeToggle();
  const languageAria = marketingText("nav.languageAria", language);
  parts.languageLabel.textContent = LANGUAGE_LABELS[language];
  parts.languageLabel.setAttribute("lang", language);
  parts.languageLabel.setAttribute("dir", languageDirection(language));
  parts.languageToggle.setAttribute("aria-label", languageAria);
  parts.languageMenu.setAttribute("aria-label", languageAria);
  for (const option of parts.languageOptions) {
    option.setAttribute(
      "aria-selected",
      option.dataset.language === language ? "true" : "false",
    );
  }
}

function isLanguageMenuOpen() {
  return !parts.languageMenu.hidden;
}

function openLanguageMenu({ focusSelected = false } = {}) {
  if (parts.languageMenu.hidden) {
    parts.languageMenu.hidden = false;
    parts.languageToggle.setAttribute("aria-expanded", "true");
  }
  if (focusSelected) {
    const active =
      parts.languageOptions.find((el) => el.dataset.language === language) ??
      parts.languageOptions[0];
    active?.focus();
  }
}

function closeLanguageMenu({ restoreFocus = false } = {}) {
  if (parts.languageMenu.hidden) return;
  parts.languageMenu.hidden = true;
  parts.languageToggle.setAttribute("aria-expanded", "false");
  if (restoreFocus) parts.languageToggle.focus();
}

function selectLanguage(next) {
  if (!isLanguagePreference(next)) return;
  try {
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, next);
  } catch {
    // Private mode / blocked storage: the pick still applies for this visit.
  }
  applyCopy(resolveLanguage(next));
  closeLanguageMenu({ restoreFocus: true });
}

parts.languageToggle.addEventListener("click", () => {
  if (isLanguageMenuOpen()) closeLanguageMenu();
  else openLanguageMenu();
});

parts.languageToggle.addEventListener("keydown", (event) => {
  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault();
    openLanguageMenu({ focusSelected: true });
  } else if (event.key === "Escape") {
    closeLanguageMenu();
  }
});

parts.languageMenu.addEventListener("click", (event) => {
  const option = event.target.closest(".nav-language-option");
  if (!option) return;
  selectLanguage(option.dataset.language);
});

parts.languageMenu.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    event.preventDefault();
    closeLanguageMenu({ restoreFocus: true });
    return;
  }
  if (event.key === "Tab") {
    // Restore focus to the trigger before hiding the menu so the browser's
    // default Tab/Shift+Tab continues cleanly from the trigger instead of
    // dropping focus to <body> when the focused option becomes hidden.
    closeLanguageMenu({ restoreFocus: true });
    return;
  }
  const options = parts.languageOptions;
  const currentIndex = options.indexOf(document.activeElement);
  if (currentIndex === -1) return;
  const cols =
    window.getComputedStyle(parts.languageMenu).gridTemplateColumns.split(" ")
      .length || 1;
  const rtl = document.documentElement.dir === "rtl";
  let nextIndex = null;
  if (event.key === "ArrowDown") {
    nextIndex = (currentIndex + cols) % options.length;
  } else if (event.key === "ArrowUp") {
    nextIndex = (currentIndex - cols + options.length) % options.length;
  } else if (event.key === "ArrowRight") {
    nextIndex =
      (currentIndex + (rtl ? -1 : 1) + options.length) % options.length;
  } else if (event.key === "ArrowLeft") {
    nextIndex =
      (currentIndex + (rtl ? 1 : -1) + options.length) % options.length;
  } else if (event.key === "Home") {
    nextIndex = 0;
  } else if (event.key === "End") {
    nextIndex = options.length - 1;
  }
  if (nextIndex !== null) {
    event.preventDefault();
    options[nextIndex]?.focus();
  }
});

document.addEventListener("pointerdown", (event) => {
  if (isLanguageMenuOpen() && !parts.languageWrap.contains(event.target)) {
    closeLanguageMenu();
  }
});

// The Live Demo writes `ice-box.language` and `ice-box.theme` when the visitor
// changes them there; a storage event never reaches the document that wrote it.
window.addEventListener("storage", (event) => {
  if (event.key === LANGUAGE_STORAGE_KEY) {
    if (!isLanguagePreference(event.newValue)) return;
    applyCopy(resolveLanguage(event.newValue));
    return;
  }
  if (event.key === THEME_STORAGE_KEY && (event.newValue == null || isThemePreference(event.newValue))) {
    applySiteTheme();
  }
});

applyCopy(language);

if (currentPlatform === "linux") {
  refineLinuxArch(navigator).then((refinedArch) => {
    if (refinedArch === detectedLinuxArch) return;
    detectedLinuxArch = refinedArch;
    parts.heroDownload.href = platformPrimaryUrl(currentPlatform);
    if (!linuxArchManuallySelected) {
      selectedLinuxArch = refinedArch;
      applyDownloadCopy(language);
    }
  });
}
