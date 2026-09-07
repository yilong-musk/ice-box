// SPDX-License-Identifier: GPL-3.0-or-later

import logoUrl from "../../desktop/src/assets/logo.png";
import { version as appVersion } from "../../desktop/package.json";
import "./style.css";

/** Same source as the desktop app and the Live Demo title bar (`APP_VERSION`). */
const displayVersion = `v${appVersion}`;

const favicon = document.createElement("link");
favicon.rel = "icon";
favicon.type = "image/png";
favicon.href = logoUrl;
document.head.appendChild(favicon);

document.querySelector("#root").innerHTML = `
  <header class="site-nav">
    <a class="brand" href="#top"><img src="${logoUrl}" alt="ice-box logo"><span>ice-box</span></a>
    <a class="nav-github" href="https://github.com/yilong-musk/ice-box" target="_blank" rel="noreferrer">GitHub ↗</a>
  </header>
  <main id="top">
    <section class="hero page-wrap">
      <div class="hero-copy">
        <h1>Proxy,<br><em>kept simple.</em></h1>
      </div>
      <div class="hero-aside">
        <div class="hero-actions">
          <a class="button button-primary" href="#demo">Live Demo <span>↓</span></a>
          <a class="text-link" href="https://github.com/yilong-musk/ice-box/releases/latest" target="_blank" rel="noreferrer">Download ↗</a>
        </div>
        <div class="hero-meta">
          <span><b class="status-dot green"></b> ${displayVersion}</span>
          <span>GPL-3.0-or-later</span>
          <span>macOS · Windows</span>
        </div>
      </div>
    </section>
    <section class="demo-section page-wrap" id="demo"><div class="app-window"><div class="window-bar"><span class="window-dots"><i></i><i></i><i></i></span><strong>ice-box <small>live product demo</small></strong><span>${displayVersion}</span></div><iframe title="ice-box real desktop frontend demo" src="./demo.html"></iframe></div></section>
  </main>
  <footer class="site-footer page-wrap"><span>© 2026 ice-box</span><a href="https://github.com/yilong-musk/ice-box" target="_blank" rel="noreferrer">github.com/yilong-musk/ice-box ↗</a></footer>
`;
