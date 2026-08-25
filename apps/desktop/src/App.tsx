import { useEffect, useState } from "react";
import { api, type StatusResponse } from "./api/tauri";
import { Home } from "./pages/Home";
import { Nodes } from "./pages/Nodes";
import { Subscriptions } from "./pages/Subscriptions";
import { Rules } from "./pages/Rules";
import { Logs } from "./pages/Logs";
import { Settings } from "./pages/Settings";
import "./App.css";

type Tab = "home" | "nodes" | "subs" | "rules" | "logs" | "settings";

const NAV_ITEMS: { id: Tab; label: string }[] = [
  { id: "home", label: "主页" },
  { id: "nodes", label: "节点" },
  { id: "rules", label: "规则" },
  { id: "subs", label: "订阅" },
  { id: "logs", label: "日志" },
  { id: "settings", label: "设置" },
];

/** SVG refraction filter for Chromium (WebView2). WebKit ignores url() in backdrop-filter. */
function LiquidGlassFilters() {
  return (
    <svg
      className="liquid-glass-defs"
      aria-hidden="true"
      width="0"
      height="0"
      focusable="false"
    >
      <defs>
        {/* Soft liquid warp — low-frequency noise displacement */}
        <filter
          id="liquid-refract"
          x="-8%"
          y="-8%"
          width="116%"
          height="116%"
          colorInterpolationFilters="sRGB"
        >
          <feTurbulence
            type="fractalNoise"
            baseFrequency="0.012 0.018"
            numOctaves="2"
            seed="3"
            result="noise"
          />
          <feGaussianBlur in="noise" stdDeviation="1.2" result="map" />
          <feDisplacementMap
            in="SourceGraphic"
            in2="map"
            scale="22"
            xChannelSelector="R"
            yChannelSelector="G"
          />
        </filter>
        {/* Stronger lens warp for the power button hero */}
        <filter
          id="liquid-refract-strong"
          x="-12%"
          y="-12%"
          width="124%"
          height="124%"
          colorInterpolationFilters="sRGB"
        >
          <feTurbulence
            type="fractalNoise"
            baseFrequency="0.01 0.014"
            numOctaves="2"
            seed="7"
            result="noise"
          />
          <feGaussianBlur in="noise" stdDeviation="1.6" result="map" />
          <feDisplacementMap
            in="SourceGraphic"
            in2="map"
            scale="32"
            xChannelSelector="R"
            yChannelSelector="G"
          />
        </filter>
      </defs>
    </svg>
  );
}

function App() {
  const [tab, setTab] = useState<Tab>("home");
  const [globalStatus, setGlobalStatus] = useState<StatusResponse | null>(null);

  useEffect(() => {
    let cancelled = false;
    const poll = async () => {
      try {
        const status = await api.getStatus();
        if (!cancelled) setGlobalStatus(status);
      } catch {
        // Background poll; tab pages surface their own errors.
      }
    };
    void poll();
    const id = window.setInterval(() => void poll(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, []);

  return (
    <div className="app-root">
      <LiquidGlassFilters />

      <div className="app-atmosphere" aria-hidden="true" />

      <div className="app">
        <aside className="sidebar liquid-glass">
          <div className="sidebar-brand">
            <span className="brand-mark" aria-hidden="true" />
            <h1>ice-box</h1>
          </div>
          <nav className="sidebar-nav" aria-label="主导航">
            {NAV_ITEMS.map(({ id, label }) => (
              <button
                key={id}
                type="button"
                className={tab === id ? "nav-item active" : "nav-item"}
                aria-current={tab === id ? "page" : undefined}
                onClick={() => setTab(id)}
              >
                <span className="nav-item-label">{label}</span>
              </button>
            ))}
          </nav>
        </aside>

        <div className="content">
          {globalStatus?.proxy_recovery_warning && (
            <p className="global-banner error" role="alert">
              {globalStatus.proxy_recovery_warning}
            </p>
          )}

          <main className="content-main">
            {tab === "home" && <Home onNavigate={setTab} />}
            {tab === "nodes" && <Nodes onNavigate={setTab} />}
            {tab === "subs" && <Subscriptions />}
            {tab === "rules" && <Rules onNavigate={setTab} />}
            {tab === "logs" && <Logs />}
            {tab === "settings" && <Settings />}
          </main>
        </div>
      </div>
    </div>
  );
}

export default App;
