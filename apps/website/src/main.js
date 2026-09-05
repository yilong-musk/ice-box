import "./style.css";

const nodes = [
  { name: "Tokyo / edge-01", protocol: "VMess", flag: "JP", latency: 42, load: "18%", color: "green" },
  { name: "Singapore / edge-02", protocol: "Trojan", flag: "SG", latency: 68, load: "31%", color: "blue" },
  { name: "Los Angeles / edge-03", protocol: "Shadowsocks", flag: "US", latency: 141, load: "54%", color: "purple" },
  { name: "Frankfurt / edge-04", protocol: "Hysteria 2", flag: "DE", latency: 188, load: "27%", color: "orange" },
];

const logs = [
  ["12:04:31", "info", "route match", "api.github.com → Tokyo / edge-01"],
  ["12:04:28", "ok", "health check", "Tokyo / edge-01 responded in 42 ms"],
  ["12:04:16", "info", "subscription", "Profile refreshed · 18 nodes"],
  ["12:03:52", "warn", "dns", "Using fallback resolver 1.1.1.1"],
];

const state = {
  tab: "home",
  running: true,
  mode: "rule",
  selectedNode: 0,
  tun: false,
  query: "",
  subscriptions: 2,
  latencyMessage: "",
};

const icon = (name) => ({
  home: "⌂", nodes: "◈", rules: "≡", subs: "↻", logs: "▤", settings: "⚙",
}[name] || "·");

function render() {
  document.querySelector("#root").innerHTML = `
    <header class="site-nav">
      <a class="brand" href="#top" aria-label="ice-box home"><span class="brand-mark">✦</span><span>ice-box</span></a>
      <nav class="site-links"><a href="#demo">Product</a><a href="#how">How it works</a><a href="#docs">Docs</a></nav>
      <a class="nav-github" href="https://github.com/supreulu/ice-box" target="_blank" rel="noreferrer">GitHub <span>↗</span></a>
    </header>

    <main id="top">
      <section class="hero page-wrap">
        <div class="hero-copy">
          <p class="eyebrow"><span class="eyebrow-dot"></span> OPEN SOURCE · DESKTOP PROXY CLIENT</p>
          <h1>A calmer way to<br><em>route the internet.</em></h1>
          <p class="hero-lede">ice-box gives macOS and Windows a focused control surface for nodes, rules, subscriptions, and live traffic — powered by sing-box.</p>
          <div class="hero-actions"><a class="button button-dark" href="#demo">Try the live demo <span>↓</span></a><a class="text-link" href="https://github.com/supreulu/ice-box" target="_blank" rel="noreferrer">View source <span>↗</span></a></div>
          <div class="hero-meta"><span><b class="status-dot green"></b> v0.1.3</span><span>MIT licensed</span><span>macOS · Windows</span></div>
        </div>
        <div class="hero-aside"><div class="signal-card"><div class="signal-top"><span>NETWORK / 01</span><span class="mono">ICE-BOX</span></div><div class="signal-grid"><span class="signal-line"></span><span class="signal-line"></span><span class="signal-line"></span><div class="signal-node node-a"></div><div class="signal-node node-b"></div><div class="signal-node node-c"></div></div><div class="signal-bottom"><span>private by default</span><strong>↗</strong></div></div></div>
      </section>

      <section class="demo-section page-wrap" id="demo">
        <div class="section-intro"><p class="eyebrow">THE PRODUCT, IN MOTION</p><h2>A real interface,<br><em>without the setup.</em></h2><p>Explore the same visual language as the desktop app. Switch pages, change modes, test a node, and see the state update instantly.</p></div>
        <div class="app-window" aria-label="Interactive ice-box product demo">
          <div class="window-bar"><div class="window-dots"><span></span><span></span><span></span></div><span class="window-title">ice-box <small>demo environment</small></span><span class="window-version">v0.1.3</span></div>
          <div class="app-body"><aside class="app-sidebar"><div class="sidebar-label">WORKSPACE</div><nav class="app-nav">${["home", "nodes", "rules", "subs", "logs", "settings"].map((tab) => `<button class="app-nav-item ${state.tab === tab ? "active" : ""}" data-tab="${tab}"><span class="nav-icon">${icon(tab)}</span><span>${tab === "subs" ? "Subscriptions" : tab[0].toUpperCase() + tab.slice(1)}</span></button>`).join("")}</nav><div class="sidebar-footer"><div class="footer-mark">✦</div><span>ice-box</span><small>demo mode</small></div></aside><section class="app-content"><div class="content-title"><div><span class="content-kicker">ICE-BOX / ${state.tab.toUpperCase()}</span><h3>${titleFor(state.tab)}</h3></div><span class="live-pill"><b class="status-dot ${state.running ? "green" : "gray"}"></b>${state.running ? "Running" : "Stopped"}</span></div>${pageFor(state.tab)}</section></div>
        </div>
      </section>

      <section class="principles page-wrap" id="how"><div class="section-intro compact"><p class="eyebrow">BUILT AROUND CLARITY</p><h2>Small surface.<br><em>Serious control.</em></h2></div><div class="principle-grid"><article><span class="principle-number">01</span><h3>One focused workspace</h3><p>Every important action is close: choose a node, change a mode, inspect a rule, or see what is happening right now.</p></article><article><span class="principle-number">02</span><h3>Rules you can read</h3><p>Human-sized tables and explicit state make it easy to understand why traffic took a particular path.</p></article><article><span class="principle-number">03</span><h3>Native where it counts</h3><p>A lightweight Tauri shell keeps the UI fast while sing-box handles the networking work underneath.</p></article></div></section>

      <section class="architecture page-wrap" id="docs"><div class="arch-copy"><p class="eyebrow">UNDER THE SURFACE</p><h2>Designed to stay<br><em>out of your way.</em></h2><p>ice-box combines a native desktop shell with a small Rust workspace. The interface talks to a local core, so settings and traffic stay on your machine.</p><a class="text-link" href="https://github.com/supreulu/ice-box#readme" target="_blank" rel="noreferrer">Read the architecture notes <span>↗</span></a></div><div class="arch-diagram"><div class="arch-box top">ice-box UI <small>React + Tauri</small></div><div class="arch-connector"></div><div class="arch-row"><div class="arch-box">ice-core <small>status · traffic · reload</small></div><div class="arch-box">ice-subscription <small>Clash · sing-box · links</small></div></div><div class="arch-connector short"></div><div class="arch-box bottom">sing-box <small>routing engine</small></div></div></section>

      <section class="final-cta page-wrap"><p class="eyebrow">READY WHEN YOU ARE</p><h2>Make the network<br><em>feel understandable.</em></h2><a class="button button-dark" href="https://github.com/supreulu/ice-box" target="_blank" rel="noreferrer">Get ice-box on GitHub <span>↗</span></a></section>
    </main>
    <footer class="site-footer page-wrap"><span>© 2026 ice-box</span><span>Made for quieter networks.</span><a href="https://github.com/supreulu/ice-box" target="_blank" rel="noreferrer">github.com/supreulu/ice-box ↗</a></footer>
  `;
  bindEvents();
}

function titleFor(tab) { return ({ home: "Overview", nodes: "Nodes", rules: "Rules", subs: "Subscriptions", logs: "Logs", settings: "Settings" })[tab]; }

function pageFor(tab) {
  if (tab === "home") return `<div class="home-page"><div class="dashboard-grid"><div class="demo-card traffic-card"><div class="card-heading"><div><span class="mini-label">TRAFFIC</span><strong>Live throughput</strong></div><span class="card-action">Last 5 min · ↗</span></div><div class="traffic-reading"><span class="traffic-value">${state.running ? "1.84" : "0.00"}</span><span class="traffic-unit">MB/s</span></div><svg class="traffic-chart" viewBox="0 0 600 150" preserveAspectRatio="none"><defs><linearGradient id="chartFill" x1="0" x2="0" y1="0" y2="1"><stop offset="0" stop-color="#5eb38b" stop-opacity=".28"/><stop offset="1" stop-color="#5eb38b" stop-opacity="0"/></linearGradient></defs><path d="M0 120 C35 110 38 72 74 91 S110 115 142 72 S190 86 220 67 S258 96 290 48 S330 76 359 62 S393 110 426 70 S470 93 501 37 S542 62 600 24 V150 H0Z" fill="url(#chartFill)"/><path d="M0 120 C35 110 38 72 74 91 S110 115 142 72 S190 86 220 67 S258 96 290 48 S330 76 359 62 S393 110 426 70 S470 93 501 37 S542 62 600 24" fill="none" stroke="#4b9f7a" stroke-width="2"/></svg><div class="chart-axis"><span>12:00</span><span>12:01</span><span>12:02</span><span>12:03</span><span>12:04</span></div></div><div class="demo-card control-card"><div class="card-heading"><div><span class="mini-label">CORE</span><strong>Service control</strong></div><span class="status-label"><b class="status-dot ${state.running ? "green" : "gray"}"></b>${state.running ? "Active" : "Paused"}</span></div><button class="power-button ${state.running ? "on" : ""}" data-action="power"><span class="power-symbol">⏻</span><span>${state.running ? "Stop service" : "Start service"}</span></button><div class="mode-label">Proxy mode</div><div class="segmented">${["rule", "global", "direct"].map((m) => `<button data-mode="${m}" class="${state.mode === m ? "selected" : ""}">${m[0].toUpperCase() + m.slice(1)}</button>`).join("")}</div><div class="selected-route"><span>Selected outbound</span><strong>${nodes[state.selectedNode].name}</strong><button data-tab="nodes">Change ↗</button></div></div></div><div class="dashboard-bottom"><div class="demo-card stat-card"><span class="mini-label">CURRENT NODE</span><div class="node-stat"><span class="node-flag">${nodes[state.selectedNode].flag}</span><div><strong>${nodes[state.selectedNode].name}</strong><small>${nodes[state.selectedNode].protocol} · ${nodes[state.selectedNode].latency} ms</small></div></div></div><div class="demo-card stat-card"><span class="mini-label">TUN INTERFACE</span><div class="tun-row"><strong>${state.tun ? "Enabled" : "Disabled"}</strong><button class="tiny-toggle ${state.tun ? "on" : ""}" data-action="tun"><span></span></button></div><small>Applies on next start</small></div><div class="demo-card stat-card"><span class="mini-label">RULES MATCHED</span><div class="big-stat">1,284 <small>today</small></div><small>84% routed by rule</small></div></div></div>`;
  if (tab === "nodes") return `<div class="page-panel"><div class="panel-toolbar"><span>${nodes.length} available nodes</span><button class="outline-button" data-action="latency">${state.latencyMessage || "Test latency ↗"}</button></div><div class="node-table">${nodes.map((n, i) => `<button class="node-row ${i === state.selectedNode ? "selected" : ""}" data-node="${i}"><span class="node-flag">${n.flag}</span><span class="node-main"><strong>${n.name}</strong><small>${n.protocol}</small></span><span class="node-load"><i style="width:${n.load}"></i></span><span class="node-latency">${n.latency} ms</span><span class="row-arrow">${i === state.selectedNode ? "✓" : "↗"}</span></button>`).join("")}</div></div>`;
  if (tab === "rules") { const filtered = ["Domain: github.com → Tokyo / edge-01", "Domain: *.googleapis.com → Singapore / edge-02", "GeoIP: CN → DIRECT", "Final → Proxy group"].filter((r) => r.toLowerCase().includes(state.query.toLowerCase())); return `<div class="page-panel"><div class="panel-toolbar"><input class="search-input" placeholder="Search rules" value="${state.query}" data-action="search"/><span>${filtered.length} of 4 rules</span><button class="outline-button">+ Add rule</button></div><div class="rule-list">${filtered.map((r, i) => `<div class="rule-row"><span class="rule-index">0${i + 1}</span><span>${r}</span><span class="rule-kind">${i === 2 ? "GEOIP" : "DOMAIN"}</span></div>`).join("") || `<div class="empty-state">No rules match “${state.query}”.</div>`}</div></div>`; }
  if (tab === "subs") return `<div class="page-panel"><div class="sub-form"><div><span class="mini-label">IMPORT PROFILE</span><strong>Add a subscription</strong></div><form data-action="add-sub"><input name="url" required placeholder="https://example.com/profile.yaml"/><button class="button button-dark" type="submit">Import</button></form></div><div class="subscription-list"><div class="subscription-row"><span class="sub-icon">↻</span><span><strong>Personal profile</strong><small>Clash · 18 nodes · updated 4 min ago</small></span><b class="status-label"><i class="status-dot green"></i>Active</b></div><div class="subscription-row"><span class="sub-icon">↻</span><span><strong>Fallback profile</strong><small>sing-box JSON · 6 nodes · updated yesterday</small></span><span class="muted">${state.subscriptions} profiles</span></div></div></div>`;
  if (tab === "logs") return `<div class="page-panel"><div class="panel-toolbar"><span>Recent activity · ${logs.length} events</span><button class="outline-button">Clear logs</button></div><div class="log-list">${logs.map(([time, type, label, text]) => `<div class="log-row"><span class="log-time">${time}</span><span class="log-type ${type}">${type}</span><span><strong>${label}</strong><small>${text}</small></span></div>`).join("")}</div></div>`;
  return `<div class="page-panel settings-panel"><div class="setting-row"><span><strong>Start with system proxy</strong><small>Apply the selected mode when ice-box launches.</small></span><button class="tiny-toggle on"><span></span></button></div><div class="setting-row"><span><strong>TUN interface</strong><small>Capture traffic at the network layer on next start.</small></span><button class="tiny-toggle ${state.tun ? "on" : ""}" data-action="tun"><span></span></button></div><div class="setting-row"><span><strong>Appearance</strong><small>Use the same quiet light palette as the desktop app.</small></span><span class="setting-value">Light <span>⌄</span></span></div><div class="settings-note"><span>✓</span> Changes are saved automatically in the desktop app.</div></div>`;
}

function bindEvents() {
  document.querySelectorAll("[data-tab]").forEach((el) => el.addEventListener("click", () => { state.tab = el.dataset.tab; render(); if (state.tab !== "home") document.querySelector("#demo").scrollIntoView({ behavior: "smooth", block: "center" }); }));
  document.querySelectorAll("[data-mode]").forEach((el) => el.addEventListener("click", () => { state.mode = el.dataset.mode; render(); }));
  document.querySelectorAll("[data-node]").forEach((el) => el.addEventListener("click", () => { state.selectedNode = Number(el.dataset.node); render(); }));
  document.querySelectorAll('[data-action="power"]').forEach((el) => el.addEventListener("click", () => { state.running = !state.running; render(); }));
  document.querySelectorAll('[data-action="tun"]').forEach((el) => el.addEventListener("click", () => { state.tun = !state.tun; render(); }));
  document.querySelectorAll('[data-action="latency"]').forEach((el) => el.addEventListener("click", () => { state.latencyMessage = "Testing…"; render(); window.setTimeout(() => { state.latencyMessage = `${nodes[state.selectedNode].latency} ms · Updated`; render(); }, 700); }));
  const search = document.querySelector('[data-action="search"]'); if (search) search.addEventListener("input", (e) => { state.query = e.target.value; render(); const input = document.querySelector('[data-action="search"]'); input?.focus(); input?.setSelectionRange(state.query.length, state.query.length); });
  const form = document.querySelector('[data-action="add-sub"]'); if (form) form.addEventListener("submit", (e) => { e.preventDefault(); state.subscriptions += 1; state.tab = "subs"; render(); });
}

render();
