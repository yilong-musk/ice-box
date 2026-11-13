// SPDX-License-Identifier: GPL-3.0-or-later

import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import faviconUrl from "../../desktop/public/favicon.png";
import runningStill from "../../desktop/src/assets/switch-logo/running-still.png";
import runningStrip from "../../desktop/src/assets/switch-logo/running.png";
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
  detectPlatform,
  installerDownloadUrl,
  installerFilename,
} from "./downloads";
import {
  applyMarketingLanguage,
  marketingText,
  resolveInitialLanguage,
} from "./i18n";
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

/** 16×16 pixel sprites, one cell per character, in the same 1px grid as the
 * switch logo. `#` is a filled pixel. Drawn here so the page needs no icon font. */
const PIXEL_ICONS = {
  download: [
    "................",
    ".......##.......",
    ".......##.......",
    ".......##.......",
    "......####......",
    ".....##..##.....",
    "....##....##....",
    ".......##.......",
    "................",
    "...##########...",
    "...#........#...",
    "...##########...",
    "................",
    "................",
    "................",
    "................",
  ],
  play: [
    "................",
    "................",
    ".....##.........",
    ".....###........",
    ".....####.......",
    ".....#####......",
    ".....######.....",
    ".....#####......",
    ".....####.......",
    ".....###........",
    ".....##.........",
    "................",
    "................",
    "................",
    "................",
    "................",
  ],
  arrowRight: [
    "................",
    "................",
    "................",
    "................",
    "......##........",
    "......###.......",
    "..########......",
    "......###.......",
    "......##........",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
  ],
  check: [
    "................",
    "................",
    "................",
    "............##..",
    "...........##...",
    "..........##....",
    "..##.....##.....",
    "...##...##......",
    "....##.##.......",
    ".....###........",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
  ],
  rss: [
    "................",
    "................",
    "..############..",
    "..#..........#..",
    "..#..######..#..",
    "..#..........#..",
    "..#..####....#..",
    "..#..........#..",
    "..#..######..#..",
    "..#..........#..",
    "..############..",
    "................",
    "................",
    "................",
    "................",
    "................",
  ],
  split: [
    "................",
    "................",
    "..##........##..",
    "...##......##...",
    "....##....##....",
    ".....##..##.....",
    "......####......",
    ".......##.......",
    ".......##.......",
    ".......##.......",
    ".......##.......",
    ".......##.......",
    "................",
    "................",
    "................",
    "................",
  ],
  server: [
    "................",
    "................",
    "................",
    "..############..",
    "..#.##.....##.#.",
    "..############..",
    "................",
    "..############..",
    "..#.##.....##.#.",
    "..############..",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
  ],
  activity: [
    "................",
    "................",
    "................",
    "................",
    "................",
    "............##..",
    "...........##...",
    "..........##....",
    "..##########....",
    "..........##....",
    "...........##...",
    "............##..",
    "................",
    "................",
    "................",
    "................",
  ],
  shield: [
    "................",
    "................",
    "....########....",
    "...##......##...",
    "..##........##..",
    "..##........##..",
    "..##........##..",
    "...##......##...",
    "....##....##....",
    ".....##..##.....",
    "......####......",
    ".......##.......",
    "................",
    "................",
    "................",
    "................",
  ],
  panel: [
    "................",
    "................",
    "..############..",
    "..#........#.#..",
    "..############..",
    "..#..........#..",
    "..#..........#..",
    "..#..........#..",
    "..#..........#..",
    "..#..........#..",
    "..############..",
    "................",
    "................",
    "................",
    "................",
    "................",
  ],
  sparkles: [
    "................",
    "................",
    "................",
    "................",
    "...........##...",
    "..........##....",
    ".........##.....",
    "..##....##......",
    "...##..##.......",
    "....####........",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
  ],
  bot: [
    "................",
    ".......##.......",
    ".......##.......",
    "....########....",
    "...##..##..##...",
    "...##..##..##...",
    "....########....",
    "......#..#......",
    ".....##..##.....",
    "....##....##....",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
  ],
  laptop: [
    "................",
    "..############..",
    "..#..........#..",
    "..#..........#..",
    "..#..........#..",
    "..#..........#..",
    "..#..........#..",
    "..#..........#..",
    "..#..........#..",
    "..#..........#..",
    "..############..",
    ".##############.",
    "......####......",
    "......####......",
    "................",
    "................",
  ],
  monitor: [
    "..############..",
    "..#..........#..",
    "..#..........#..",
    "..#..........#..",
    "..#..........#..",
    "..#..........#..",
    "..#..........#..",
    "..#..........#..",
    "..############..",
    ".......##.......",
    ".......##.......",
    ".......##.......",
    ".....######.....",
    ".....######.....",
    "................",
    "................",
  ],
  smartphone: [
    "................",
    "....########....",
    "....#......#....",
    "....#......#....",
    "....#......#....",
    "....#......#....",
    "....#......#....",
    "....#......#....",
    "....#......#....",
    "....#......#....",
    "....#......#....",
    "....#..##..#....",
    "....#......#....",
    "....########....",
    "................",
    "................",
  ],
  sun: [
    ".......##.......",
    "................",
    "..##........##..",
    "....########....",
    "...##########...",
    "...##########...",
    "##.##########.##",
    "...##########...",
    "...##########...",
    "....########....",
    "..##........##..",
    "................",
    ".......##.......",
    "................",
    "................",
    "................",
  ],
  moon: [
    "................",
    ".....######.....",
    "...####.........",
    "..###...........",
    ".###............",
    ".###............",
    ".###............",
    "..###...........",
    "...####.........",
    ".....######.....",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
  ],
};

/** The mark stays the original stroke icon; a 16px octocat does not read. */
const STROKE_ICONS = {
  github:
    '<path d="M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.4 5.4 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4"/><path d="M9 18c-4.51 2-5-2-7-2"/>',
};

function icon(name, className = "icon") {
  const stroke = STROKE_ICONS[name];
  if (stroke) {
    return `<svg class="${className}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${stroke}</svg>`;
  }
  const rows = PIXEL_ICONS[name];
  const rects = [];
  for (let y = 0; y < rows.length; y++) {
    const row = rows[y];
    for (let x = 0; x < row.length; x++) {
      if (row[x] === "#") rects.push(`<rect x="${x}" y="${y}" width="1" height="1"/>`);
    }
  }
  return `<svg class="${className} pixel-icon" viewBox="0 0 16 16" fill="currentColor" shape-rendering="crispEdges" aria-hidden="true">${rects.join("")}</svg>`;
}

/** `data-i18n` fills textContent, `data-i18n-html` fills trusted markup from
 * the site's own dictionary (the hero title carries an accent span). */
const t = (key) => `data-i18n="${key}"`;

const FEATURES = [
  ["subs", "rss"],
  ["routing", "split"],
  ["nodes", "server"],
  ["connections", "activity"],
  ["capture", "shield"],
  ["tray", "panel"],
  ["upkeep", "sparkles"],
];

const PLATFORM_ICON = { macos: "laptop", windows: "monitor", android: "smartphone" };

const root = document.querySelector("#root");

root.innerHTML = `
  <div class="backdrop" aria-hidden="true"><span class="grid"></span><span class="mote"></span><span class="mote"></span><span class="mote"></span><span class="mote"></span><span class="mote"></span><span class="mote"></span></div>
  <header class="site-nav">
    <div class="nav-inner page-wrap">
      <a class="brand" href="#top"><span class="brand-logo" aria-hidden="true" style="--brand-logo-still: url('${runningStill}'); --brand-logo-strip: url('${runningStrip}')"></span><span class="brand-name">ice-box</span></a>
      <div class="nav-actions">
        <a class="icon-button" href="${REPO_URL}" target="_blank" rel="noreferrer" aria-label="GitHub">${icon("github")}</a>
        <button class="icon-button nav-theme" type="button"><span class="theme-icon theme-icon-dark">${icon("moon")}</span><span class="theme-icon theme-icon-light">${icon("sun")}</span></button>
        <button class="nav-language" type="button"></button>
      </div>
    </div>
  </header>
  <main id="top">
    <section class="hero page-wrap">
      <h1 class="reveal" data-i18n-html="hero.title"></h1>
      <p class="hero-subtitle reveal" ${t("hero.subtitle")}></p>
      <div class="hero-actions reveal">
        <a class="button button-primary button-large hero-download" href="#download">${icon("download")}<span class="hero-download-label"></span></a>
        <a class="button button-ghost button-large" href="#demo">${icon("play")}<span ${t("hero.tryDemo")}></span></a>
      </div>
      <p class="hero-meta reveal"><span ${t("hero.platforms")}></span><span class="dot-sep"></span><span>GPL-3.0-or-later</span><span class="dot-sep"></span><span>sing-box</span></p>
    </section>

    <section class="demo-section page-wrap" id="demo">
      <div class="showcase">
        <div class="macbook reveal"><div class="macbook-lid"><div class="macbook-screen"><div class="demo-screen"><span class="demo-window-lights" aria-hidden="true"><i></i><i></i><i></i></span><iframe src="./demo.html" loading="lazy"></iframe></div></div></div><div class="macbook-base" aria-hidden="true"></div></div>
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
      <p class="demo-caption"><span class="pulse-dot"></span><strong>ice-box</strong><span ${t("demo.windowBar")}></span><span>${displayVersion}</span></p>
    </section>

    <section class="section page-wrap" id="features" aria-labelledby="features-title">
      <div class="section-head reveal">
        <p class="kicker" ${t("features.kicker")}></p>
        <h2 id="features-title" ${t("features.title")}></h2>
        <p class="section-body" ${t("features.body")}></p>
      </div>
      <div class="feature-grid">
        ${FEATURES.map(
          ([key, iconName]) => `<article class="feature-card reveal" data-feature="${key}">
          <span class="feature-icon">${icon(iconName)}</span>
          <h3 ${t(`features.${key}.title`)}></h3>
          <p ${t(`features.${key}.body`)}></p>
        </article>`,
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
      <div class="terminal reveal" role="img" aria-labelledby="agent-terminal-title">
        <div class="terminal-bar"><span class="terminal-lights" aria-hidden="true"><i></i><i></i><i></i></span><span id="agent-terminal-title" class="terminal-title">${icon("bot", "icon icon-xs")}<span ${t("agent.terminalTitle")}></span></span></div>
        <div class="terminal-body">
          <p class="term-line term-user"><span class="term-prompt">›</span><span ${t("agent.prompt")}></span></p>
          <p class="term-line term-call"><span class="term-tool">ice-box.set_proxy_mode</span><span class="term-args">{"mode":"global"}</span><span class="term-ok">${icon("check", "icon icon-xs")}</span></p>
          <p class="term-line term-call"><span class="term-tool">ice-box.list_connections</span><span class="term-args">{}</span><span class="term-ok">${icon("check", "icon icon-xs")}</span></p>
          <p class="term-line term-reply" ${t("agent.reply")}></p>
          <p class="term-line term-config"><span class="term-comment">// mcpServers.ice-box</span><br><span class="term-key">"command"</span>: <span class="term-str">".../ice-box"</span>,<br><span class="term-key">"args"</span>: [<span class="term-str">"--mcp-stdio"</span>, <span class="term-str">"--data-dir"</span>, <span class="term-str">"..."</span>]</p>
        </div>
      </div>
    </section>
    <section class="section page-wrap download" id="download" aria-labelledby="download-title">
      <div class="section-head reveal">
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
          <a class="button download-button" href="${installerDownloadUrl(appVersion, platform)}">${icon("download")}<span class="download-action"></span></a>
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
  heroDownloadLabel: root.querySelector(".hero-download-label"),
  demoFrame: root.querySelector(".demo-screen iframe"),
  phoneDemoFrame: root.querySelector(".phone-viewport iframe"),
  phoneFrame: root.querySelector(".phone-frame"),
  phoneClock: root.querySelector(".phone-clock"),
  languageToggle: root.querySelector(".nav-language"),
  themeToggle: root.querySelector(".nav-theme"),
  nav: root.querySelector(".site-nav"),
};

const currentPlatform = detectPlatform(navigator.userAgent);

/**
 * The Live Demo renders the desktop app at its default 860x620 window size and
 * scales that window to the available screen width, so the demo matches the
 * app and the README screenshots. Below 1100px the laptop shell is dropped and
 * the demo uses the full container width instead.
 */
const DEMO_WINDOW = { width: 860, height: 620 };
const DEMO_SHELL_MIN_WIDTH = 1100;

function fitDemoFrame() {
  const frame = parts.demoFrame;
  if (window.matchMedia(`(max-width: ${DEMO_SHELL_MIN_WIDTH}px)`).matches) {
    frame.style.zoom = "";
    return;
  }
  frame.style.zoom = String(frame.parentElement.clientWidth / DEMO_WINDOW.width);
}

fitDemoFrame();
window.addEventListener("resize", fitDemoFrame);

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

function applySiteTheme() {
  const appTheme = storedTheme();
  const theme = sessionTheme ?? appTheme;
  document.documentElement.dataset.theme = theme;
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute("content", theme === "dark" ? "#0b1020" : "#e7f4fb");
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

/** The nav gains a solid backdrop once the page scrolls under it. */
function updateNavState() {
  parts.nav.classList.toggle("is-scrolled", window.scrollY > 8);
}

updateNavState();
window.addEventListener("scroll", updateNavState, { passive: true });

/** Sections fade in as they enter the viewport. Without IntersectionObserver,
 * or with reduced motion, everything is shown at once. */
function setUpReveal() {
  const items = root.querySelectorAll(".reveal");
  const reduced = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
  if (reduced || typeof IntersectionObserver !== "function") {
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
      `${marketingText(`download.${platform}.kind`, next)} · ${installerFilename(appVersion, platform)}`;
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
