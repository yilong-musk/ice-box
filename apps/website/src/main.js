// SPDX-License-Identifier: GPL-3.0-or-later

import logoUrl from "../../desktop/src/assets/logo.png";
import { version as appVersion } from "../../desktop/package.json";
import {
  LANGUAGE_STORAGE_KEY,
  isLanguagePreference,
  resolveLanguage,
} from "../../desktop/src/lib/language";
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
          <a class="button button-primary" href="#demo"><span class="button-label"></span> <span>↓</span></a>
          <a class="text-link" href="https://github.com/yilong-musk/ice-box/releases/latest" target="_blank" rel="noreferrer"></a>
        </div>
        <div class="hero-meta">
          <span><b class="status-dot green"></b> ${displayVersion}</span>
          <span>GPL-3.0-or-later</span>
          <span>macOS · Windows</span>
        </div>
      </div>
    </section>
    <section class="demo-section page-wrap" id="demo"><div class="app-window"><div class="window-bar"><span class="window-dots"><i></i><i></i><i></i></span><strong>ice-box <small></small></strong><span>${displayVersion}</span></div><iframe src="./demo.html"></iframe></div></section>
  </main>
  <footer class="site-footer page-wrap"><span>© 2026 ice-box</span><a href="https://github.com/yilong-musk/ice-box" target="_blank" rel="noreferrer">github.com/yilong-musk/ice-box ↗</a></footer>
`;

const parts = {
  headline: root.querySelector(".hero-copy h1"),
  demoLabel: root.querySelector(".button-label"),
  download: root.querySelector(".text-link"),
  windowBar: root.querySelector(".window-bar small"),
  demoFrame: root.querySelector(".app-window iframe"),
  languageToggle: root.querySelector(".nav-language"),
};

/** Write every locale-dependent piece; runs once before the first paint and
 * again on each language change. The embedded demo is never re-rendered, so
 * its state survives: it follows through the shared `ice-box.language` key. */
function applyCopy(next) {
  language = next;
  applyMarketingLanguage(language);
  parts.headline.innerHTML = marketingText("hero.title", language);
  parts.demoLabel.textContent = marketingText("hero.demo", language);
  parts.download.textContent = marketingText("hero.download", language);
  parts.windowBar.textContent = marketingText("demo.windowBar", language);
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
