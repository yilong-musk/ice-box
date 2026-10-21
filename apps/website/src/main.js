// SPDX-License-Identifier: GPL-3.0-or-later

import logoUrl from "../../desktop/src/assets/logo.png";
import { version as appVersion } from "../../desktop/package.json";
import {
  LANGUAGE_STORAGE_KEY,
  isLanguagePreference,
  resolveLanguage,
} from "../../desktop/src/lib/language";
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

// The Live Demo writes `ice-box.language` when the visitor changes the
// language there; a storage event never reaches the document that wrote it.
window.addEventListener("storage", (event) => {
  if (event.key !== LANGUAGE_STORAGE_KEY) return;
  if (!isLanguagePreference(event.newValue)) return;
  applyCopy(resolveLanguage(event.newValue));
});

applyCopy(language);
