// SPDX-License-Identifier: GPL-3.0-or-later

import logoUrl from "../../desktop/src/assets/logo.png";
import { version as appVersion } from "../../desktop/package.json";
import {
  LANGUAGE_STORAGE_KEY,
  isLanguagePreference,
  resolveLanguage,
} from "../../desktop/src/lib/language";
import {
  THEME_STORAGE_KEY,
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
import "./style.css";
import "./phone-demo.css";

/** Same source as the desktop app and the Live Demo title bar (`APP_VERSION`). */
const displayVersion = `v${appVersion}`;

const favicon = document.createElement("link");
favicon.rel = "icon";
favicon.type = "image/png";
favicon.href = logoUrl;
document.head.appendChild(favicon);

/** Resolved before the first render, so the first paint is already localized
 * (the static index.html keeps its English copy as the no-JS fallback). */
let language = resolveInitialLanguage();

const root = document.querySelector("#root");

root.innerHTML = `
  <header class="site-nav">
    <a class="brand" href="#top"><img src="${logoUrl}" alt="ice-box logo"><span>ice-box</span></a>
    <div class="nav-actions">
      <a class="nav-github" href="https://github.com/yilong-musk/ice-box" target="_blank" rel="noreferrer">GitHub ↗</a>
      <button class="nav-language" type="button"></button>
    </div>
  </header>
  <main id="top">
    <section class="hero page-wrap">
      <div class="hero-copy">
        <h1></h1>
      </div>
      <div class="hero-aside">
        <div class="hero-actions">
          <a class="button button-primary" href="#download"><span class="button-label"></span> <span aria-hidden="true">↓</span></a>
          <a class="text-link" href="#download"></a>
        </div>
        <div class="hero-meta">
          <span><b class="status-dot green"></b> ${displayVersion}</span>
          <span>GPL-3.0-or-later</span>
          <span>macOS · Windows · Android</span>
        </div>
      </div>
    </section>
    <section class="demo-section page-wrap" id="demo"><div class="macbook"><div class="macbook-lid"><div class="macbook-screen"><div class="demo-screen"><span class="demo-window-lights" aria-hidden="true"><i></i><i></i><i></i></span><iframe src="./demo.html"></iframe></div></div><span class="macbook-camera" aria-hidden="true"></span></div><div class="macbook-base" aria-hidden="true"></div></div><p class="demo-caption"><strong>ice-box</strong><span class="demo-caption-label"></span><span>${displayVersion}</span></p></section>
    <section class="phone-demo page-wrap" id="android-demo" aria-labelledby="phone-demo-title">
      <div class="phone-demo-copy">
        <p class="phone-demo-kicker"></p>
        <h2 id="phone-demo-title"></h2>
        <p class="phone-demo-body"></p>
      </div>
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
              <iframe src="./mobile-demo.html" title="ice-box Android UI demo" loading="lazy"></iframe>
              <span class="phone-gesture" aria-hidden="true"></span>
            </div>
            <span class="phone-camera" aria-hidden="true"></span>
          </div>
        </div>
      </div>
    </section>
    <section class="download page-wrap" id="download" aria-labelledby="download-title">
      <div class="download-head">
        <div>
          <p class="download-kicker"></p>
          <h2 id="download-title"></h2>
        </div>
        <p class="download-version">${displayVersion}</p>
      </div>
      <ul class="download-list">
        ${INSTALLER_PLATFORMS.map(
          (platform) => `<li class="download-row" data-platform="${platform}">
          <div class="download-identity">
            <p class="download-name"><span class="download-platform"></span><span class="download-yours" hidden></span></p>
            <p class="download-file"></p>
          </div>
          <p class="download-note"></p>
          <a class="button download-button" href="${installerDownloadUrl(appVersion, platform)}"><span class="download-action"></span><span aria-hidden="true">↓</span></a>
        </li>`,
        ).join("")}
      </ul>
    </section>
  </main>
  <footer class="site-footer page-wrap"><span>© 2026 ice-box</span><a href="https://github.com/yilong-musk/ice-box" target="_blank" rel="noreferrer">github.com/yilong-musk/ice-box ↗</a></footer>
`;

const parts = {
  headline: root.querySelector(".hero-copy h1"),
  downloadLabel: root.querySelector(".button-label"),
  moreDownloads: root.querySelector(".text-link"),
  downloadKicker: root.querySelector(".download-kicker"),
  downloadTitle: root.querySelector("#download-title"),
  demoCaption: root.querySelector(".demo-caption-label"),
  demoFrame: root.querySelector(".demo-screen iframe"),
  phoneDemoKicker: root.querySelector(".phone-demo-kicker"),
  phoneDemoTitle: root.querySelector("#phone-demo-title"),
  phoneDemoBody: root.querySelector(".phone-demo-body"),
  phoneDemoFrame: root.querySelector(".phone-viewport iframe"),
  phoneFrame: root.querySelector(".phone-frame"),
  phoneClock: root.querySelector(".phone-clock"),
  languageToggle: root.querySelector(".nav-language"),
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

applyPhoneFrameTheme();
window
  .matchMedia?.("(prefers-color-scheme: dark)")
  .addEventListener?.("change", applyPhoneFrameTheme);

/** Platform names, first-launch notes, and which row matches this browser. */
function applyDownloadCopy(next) {
  for (const platform of INSTALLER_PLATFORMS) {
    const row = root.querySelector(`[data-platform="${platform}"]`);
    const isCurrent = platform === currentPlatform;
    row.classList.toggle("is-current", isCurrent);
    const name = marketingText(`download.${platform}.name`, next);
    const yours = marketingText("download.yours", next);
    const action = marketingText("download.action", next);
    row.querySelector(".download-platform").textContent = name;
    const yoursEl = row.querySelector(".download-yours");
    yoursEl.textContent = yours;
    yoursEl.hidden = !isCurrent;
    row.querySelector(".download-file").textContent =
      `${marketingText(`download.${platform}.kind`, next)} · ${installerFilename(appVersion, platform)}`;
    row.querySelector(".download-note").textContent = marketingText(
      `download.${platform}.note`,
      next,
    );
    const button = row.querySelector(".download-button");
    button.querySelector(".download-action").textContent = action;
    button.setAttribute(
      "aria-label",
      isCurrent ? `${action} ${name}, ${yours}` : `${action} ${name}`,
    );
  }
}

/** Primary action: the matching installer, or a plain download when we have none. */
function primaryDownloadLabel(next) {
  if (currentPlatform == null) return marketingText("hero.installer", next);
  const platformName = marketingText(
    `download.${currentPlatform}.name`,
    next,
  );
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
  parts.headline.innerHTML = marketingText("hero.title", language);
  parts.downloadLabel.textContent = primaryDownloadLabel(language);
  parts.moreDownloads.textContent = marketingText("hero.more", language);
  parts.downloadKicker.textContent = marketingText("download.kicker", language);
  parts.downloadTitle.textContent = marketingText("download.title", language);
  applyDownloadCopy(language);
  parts.demoCaption.textContent = marketingText("demo.windowBar", language);
  parts.demoFrame.title = marketingText("demo.iframeTitle", language);
  parts.phoneDemoKicker.textContent = marketingText("phoneDemo.kicker", language);
  parts.phoneDemoTitle.textContent = marketingText("phoneDemo.title", language);
  parts.phoneDemoBody.textContent = marketingText("phoneDemo.body", language);
  parts.phoneDemoFrame.title = marketingText(
    "phoneDemo.iframeTitle",
    language,
  );
  parts.languageToggle.textContent = marketingText(
    "nav.languageToggle",
    language,
  );
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
  if (event.key === THEME_STORAGE_KEY) applyPhoneFrameTheme();
});

applyCopy(language);
