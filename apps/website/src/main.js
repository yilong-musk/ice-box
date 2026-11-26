// SPDX-License-Identifier: GPL-3.0-or-later

import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import faviconUrl from "../../desktop/public/favicon.png";
import runningStill from "../../desktop/src/assets/switch-logo/32/running-still.png";
import runningStrip from "../../desktop/src/assets/switch-logo/32/running.png";
import { version as appVersion } from "../../desktop/package.json";
import {
  LANGUAGE_STORAGE_KEY,
  isLanguagePreference,
  resolveLanguage,
} from "../../desktop/src/lib/language";
import {
  THEME_STORAGE_KEY,
  isThemePreference,
  readThemePreference,
  resolveTheme,
} from "../../desktop/src/lib/theme";
import {
  INSTALLER_PLATFORMS,
  detectLinuxArch,
  detectPlatform,
  installerDownloadUrl,
  installerFilename,
  linuxInstallerDownloadUrl,
  linuxInstallerFilename,
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
  check: '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
  laptop: '<rect x="4.5" y="5" width="15" height="10.5" rx="1.75"/><path d="M2.5 19h19"/>',
  monitor: '<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M12 16v4M8 20h8"/>',
  terminal: '<polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/>',
  smartphone: '<rect x="6.5" y="2.75" width="11" height="18.5" rx="2.75"/><path d="M11 18h2"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M4.6 4.6l1.4 1.4M18 18l1.4 1.4M2.5 12h2M19.5 12h2M4.6 19.4 6 18M18 6l1.4-1.4"/>',
  moon: '<path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z"/>',
  github:
    '<path d="M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.4 5.4 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4"/><path d="M9 18c-4.51 2-5-2-7-2"/>',
};

function icon(name, className = "icon") {
  return `<svg class="${className}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICONS[name]}</svg>`;
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

const PLATFORM_ICON = {
  macos: "laptop",
  windows: "monitor",
  linux: "terminal",
  android: "smartphone",
};

const LINUX_VARIANT_LINKS = [
  ["appimage-amd64", "AppImage · x64"],
  ["deb-amd64", ".deb · x64"],
  ["appimage-aarch64", "AppImage · arm64"],
  ["deb-arm64", ".deb · arm64"],
];

const windowLights = `<span class="window-lights" aria-hidden="true"><i></i><i></i><i></i></span>`;

const currentPlatform = detectPlatform(navigator.userAgent, navigator.maxTouchPoints);
let linuxPrimaryVariant = linuxPrimaryInstallerVariant(
  detectLinuxArch(navigator.userAgent),
);

function platformPrimaryFilename(platform) {
  return platform === "linux"
    ? linuxInstallerFilename(appVersion, linuxPrimaryVariant)
    : installerFilename(appVersion, platform);
}

function platformPrimaryUrl(platform) {
  return platform === "linux"
    ? linuxInstallerDownloadUrl(appVersion, linuxPrimaryVariant)
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
        <button class="nav-language" type="button"></button>
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
                <div class="phone-status-bar" aria-hidden="true">
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
          <div class="tool-calls">
            <p class="tool-call"><span class="tool-name">ice-box.set_proxy_mode</span><span class="tool-args">{"mode":"global"}</span><span class="tool-ok">${icon("check", "icon icon-xs")}</span></p>
            <p class="tool-call"><span class="tool-name">ice-box.list_connections</span><span class="tool-args">{}</span><span class="tool-ok">${icon("check", "icon icon-xs")}</span></p>
          </div>
          <p class="bubble bubble-agent" ${t("agent.reply")}></p>
          <p class="config"><span class="cfg-comment">// mcpServers.ice-box</span><br><span class="cfg-key">"command"</span>: <span class="cfg-str">".../ice-box"</span>,<br><span class="cfg-key">"args"</span>: [<span class="cfg-str">"--mcp-stdio"</span>, <span class="cfg-str">"--data-dir"</span>, <span class="cfg-str">"..."</span>]</p>
        </div>
      </div>
    </section>

    <section class="section page-wrap download" id="download" aria-labelledby="download-title">
      <div class="section-head section-head-center reveal">
        <p class="kicker" ${t("download.kicker")}></p>
        <h2 id="download-title" ${t("download.title")}></h2>
        <p class="section-body" ${t("download.body")}></p>
      </div>
      <ul class="download-grid">
        ${INSTALLER_PLATFORMS.map(
          (platform) => `<li class="download-card reveal" data-platform="${platform}">
          <div class="download-top">
            <span class="download-icon">${icon(PLATFORM_ICON[platform])}</span>
            <span class="download-yours" hidden></span>
          </div>
          <h3 class="download-name"></h3>
          <p class="download-file"></p>
          <p class="download-requires"></p>
          <p class="download-note"></p>
          <a class="button download-button" href="${platformPrimaryUrl(platform)}">${icon("download")}<span class="download-action"></span></a>
          ${
            platform === "linux"
              ? `<div class="download-variants">${LINUX_VARIANT_LINKS.map(
                  ([variant, label]) =>
                    `<a class="download-variant" href="${linuxInstallerDownloadUrl(appVersion, variant)}">${label}</a>`,
                ).join("")}</div>`
              : ""
          }
        </li>`,
        ).join("")}
      </ul>
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
  languageToggle: root.querySelector(".nav-language"),
  themeToggle: root.querySelector(".nav-theme"),
  nav: root.querySelector(".site-nav"),
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

/** Platform names, requirements, first-launch notes, and which card matches
 * this browser. */
function applyDownloadCopy(next) {
  for (const platform of INSTALLER_PLATFORMS) {
    const card = root.querySelector(`[data-platform="${platform}"]`);
    const isCurrent = platform === currentPlatform;
    card.classList.toggle("is-current", isCurrent);
    const name = marketingText(`download.${platform}.name`, next);
    const yours = marketingText("download.yours", next);
    const action = marketingText("download.action", next);
    card.querySelector(".download-name").textContent = name;
    const yoursEl = card.querySelector(".download-yours");
    yoursEl.textContent = yours;
    yoursEl.hidden = !isCurrent;
    card.querySelector(".download-file").textContent =
      `${marketingText(`download.${platform}.kind`, next)} · ${platformPrimaryFilename(platform)}`;
    card.querySelector(".download-requires").textContent =
      marketingText(`download.${platform}.requires`, next);
    card.querySelector(".download-note").textContent = marketingText(
      `download.${platform}.note`,
      next,
    );
    const button = card.querySelector(".download-button");
    button.querySelector(".download-action").textContent = `${action} ${name}`;
    button.setAttribute(
      "aria-label",
      isCurrent ? `${action} ${name}, ${yours}` : `${action} ${name}`,
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
  parts.languageToggle.textContent = marketingText("nav.languageToggle", language);
  parts.languageToggle.setAttribute(
    "aria-label",
    marketingText("nav.languageToggleAria", language),
  );
}

parts.languageToggle.addEventListener("click", () => {
  const next = language === "zh" ? "en" : "zh";
  try {
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, next);
  } catch {
    // Private mode / blocked storage: the pick still applies for this visit.
  }
  applyCopy(next);
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
    const nextVariant = linuxPrimaryInstallerVariant(refinedArch);
    if (nextVariant === linuxPrimaryVariant) return;
    linuxPrimaryVariant = nextVariant;
    parts.heroDownload.href = platformPrimaryUrl(currentPlatform);
    const linuxCardButton = root.querySelector('[data-platform="linux"] .download-button');
    if (linuxCardButton) {
      linuxCardButton.href = platformPrimaryUrl("linux");
    }
    applyDownloadCopy(language);
  });
}
