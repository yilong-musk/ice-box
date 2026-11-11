// SPDX-License-Identifier: GPL-3.0-or-later

import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import logoUrl from "../../desktop/src/assets/logo.png";
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
import "./phone-demo.css";
import "./style.css";

/** Same source as the desktop app and the Live Demo title bar (`APP_VERSION`). */
const displayVersion = `v${appVersion}`;
const REPO_URL = "https://github.com/yilong-musk/ice-box";

const favicon = document.createElement("link");
favicon.rel = "icon";
favicon.type = "image/png";
favicon.href = logoUrl;
document.head.appendChild(favicon);

/** Resolved before the first render, so the first paint is already localized
 * (the static index.html keeps its English copy as the no-JS fallback). */
let language = resolveInitialLanguage();

/** Lucide-style 24px stroke icons, inlined so the page needs no icon font. */
const ICON_PATHS = {
  download: '<path d="M12 15V3"/><path d="m7 10 5 5 5-5"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>',
  play: '<path d="m7 4 13 8-13 8V4Z"/>',
  arrowRight: '<path d="M5 12h14"/><path d="m13 6 6 6-6 6"/>',
  external: '<path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>',
  github: '<path d="M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.4 5.4 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4"/><path d="M9 18c-4.51 2-5-2-7-2"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  rss: '<path d="M4 11a9 9 0 0 1 9 9"/><path d="M4 4a16 16 0 0 1 16 16"/><circle cx="5" cy="19" r="1"/>',
  split: '<path d="M16 3h5v5"/><path d="M8 3H3v5"/><path d="M12 22v-8.3a4 4 0 0 0-1.17-2.83L3 3"/><path d="m15 9 6-6"/>',
  server: '<rect x="2" y="3" width="20" height="8" rx="2"/><rect x="2" y="13" width="20" height="8" rx="2"/><path d="M6 7h.01"/><path d="M6 17h.01"/>',
  activity: '<path d="M22 12h-4l-3 9L9 3l-3 9H2"/>',
  shield: '<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/><path d="m9 12 2 2 4-4"/>',
  panel: '<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 9h18"/><path d="M7 6h.01"/><path d="M11 6h.01"/>',
  sparkles: '<path d="M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z"/><path d="M19 15l.9 2.1L22 18l-2.1.9L19 21l-.9-2.1L16 18l2.1-.9z"/>',
  bot: '<path d="M12 8V4H8"/><rect x="4" y="8" width="16" height="12" rx="2"/><path d="M2 14h2"/><path d="M20 14h2"/><path d="M15 13v2"/><path d="M9 13v2"/>',
  laptop: '<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M2 20h20"/>',
  monitor: '<rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8"/><path d="M12 17v4"/>',
  smartphone: '<rect x="6" y="2" width="12" height="20" rx="2.5"/><path d="M11 18h2"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/>',
  moon: '<path d="M20.99 12.79A9 9 0 1 1 11.21 3a7 7 0 0 0 9.78 9.79Z"/>',
};

function icon(name, className = "icon") {
  return `<svg class="${className}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICON_PATHS[name]}</svg>`;
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
  <div class="backdrop" aria-hidden="true"><span class="glow glow-a"></span><span class="glow glow-b"></span><span class="glow glow-c"></span><span class="grid"></span></div>
  <header class="site-nav">
    <div class="nav-inner page-wrap">
      <a class="brand" href="#top"><img src="${logoUrl}" alt="" width="28" height="28"><span>ice-box</span></a>
      <nav class="nav-links" aria-label="Primary">
        <a href="#features" ${t("nav.features")}></a>
        <a href="#demo" ${t("nav.demo")}></a>
        <a href="#agent" ${t("nav.agent")}></a>
        <a href="#download" ${t("nav.download")}></a>
      </nav>
      <div class="nav-actions">
        <a class="icon-button" href="${REPO_URL}" target="_blank" rel="noreferrer" aria-label="GitHub">${icon("github")}</a>
        <button class="icon-button nav-theme" type="button"><span class="theme-icon theme-icon-dark">${icon("moon")}</span><span class="theme-icon theme-icon-light">${icon("sun")}</span></button>
        <button class="nav-language" type="button"></button>
        <a class="button button-primary button-small nav-download" href="#download">${icon("download")}<span ${t("nav.download")}></span></a>
      </div>
    </div>
  </header>
  <main id="top">
    <section class="hero page-wrap">
      <a class="hero-badge reveal" href="${REPO_URL}" target="_blank" rel="noreferrer"><span class="pulse-dot"></span><span class="hero-badge-version">${displayVersion}</span><span ${t("hero.badge")}></span>${icon("arrowRight", "icon icon-xs")}</a>
      <h1 class="reveal" data-i18n-html="hero.title"></h1>
      <p class="hero-subtitle reveal" ${t("hero.subtitle")}></p>
      <div class="hero-actions reveal">
        <a class="button button-primary button-large hero-download" href="#download">${icon("download")}<span class="hero-download-label"></span></a>
        <a class="button button-ghost button-large" href="#demo">${icon("play")}<span ${t("hero.tryDemo")}></span></a>
      </div>
      <p class="hero-meta reveal"><span ${t("hero.platforms")}></span><span class="dot-sep"></span><span>GPL-3.0</span><span class="dot-sep"></span><span>sing-box</span></p>
    </section>

    <section class="demo-section page-wrap" id="demo">
      <div class="showcase">
        <div class="macbook reveal"><div class="macbook-lid"><div class="macbook-screen"><div class="demo-screen"><span class="demo-window-lights" aria-hidden="true"><i></i><i></i><i></i></span><iframe src="./demo.html" loading="lazy"></iframe></div></div><span class="macbook-camera" aria-hidden="true"></span></div><div class="macbook-base" aria-hidden="true"></div></div>
        <div class="phone-float reveal" id="android-demo">
        <div class="phone-demo-stage">
          <div class="phone-frame">
            <span class="phone-sim" aria-hidden="true"></span>
            <div class="phone-screen">
              <div class="phone-status-bar" aria-hidden="true">
                <span class="phone-clock"></span>
                <div class="phone-status-icons">
                  <svg width="15" height="15" viewBox="0 0 24 24" fill="none"><path d="M12.65 10A5.99 5.99 0 0 0 7 6c-3.31 0-6 2.69-6 6s2.69 6 6 6a5.99 5.99 0 0 0 5.65-4H17v4h4v-4h2v-4H12.65ZM7 14c-1.1 0-2-.9-2-2s.9-2 2-2 2 .9 2 2-.9 2-2 2Z" fill="currentColor"/></svg>
                  <svg width="14" height="12" viewBox="0 0 14 12" fill="none"><path d="M1 4.2C2.6 2.8 4.7 2 7 2s4.4.8 6 2.2" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><path d="M3.1 6.7C4.2 5.7 5.5 5.1 7 5.1s2.8.6 3.9 1.6" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><path d="M5.2 9.1C5.8 8.5 6.4 8.2 7 8.2s1.2.3 1.8.9" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><circle cx="7" cy="10.4" r="0.9" fill="currentColor"/></svg>
                  <svg width="22" height="11" viewBox="0 0 22 11" fill="none"><rect x="0.5" y="0.5" width="19" height="10" rx="2.5" stroke="currentColor" stroke-opacity="0.85"/><rect x="2" y="2" width="14.5" height="7" rx="1.4" fill="currentColor"/><rect x="20.2" y="3.5" width="1.5" height="4" rx="0.75" fill="currentColor" fill-opacity="0.85"/></svg>
                </div>
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
          <p class="term-line term-call"><span class="term-tool">ice-box.list_connections</span><span class="term-args">{"limit":3}</span><span class="term-ok">${icon("check", "icon icon-xs")}</span></p>
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
      <div class="footer-brand">
        <a class="brand" href="#top"><img src="${logoUrl}" alt="" width="24" height="24"><span>ice-box</span></a>
      </div>
      <nav class="footer-links" aria-label="Footer">
        <a href="${REPO_URL}" target="_blank" rel="noreferrer">${icon("github", "icon icon-xs")}<span ${t("footer.source")}></span></a>
        <a href="${REPO_URL}/releases" target="_blank" rel="noreferrer"><span ${t("footer.releases")}></span></a>
        <a href="${REPO_URL}/blob/main/CHANGELOG.md" target="_blank" rel="noreferrer"><span ${t("footer.changelog")}></span></a>
      </nav>
      <p class="footer-legal"><span>© 2026 ice-box</span><span class="dot-sep"></span><span ${t("footer.license")}></span><span class="dot-sep"></span><span>${displayVersion}</span></p>
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

/** The phone frame draws Android system chrome, so its status bar follows the
 * same stored theme the embedded app uses. */
function applyPhoneFrameTheme() {
  parts.phoneFrame.dataset.theme = resolveTheme(readThemePreference());
}

/**
 * Site theme. The page shares `ice-box.theme` with the Live Demo, so a pick
 * here also switches both embedded apps (they bridge the storage event), and
 * a pick inside a demo switches the page. `index.html` sets `data-theme`
 * before the first paint; this keeps it current.
 */
function siteTheme() {
  return resolveTheme(readThemePreference());
}

function themeToggleLabel(next) {
  return marketingText(
    siteTheme() === "dark" ? "nav.themeToLight" : "nav.themeToDark",
    next,
  );
}

function applySiteTheme() {
  const theme = siteTheme();
  document.documentElement.dataset.theme = theme;
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute("content", theme === "dark" ? "#07090b" : "#f6f8f7");
  parts.themeToggle.setAttribute("aria-label", themeToggleLabel(language));
  parts.themeToggle.title = themeToggleLabel(language);
  applyPhoneFrameTheme();
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
  } catch {
    // Blocked storage: the page switches, the demos keep their own theme.
    document.documentElement.dataset.theme = next;
    return;
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
  parts.themeToggle.setAttribute("aria-label", themeToggleLabel(language));
  parts.themeToggle.title = themeToggleLabel(language);
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
