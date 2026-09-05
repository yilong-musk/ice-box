import logoUrl from "../../desktop/src/assets/logo.png";
import "./style.css";

const icon = (path) => `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="${path}" /></svg>`;

document.querySelector("#root").innerHTML = `
  <header class="site-nav">
    <a class="brand" href="#top"><img src="${logoUrl}" alt="ice-box logo"><span>ice-box</span></a>
    <nav class="site-links" aria-label="Main navigation"><a href="#demo">Demo</a><a href="#features">Features</a><a href="#architecture">Architecture</a></nav>
    <a class="nav-github" href="https://github.com/yilong-musk/ice-box" target="_blank" rel="noreferrer">GitHub ↗</a>
  </header>
  <main id="top">
    <section class="hero page-wrap">
      <div class="hero-copy">
        <p class="eyebrow"><span class="eyebrow-dot"></span> OPEN SOURCE · DESKTOP PROXY CLIENT</p>
        <h1>Control your<br><em>network, clearly.</em></h1>
        <p class="hero-lede">A focused desktop client for nodes, rules, subscriptions, and live traffic.</p>
        <div class="hero-actions"><a class="button button-primary" href="#demo">Open live demo <span>↓</span></a><a class="text-link" href="https://github.com/yilong-musk/ice-box" target="_blank" rel="noreferrer">View source ↗</a></div>
        <div class="hero-meta"><span><b class="status-dot green"></b> v0.1.3</span><span>MIT licensed</span><span>macOS · Windows</span></div>
      </div>
      <div class="hero-aside" aria-hidden="true"><div class="signal-card"><div class="signal-top"><span>ICE-BOX / NETWORK</span><span>01</span></div><div class="signal-grid"><i></i><i></i><i></i><b class="node-a"></b><b class="node-b"></b><b class="node-c"></b></div><div class="signal-bottom"><span>private by default</span><strong>↗</strong></div></div></div>
    </section>
    <section class="demo-section page-wrap" id="demo"><div class="section-intro"><div><p class="eyebrow">THE REAL FRONTEND</p><h2>Try ice-box<br><em>in your browser.</em></h2></div><p>Interactive demo powered by the same desktop UI in this repository. No install required.</p></div><div class="app-window"><div class="window-bar"><span class="window-dots"><i></i><i></i><i></i></span><strong>ice-box <small>live product demo</small></strong><span>v0.1.3</span></div><iframe title="ice-box real desktop frontend demo" src="./demo.html"></iframe></div></section>
    <section class="principles page-wrap" id="features"><div class="section-intro compact"><p class="eyebrow">CORE FEATURES</p><h2>Everything important,<br><em>close at hand.</em></h2></div><div class="principle-grid"><article>${icon("M12 3v18M3 12h18")}<h3>Nodes</h3><p>Choose and inspect active endpoints from one focused workspace.</p></article><article>${icon("M4 6h16M4 12h10M4 18h7")}<h3>Rules</h3><p>Read routing intent and current state without digging through config.</p></article><article>${icon("M3 12h4l2-7 4 14 2-7h6")}<h3>Live traffic</h3><p>See connection status and traffic movement as it happens.</p></article></div></section>
    <section class="architecture page-wrap" id="architecture"><div><p class="eyebrow">ARCHITECTURE</p><h2>Native shell.<br><em>Local control.</em></h2><p>The React and Tauri interface talks to a local Rust core, while sing-box handles routing underneath.</p><a class="text-link" href="https://github.com/yilong-musk/ice-box#readme" target="_blank" rel="noreferrer">Read the project notes ↗</a></div><div class="arch-diagram"><div class="arch-box top">ice-box UI <small>React + Tauri</small></div><i></i><div class="arch-row"><div class="arch-box">ice-core <small>status · traffic</small></div><div class="arch-box">subscriptions <small>Clash · sing-box</small></div></div><i></i><div class="arch-box bottom">sing-box <small>routing engine</small></div></div></section>
    <section class="final-cta page-wrap"><p class="eyebrow">OPEN SOURCE</p><h2>Make the network<br><em>feel understandable.</em></h2><a class="button button-primary" href="https://github.com/yilong-musk/ice-box" target="_blank" rel="noreferrer">Get ice-box on GitHub ↗</a></section>
  </main>
  <footer class="site-footer page-wrap"><span>© 2026 ice-box</span><span>Built for quieter networks.</span><a href="https://github.com/yilong-musk/ice-box" target="_blank" rel="noreferrer">github.com/yilong-musk/ice-box ↗</a></footer>
`;
