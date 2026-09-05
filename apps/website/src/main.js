import logoUrl from "../../desktop/src/assets/logo.png";
import "./style.css";

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
        <p class="eyebrow"><span class="eyebrow-dot"></span> OPEN SOURCE · DESKTOP PROXY CLIENT</p>
        <h1>Control your<br><em>network, clearly.</em></h1>
      </div>
      <div class="hero-aside">
        <div class="hero-actions">
          <a class="button button-primary" href="#demo">Open live demo <span>↓</span></a>
          <a class="text-link" href="https://github.com/yilong-musk/ice-box" target="_blank" rel="noreferrer">View source ↗</a>
        </div>
        <div class="hero-meta">
          <span><b class="status-dot green"></b> v0.1.3</span>
          <span>MIT licensed</span>
          <span>macOS · Windows</span>
        </div>
      </div>
    </section>
    <section class="demo-section page-wrap" id="demo"><h2>Try ice-box</h2><div class="app-window"><div class="window-bar"><span class="window-dots"><i></i><i></i><i></i></span><strong>ice-box <small>live product demo</small></strong><span>v0.1.3</span></div><iframe title="ice-box real desktop frontend demo" src="./demo.html"></iframe></div></section>
  </main>
  <footer class="site-footer page-wrap"><span>© 2026 ice-box</span><a href="https://github.com/yilong-musk/ice-box" target="_blank" rel="noreferrer">github.com/yilong-musk/ice-box ↗</a></footer>
`;
